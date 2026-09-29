//! The whole product through MCP, offline: a fixture resume, a temporary
//! SQLite database, and mock job boards serving saved real responses for
//! both discovery (`JOBHUNT_DISCOVERY_ENDPOINT`) and verification
//! (`JOBHUNT_VERIFY_ENDPOINT`). An MCP client drives `narrow mcp` from
//! profile to preferences, search, inspection, verification, feedback,
//! a second search that reflects it, and application context, which is
//! checked claim by claim against the evidence policy in the database.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::HashSet;

use jobhunt_profile::{ProfileService, ReviewReason, Standing};
use jobhunt_storage::SqliteJobStore;
use serde_json::{Value, json};

use common::{Env, McpClient, requests, result, result_ids};

const SECURITY: &str = "Security Engineer, Cloud";
const SYSTEMS: &str = "Member of Technical Staff - Systems";
const FORWARD: &str = "Forward Deployed Engineer - ML";

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect()
}

fn values<'a>(list: &'a Value, field: &str) -> Vec<&'a str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|v| v[field].as_str().unwrap())
        .collect()
}

/// Every `claim_id` anywhere in a JSON value.
fn claim_ids(value: &Value, out: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(id)) = map.get("claim_id") {
                out.insert(id.clone());
            }
            map.values().for_each(|v| claim_ids(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| claim_ids(v, out)),
        _ => {}
    }
}

/// Checks application context against the profile stored in the database:
/// every fact is usable under the evidence policy and cites its source
/// snippet; nothing withheld leaks; counts add up.
async fn assert_only_usable_evidence(env: &Env, context: &Value) -> HashSet<String> {
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let data = ProfileService::new(&store).require().await.unwrap();
    store.close().await;
    let mut ids = HashSet::new();
    claim_ids(context, &mut ids);
    assert!(!ids.is_empty(), "some evidence is usable");
    for id in &ids {
        let claim = data
            .claims
            .iter()
            .find(|c| c.id.to_string() == *id)
            .unwrap_or_else(|| panic!("{id} is not a claim of the profile"));
        assert!(
            data.standing(claim).is_usable(),
            "{id} ({:?}, {}) is not usable but was included",
            data.standing(claim),
            claim.text
        );
    }
    let text = context.to_string();
    let mut withheld = 0;
    for claim in &data.claims {
        match data.standing(claim) {
            Standing::Usable(_) => {}
            _ => {
                withheld += 1;
                assert!(!ids.contains(&claim.id.to_string()));
                assert!(
                    !text.contains(&claim.text),
                    "withheld claim text leaked: {}",
                    claim.text
                );
            }
        }
    }
    let w = &context["withheld"];
    assert_eq!(
        w["needs_review"].as_u64().unwrap() + w["rejected"].as_u64().unwrap(),
        withheld
    );
    // Provenance: every fact from the resume quotes it.
    let docs: HashSet<String> = data.documents.iter().map(|d| d.id.to_string()).collect();
    for group in context["relevant_experience"].as_array().unwrap() {
        for fact in group["facts"].as_array().unwrap() {
            if fact["usable_because"] == "quoted_from_resume" {
                let source = &fact["source"];
                assert!(!source["snippet"].as_str().unwrap().is_empty());
                assert!(docs.contains(source["document"].as_str().unwrap()));
            }
        }
    }
    assert!(
        context["policy"]
            .as_str()
            .unwrap()
            .contains("do not add metrics")
    );
    ids
}

#[tokio::test(flavor = "multi_thread")]
async fn the_whole_loop_through_mcp() {
    let env = Env::new().await;
    // The resume is imported with the CLI (a file on this machine); from
    // here on, everything goes through the MCP client.
    env.ok(&["init", common::resume("marina_costa.pdf").to_str().unwrap()]);
    let mut mcp = env.mcp().await;
    mcp.initialize().await;

    // 1. The profile: professional context, no contact details.
    let profile = mcp.call("get_profile", json!({})).await;
    assert_eq!(profile["experiences"].as_array().unwrap().len(), 4);
    assert_eq!(
        profile["headline"],
        "Senior Software Engineer · Backend & Platform"
    );
    let technologies = values(&profile["technologies"], "name");
    for t in ["Rust", "PostgreSQL", "Kafka", "Terraform"] {
        assert!(technologies.contains(&t), "{t} in {technologies:?}");
    }
    assert!(profile["needs_review_total"].as_u64().unwrap() > 0);
    assert_eq!(profile["contact_details_omitted"], true);
    let text = profile.to_string();
    for private in [
        "marina.costa@example.com",
        "91234-5678",
        "Marina Costa",
        "linkedin.com/in",
    ] {
        assert!(!text.contains(private), "{private} leaked");
    }

    // 2. Preferences, in words and precisely. Nothing is silently dropped.
    let update = json!({
        "statement": "I want backend or infrastructure roles, at least USD 180k, and something about vibes",
        "set": [
            {"kind": "location", "place": "New York, NY"},
            {"kind": "authorized_in", "place": "United States"}
        ]
    });
    let prefs = mcp.call("update_preferences", update.clone()).await;
    let interpreted = values(&prefs["interpreted"], "value");
    for v in [
        "backend roles",
        "infrastructure roles",
        "at least USD 180,000 per year",
        "based in New York, NY",
        "authorized to work in United States",
    ] {
        assert!(interpreted.contains(&v), "{v} in {interpreted:?}");
    }
    assert!(
        strings(&prefs["not_understood"])
            .iter()
            .any(|p| p.contains("vibes"))
    );
    assert_eq!(prefs["statement"]["reading"], "partial");
    assert_eq!(prefs["unchanged"], false);
    // A client retrying the same update changes nothing.
    let retry = mcp.call("update_preferences", update).await;
    assert_eq!(retry["unchanged"], true);
    assert_eq!(retry["active"], prefs["active"]);

    // 3. The search: refreshes the boards, verifies the best candidates,
    // and returns a short, explained list.
    let first = mcp.call("search_jobs", json!({"refresh": "always"})).await;
    let refresh = &first["refresh"];
    assert_eq!(refresh["performed"], true);
    assert_eq!(refresh["sources_read"], 4);
    assert_eq!(refresh["new_jobs"], 11);
    assert_eq!(first["funnel"]["checked"], 11);
    assert!(first["verified_now"].as_u64().unwrap() >= 2);
    let results = first["results"].as_array().unwrap();
    assert!(!results.is_empty() && results.len() <= 5, "{first:#}");
    for r in results {
        assert!(r["id"].as_str().unwrap().starts_with("opp_"));
        for field in ["company", "title", "summary", "next_step"] {
            assert!(!r[field].as_str().unwrap().is_empty(), "{field}");
        }
        assert!(
            ["strong_fit", "worth_reviewing"].contains(&r["tier"].as_str().unwrap()),
            "{r}"
        );
        assert!(r["verification"]["state"].is_string());
        assert!(r["eligibility"]["status"].is_string());
        assert!(!r["why"].as_array().unwrap().is_empty());
    }
    assert!(!first.to_string().contains('%'), "no percentages");
    for title in [SECURITY, SYSTEMS] {
        let r = result(&first, title);
        assert_eq!(r["tier"], "strong_fit");
        assert_eq!(r["recommendation"], "recommended");
        assert_eq!(r["verification"]["trusted"], true);
        assert_eq!(r["verification"]["state"], "verified_active");
        assert_eq!(r["eligibility"]["status"], "eligible");
    }
    let security = result(&first, SECURITY)["id"].as_str().unwrap().to_owned();
    let systems = result(&first, SYSTEMS)["id"].as_str().unwrap().to_owned();
    let forward = result(&first, FORWARD)["id"].as_str().unwrap().to_owned();

    // 4. One opportunity in detail, with its sources.
    let job = mcp
        .call("get_job", json!({"id": security, "include_sources": true}))
        .await;
    assert_eq!(job["company"], "Ramp");
    assert_eq!(job["decision"]["tier"], "strong_fit");
    assert!(
        strings(&job["decision"]["worth"])
            .contains(&"Infrastructure roles: a role you want".to_owned())
    );
    assert_eq!(
        job["pipeline"]["stage"], "unseen",
        "looking is not feedback"
    );
    assert_eq!(job["compensation"]["verified"], true);
    assert_eq!(
        job["compensation"]["ranges"][0],
        "USD 211,400 – 290,600 per year"
    );
    assert_eq!(job["eligibility"]["status"], "eligible");
    assert!(
        job["eligibility"]["disclaimer"]
            .as_str()
            .unwrap()
            .contains("not legal advice")
    );
    let sources = job["sources"].as_array().unwrap();
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0]["source"], "ashby:ramp");
    assert!(sources[0]["job_id"].as_str().unwrap().starts_with("job_"));
    assert_eq!(sources[0]["last_attempt"]["listing"], "active");
    let brief = mcp.call("get_job", json!({"id": security})).await;
    assert!(brief.get("sources").is_none(), "sources only on request");
    assert!(brief["description"].as_str().unwrap().len() <= 1300);

    // 5. Verification: reused within minutes, asked again when forced.
    let before = requests(&env.server).await;
    let reused = mcp.call("verify_job", json!({"id": security})).await;
    assert_eq!(reused["reused"], 1);
    assert_eq!(requests(&env.server).await, before, "nothing fetched");
    let verified = mcp
        .call("verify_job", json!({"id": security, "force": true}))
        .await;
    assert!(requests(&env.server).await > before);
    assert_eq!(verified["reused"], 0);
    assert_eq!(verified["listing"], "active");
    assert_eq!(verified["application"], "active");
    assert_eq!(verified["authority"], "employer_configured_ats");
    assert_eq!(verified["recommendable"], true);
    assert_eq!(verified["eligibility"]["status"], "eligible");
    assert!(verified["last_attempt_at"].is_string());
    assert_eq!(verified["last_attempt_at"], verified["last_success_at"]);
    assert_eq!(
        verified["compensation"]["ranges"][0],
        "USD 211,400 – 290,600 per year"
    );
    assert!(verified["uncertainty"].is_array());

    // 6. Feedback, on logical opportunities.
    let saved = mcp
        .call(
            "save_job",
            json!({"id": security, "reason": "interesting infra problem"}),
        )
        .await;
    assert_eq!(saved["recorded"], true);
    assert_eq!(saved["previous"]["stage"], "unseen");
    assert_eq!(saved["state"]["stage"], "saved");
    let rejected = mcp
        .call(
            "reject_job",
            json!({"id": forward, "reason": "customer-facing, too corporate"}),
        )
        .await;
    assert_eq!(rejected["state"]["stage"], "rejected");
    let reading = &rejected["interpretation"];
    assert_eq!(reading["reason"], "customer-facing, too corporate");
    assert_eq!(reading["understood"], true);
    assert!(
        strings(&reading["read_as"]).contains(&"avoid company: large companies".to_owned()),
        "{reading}"
    );
    assert!(
        rejected["note"]
            .as_str()
            .unwrap()
            .contains("won't be recommended")
    );
    let applied = mcp.call("mark_applied", json!({"id": systems})).await;
    assert_eq!(applied["state"]["stage"], "applied");
    assert_eq!(applied["state"]["feedback"].as_array().unwrap().len(), 1);

    // 7. The next search reflects it: rejected and applied opportunities
    // leave the list, the saved one stays, and the funnel says why.
    let second = mcp.call("search_jobs", json!({"refresh": "never"})).await;
    assert_eq!(second["refresh"]["performed"], false);
    assert_eq!(second["verified_now"], 0, "offline: nothing verified");
    let ids = result_ids(&second);
    assert!(ids.contains(&security));
    assert!(!ids.contains(&forward) && !ids.contains(&systems));
    assert_eq!(second["not_shown"]["rejected"], 1);
    assert_eq!(second["not_shown"]["in_pipeline"], 1);
    assert_eq!(second["learning"]["feedback"], 3);
    assert_ne!(result_ids(&first), ids);
    let pipeline = mcp.call("get_pipeline", json!({})).await;
    let entries = pipeline["entries"].as_array().unwrap();
    assert_eq!(values(&pipeline["entries"], "stage"), ["applied", "saved"]);
    assert_eq!(entries[1]["last_reason"], "interesting infra problem");

    // 8. Application context: evidence only, only what may be used.
    let context = mcp
        .call("prepare_application_context", json!({"id": security}))
        .await;
    assert_eq!(context["job"]["title"], SECURITY);
    assert_eq!(context["decision"]["tier"], "strong_fit");
    assert!(context.get("contact").is_none());
    assert!(!context.to_string().contains("marina.costa@example.com"));
    let asks: Vec<&str> = values(&context["job_asks"]["technologies"], "name");
    assert!(
        asks.contains(&"AWS") && asks.contains(&"Terraform"),
        "{asks:?}"
    );
    let aws = context["job_asks"]["technologies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "AWS")
        .unwrap();
    assert!(!aws["evidence"].as_array().unwrap().is_empty());
    let experience = context["relevant_experience"].as_array().unwrap();
    assert!(!experience.is_empty());
    assert!(
        experience[0]["relevance"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r.as_str().unwrap().contains("aws")),
        "{:#}",
        experience[0]
    );
    // The domain claims are JobHunt's inferences: withheld, and reported
    // as missing evidence instead.
    assert!(
        strings(&context["missing_evidence"])
            .iter()
            .any(|m| m.contains("security domain")),
        "{:#}",
        context["missing_evidence"]
    );
    assert_only_usable_evidence(&env, &context).await;
    let with_contact = mcp
        .call(
            "prepare_application_context",
            json!({"id": security, "include_contact_details": true}),
        )
        .await;
    assert_eq!(with_contact["contact"]["name"], "Marina Costa");
    assert!(
        with_contact["contact"]["contacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "email" && c["value"] == "marina.costa@example.com")
    );

    let transcript = mcp.close().await;
    assert!(transcript.iter().all(|m| m.get("id").is_some()));
}

#[tokio::test(flavor = "multi_thread")]
async fn application_context_follows_the_evidence_policy() {
    let env = Env::new().await;
    env.with_profile();
    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    let found = mcp.call("search_jobs", json!({"refresh": "always"})).await;
    let security = result(&found, SECURITY)["id"].as_str().unwrap().to_owned();
    let baseline = mcp
        .call("prepare_application_context", json!({"id": security}))
        .await;
    let included = assert_only_usable_evidence(&env, &baseline).await;

    // Decide about three claims through the CLI: confirm an inference,
    // reject a quoted fact.
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let data = ProfileService::new(&store).require().await.unwrap();
    store.close().await;
    let inferred = data
        .claims
        .iter()
        .find(|c| {
            c.topic.as_deref() == Some("infrastructure")
                && data.standing(c) == Standing::NeedsReview(ReviewReason::Inferred)
        })
        .expect("an inferred infrastructure claim");
    let quoted = data
        .claims
        .iter()
        .find(|c| included.contains(&c.id.to_string()) && c.topic.as_deref() == Some("aws"))
        .expect("a quoted AWS claim in the context");
    assert!(!included.contains(&inferred.id.to_string()));
    env.ok(&["claims", "confirm", &inferred.id.to_string()]);
    env.ok(&["claims", "reject", &quoted.id.to_string()]);

    let decided = mcp
        .call("prepare_application_context", json!({"id": security}))
        .await;
    let now_included = assert_only_usable_evidence(&env, &decided).await;
    assert!(
        now_included.contains(&inferred.id.to_string()),
        "confirmed: usable"
    );
    assert!(
        !now_included.contains(&quoted.id.to_string()),
        "rejected: never"
    );
    assert_eq!(
        decided["withheld"]["rejected"].as_u64().unwrap(),
        baseline["withheld"]["rejected"].as_u64().unwrap() + 1
    );
    let confirmed = decided.to_string();
    assert!(confirmed.contains("\"usable_because\":\"confirmed_by_you\""));

    // A newer resume that no longer says some things: those claims need
    // review again and leave the context.
    env.ok(&[
        "init",
        common::resume("marina_costa_v2.pdf").to_str().unwrap(),
    ]);
    let store = SqliteJobStore::open(&env.db()).await.unwrap();
    let data = ProfileService::new(&store).require().await.unwrap();
    store.close().await;
    let stale: Vec<String> = data
        .claims
        .iter()
        .filter(|c| data.standing(c) == Standing::NeedsReview(ReviewReason::SourceRemoved))
        .map(|c| c.id.to_string())
        .collect();
    assert!(!stale.is_empty(), "the new resume dropped something");
    let after = mcp
        .call("prepare_application_context", json!({"id": security}))
        .await;
    let after_ids = assert_only_usable_evidence(&env, &after).await;
    for id in &stale {
        assert!(!after_ids.contains(id), "stale claim {id} included");
    }
    assert!(
        stale
            .iter()
            .any(|id| included.contains(id) || now_included.contains(id))
    );
    mcp.close().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn search_keeps_the_ranking_rules() {
    let env = Env::new().await;
    env.with_profile();
    env.ok(&[
        "preferences",
        "add",
        "I want backend or infrastructure roles, at least USD 180k",
    ]);
    // Won't relocate: the office jobs abroad are left out, as a conflict
    // with what the person stated rather than as "can't take it".
    env.ok(&["preferences", "set", "relocation", "no"]);
    let mut mcp = env.mcp().await;
    mcp.initialize().await;
    let all = json!({"refresh": "always", "include_lower_tiers": true, "limit": 25});
    let first = mcp.call("search_jobs", all.clone()).await;
    let titles: Vec<&str> = values(&first["results"], "title");
    // Jobs someone can't take, or that conflict with what they require,
    // are not recommended, even among the lower tiers.
    let not_shown = &first["not_shown"];
    assert!(
        not_shown["ineligible"].as_u64().unwrap()
            + not_shown["unmet_requirement"].as_u64().unwrap()
            >= 3,
        "{not_shown:#}"
    );
    assert!(
        not_shown["unmet_requirement"].as_u64().unwrap() >= 1,
        "refusing to relocate is a stated conflict: {not_shown:#}"
    );
    for abroad in [
        "Account Executive, Enterprise (Berlin, Germany)",
        "Account Executive, Enterprise (Bengaluru, India)",
        "Solutions Engineer, Europe",
    ] {
        assert!(!titles.contains(&abroad), "{abroad} is ineligible");
    }
    // Unpublished pay is unknown, not low.
    let fullstack = result(&first, "Senior / Staff Fullstack Engineer");
    assert!(
        strings(&fullstack["consider"])
            .contains(&"Pay isn't published: unknown, not low".to_owned())
            || fullstack["summary"]
                .as_str()
                .unwrap()
                .contains("pay not published"),
        "{fullstack:#}"
    );
    assert_eq!(first["not_shown"]["below_pay_minimum"], 0);
    assert!(!first.to_string().contains('%'));

    // Rejected and applied opportunities don't come back; explicit
    // preferences outrank what feedback teaches.
    let systems = result(&first, SYSTEMS)["id"].as_str().unwrap().to_owned();
    let forward = result(&first, FORWARD)["id"].as_str().unwrap().to_owned();
    let security = result(&first, SECURITY)["id"].as_str().unwrap().to_owned();
    mcp.call(
        "reject_job",
        json!({"id": systems, "reason": "no infrastructure work please"}),
    )
    .await;
    mcp.call(
        "reject_job",
        json!({"id": forward, "reason": "no infrastructure work please"}),
    )
    .await;
    let second = mcp
        .call(
            "search_jobs",
            json!({"refresh": "never", "include_lower_tiers": true, "limit": 25}),
        )
        .await;
    let ids = result_ids(&second);
    assert!(!ids.contains(&systems) && !ids.contains(&forward));
    assert_eq!(second["not_shown"]["rejected"], 2);
    let kept = result(&second, SECURITY);
    assert!(
        strings(&kept["why"]).contains(&"Infrastructure roles: a role you want".to_owned()),
        "what you said wins over what feedback suggests: {kept:#}"
    );
    mcp.call("mark_applied", json!({"id": security})).await;
    let third = mcp
        .call(
            "search_jobs",
            json!({"refresh": "never", "include_lower_tiers": true, "limit": 25}),
        )
        .await;
    assert!(!result_ids(&third).contains(&security));
    assert_eq!(third["not_shown"]["in_pipeline"], 1);
    // Saving a rejected opportunity brings it back.
    mcp.call("save_job", json!({"id": forward})).await;
    let fourth = mcp
        .call(
            "search_jobs",
            json!({"refresh": "never", "include_lower_tiers": true, "limit": 25}),
        )
        .await;
    assert!(result_ids(&fourth).contains(&forward));
    mcp.close().await;

    // The CLI sees the same learned taste, and what the person said wins.
    let taste = env.ok(&["taste"]);
    assert!(taste.contains("Covered by what you said"), "{taste}");
}

#[allow(dead_code)]
async fn debug(mcp: &mut McpClient, tool: &str, args: Value) {
    let v = mcp.call(tool, args).await;
    eprintln!("{tool}: {v:#}");
}
