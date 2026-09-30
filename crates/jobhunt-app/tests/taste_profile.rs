//! BRU-321 through the application: one sentence about what the person
//! wants becomes a concise taste profile (read by a model or by the
//! built-in rules), practical constraints stay constraints, corrections
//! win, and nothing is interpreted twice.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_app::preferences::{PreferenceUpdate, StanceInput};
use jobhunt_app::taste_profile::{PolarityInput, TasteAction, TasteItemView, TasteProfileView};
use jobhunt_app::{AppConfig, LoadedConfig, LocalApp, TasteModel};
use jobhunt_profile::taste::reading::{
    InterpretError, ReadAssertion, TasteInterpreter, TasteReading, TasteRequest,
};
use jobhunt_profile::taste::{Polarity, TasteConfidence, TasteDimension, TasteOrigin, TasteSource};
use jobhunt_profile::{CompanyTrait, PreferenceValue, Stance};
use jobhunt_storage::SqliteJobStore;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap()
}

fn later(minutes: i64) -> DateTime<Utc> {
    now() + Duration::minutes(minutes)
}

const RESUME: &str = "\
# Ana Example
São Paulo, Brazil · ana@example.com

## Experience

### Senior Software Engineer — Orbitly
2021 – Present
- Owned the backend platform for payments, in TypeScript and Go.
- Led the migration to Kubernetes.

### Software Engineer — Ledgerline
2017 – 2021
- Built APIs in Node.js on PostgreSQL.
";

const SENIOR: &str = "I like small technical teams where I can own things end to end. \
Backend/platform work, startups or small growth companies, remote. I don't want early-career \
roles or giant process-heavy companies.";

async fn app(model: Option<Arc<dyn TasteInterpreter>>) -> LocalApp {
    let app = LocalApp::with_store(
        LoadedConfig {
            config: AppConfig::default(),
            file: None,
            default_file: None,
            database: PathBuf::from(":memory:"),
        },
        SqliteJobStore::open_in_memory().await.unwrap(),
    )
    .with_taste_model(TasteModel {
        model,
        problem: None,
    });
    app.import_resume(RESUME.as_bytes(), Some("resume.md".into()), now())
        .await
        .unwrap();
    app
}

/// A model double: answers with a canned reading (or fails), and counts
/// what it was asked.
struct Fake {
    calls: AtomicUsize,
    fail: bool,
    requests: std::sync::Mutex<Vec<String>>,
}

impl Fake {
    fn new(fail: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: AtomicUsize::new(0),
            fail,
            requests: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

fn read(d: TasteDimension, v: &str, p: Polarity, quote: &str) -> ReadAssertion {
    ReadAssertion {
        dimension: d,
        value: v.into(),
        polarity: p,
        confidence: TasteConfidence::High,
        text: jobhunt_profile::taste::vocab::sentence(d, v, p),
        explanation: Some("Said so.".into()),
        origin: TasteOrigin::Interpreted,
        sources: vec![TasteSource::Words {
            quote: quote.into(),
            statement: None,
        }],
    }
}

#[async_trait]
impl TasteInterpreter for Fake {
    fn name(&self) -> String {
        "model/fake:test".into()
    }

    fn is_remote(&self) -> bool {
        true
    }

    async fn interpret(&self, request: &TasteRequest) -> Result<TasteReading, InterpretError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.requests
            .lock()
            .unwrap()
            .push(jobhunt_profile::taste::reading::render(request));
        if self.fail {
            return Err(InterpretError::Status {
                status: 503,
                retryable: true,
            });
        }
        let words = request.all_words();
        let mut assertions = Vec::new();
        if words.contains("early-stage") {
            assertions.push(read(
                TasteDimension::Company,
                "early_stage",
                Polarity::Prefer,
                "early-stage",
            ));
        }
        if words.contains("small technical teams") {
            assertions.push(read(
                TasteDimension::Team,
                "small_team",
                Polarity::Prefer,
                "small technical teams",
            ));
            assertions.push(read(
                TasteDimension::Seniority,
                "early_career",
                Polarity::Avoid,
                "early-career",
            ));
        }
        if words.contains("small established") {
            assertions.push(read(
                TasteDimension::Company,
                "startup",
                Polarity::Open,
                "startups",
            ));
            assertions.push(read(
                TasteDimension::Company,
                "small_company",
                Polarity::Open,
                "small established companies",
            ));
        }
        Ok(TasteReading {
            interpreter: self.name(),
            summary: Some("Senior work on small teams.".into()),
            assertions,
            ambiguities: vec!["Small team, or small company?".into()],
            constraints_noted: vec!["remote".into()],
            rejected: 0,
        })
    }
}

fn items(v: &TasteProfileView) -> Vec<&TasteItemView> {
    v.understood
        .iter()
        .chain(&v.avoid)
        .flat_map(|l| &l.items)
        .collect()
}

fn find<'a>(v: &'a TasteProfileView, key: &str) -> Option<&'a TasteItemView> {
    items(v)
        .into_iter()
        .find(|i| format!("{}:{}", i.dimension, i.value) == key)
}

#[tokio::test]
async fn one_sentence_becomes_a_concise_profile_with_the_rules() {
    let app = app(None).await;
    let r = app.describe_taste(SENIOR, now()).await.unwrap();
    assert!(r.interpreted);
    let v = &r.profile;
    assert_eq!(v.looking_for.as_deref(), Some(SENIOR));
    assert_eq!(v.reader.kind, "rules");
    for key in [
        "team:small_team",
        "ownership:high",
        "work_shape:backend",
        "work_shape:platform",
        "company:startup",
        "company:growth",
    ] {
        let item = find(v, key).unwrap_or_else(|| panic!("{key} missing: {v:#?}"));
        assert_eq!(item.polarity, "prefer");
        assert_eq!(item.basis, "Narrow's reading of your words");
    }
    for key in [
        "seniority:early_career",
        "culture:process_heavy",
        "company:large_company",
    ] {
        assert_eq!(find(v, key).unwrap().polarity, "avoid", "{key}");
    }
    // The level comes from the profile, and says so.
    let senior = find(v, "seniority:senior").expect("inferred from the latest title");
    assert_eq!(senior.origin, "profile");
    assert_eq!(senior.basis, "Inferred from your profile");
    assert!(senior.sources[0].text.contains("Senior Software Engineer"));
    assert!(v.needs_confirmation);
    assert!(
        v.understood.len() <= 8,
        "a concise summary: {} lines",
        v.understood.len()
    );
    // Remote is a practical constraint, read deterministically, never taste.
    assert!(!items(v).iter().any(|i| i.value.contains("remote")));
    assert!(
        v.constraints.iter().any(|c| c.kind == "work_setup"),
        "{:?}",
        v.constraints
    );
    assert!(
        v.constraints
            .iter()
            .any(|c| c.kind == "location" && c.text.contains("São Paulo"))
    );
    assert!(
        v.interpretation
            .as_ref()
            .unwrap()
            .constraints_noted
            .contains(&"remote".to_owned())
    );
}

#[tokio::test]
async fn a_model_reads_once_and_is_not_asked_again_for_the_same_input() {
    let fake = Fake::new(false);
    let app = app(Some(fake.clone())).await;
    let r = app.describe_taste(SENIOR, now()).await.unwrap();
    assert_eq!(fake.calls(), 1);
    assert_eq!(r.profile.reader.name, "model/fake:test");
    assert_eq!(
        find(&r.profile, "team:small_team")
            .unwrap()
            .interpreter
            .as_deref(),
        Some("model/fake:test")
    );
    // Reading the page, ranking Today, describing the same words again:
    // no model call.
    app.taste_profile().await.unwrap();
    app.taste_view().await.unwrap();
    let again = app.describe_taste(SENIOR, later(1)).await.unwrap();
    assert!(!again.interpreted);
    assert_eq!(fake.calls(), 1);
    // Only when asked.
    app.review_taste(&TasteAction::Reinterpret, later(2))
        .await
        .unwrap();
    assert_eq!(fake.calls(), 2);
    // What was sent: the words and a normalized career, nothing personal.
    let sent = fake.requests.lock().unwrap()[0].clone();
    assert!(sent.contains("small technical teams"));
    assert!(sent.contains("Senior Software Engineer"));
    for private in [
        "Ana",
        "ana@example.com",
        "Orbitly",
        "Ledgerline",
        "São Paulo",
    ] {
        assert!(!sent.contains(private), "{private} was sent: {sent}");
    }
}

#[tokio::test]
async fn a_failing_model_falls_back_to_the_rules_and_says_so() {
    let fake = Fake::new(true);
    let app = app(Some(fake.clone())).await;
    let r = app.describe_taste(SENIOR, now()).await.unwrap();
    assert_eq!(fake.calls(), 1);
    let interpretation = r.profile.interpretation.as_ref().unwrap();
    assert_eq!(interpretation.outcome, "fallback");
    assert_eq!(interpretation.interpreter, "rules/1");
    assert!(
        interpretation
            .note
            .as_ref()
            .unwrap()
            .contains("couldn't be used")
    );
    assert!(find(&r.profile, "team:small_team").is_some(), "still read");
    // The model is tried again next time, since the rules read it.
    app.review_taste(&TasteAction::Reinterpret, later(1))
        .await
        .unwrap();
    assert_eq!(fake.calls(), 2);
}

#[tokio::test]
async fn corrections_win_over_reinterpretation() {
    let fake = Fake::new(false);
    let app = app(Some(fake.clone())).await;
    let r = app
        .describe_taste(
            "I want early-stage startups and small technical teams.",
            now(),
        )
        .await
        .unwrap();
    let early = find(&r.profile, "company:early_stage").unwrap().id.clone();
    // "open to startups or small established companies"
    let r = app
        .review_taste(
            &TasteAction::Correct {
                id: early,
                text: Some("Open to startups or small established companies".into()),
                polarity: None,
            },
            later(1),
        )
        .await
        .unwrap();
    let startup = find(&r.profile, "company:startup").unwrap();
    assert_eq!(startup.polarity, "open");
    assert_eq!(startup.basis, "You corrected this");
    assert_eq!(startup.original.as_deref(), Some("Early-stage companies"));
    assert!(find(&r.profile, "company:early_stage").is_none());
    // Remove one, say another doesn't matter.
    let team = find(&r.profile, "team:small_team").unwrap().id.clone();
    let early_career = find(&r.profile, "seniority:early_career")
        .unwrap()
        .id
        .clone();
    app.review_taste(&TasteAction::Remove { id: team }, later(2))
        .await
        .unwrap();
    app.review_taste(&TasteAction::Neutral { id: early_career }, later(3))
        .await
        .unwrap();
    // Reinterpreting, and a resume re-import, change none of it.
    app.review_taste(&TasteAction::Reinterpret, later(4))
        .await
        .unwrap();
    app.import_resume(RESUME.as_bytes(), Some("resume.md".into()), later(5))
        .await
        .unwrap();
    let v = app.taste_profile().await.unwrap();
    assert!(find(&v, "company:early_stage").is_none());
    assert!(find(&v, "team:small_team").is_none());
    assert_eq!(v.removed.len(), 1);
    assert_eq!(v.neutral.len(), 1);
    assert_eq!(v.neutral[0].value, "early_career");
    assert_eq!(find(&v, "company:startup").unwrap().polarity, "open");
    let keys: Vec<String> = items(&v)
        .iter()
        .map(|i| format!("{}:{}", i.dimension, i.value))
        .collect();
    let mut unique = keys.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(keys.len(), unique.len(), "no duplicates: {keys:?}");
    // The settled statements were given to the model, so it doesn't
    // contradict them.
    let last = fake.requests.lock().unwrap().last().cloned().unwrap();
    assert!(last.contains("Already settled by the person"), "{last}");
}

#[tokio::test]
async fn confirming_makes_the_summary_the_persons() {
    let app = app(None).await;
    app.describe_taste(SENIOR, now()).await.unwrap();
    let r = app
        .review_taste(&TasteAction::Confirm { ids: Vec::new() }, later(1))
        .await
        .unwrap();
    assert!(!r.profile.needs_confirmation);
    assert!(r.profile.confirmed_at.is_some());
    assert!(
        items(&r.profile)
            .iter()
            .all(|i| i.basis == "You confirmed" || i.basis == "You said")
    );
    // A new description needs a new confirmation.
    let r = app
        .describe_taste("Staff-plus platform roles at growth companies.", later(2))
        .await
        .unwrap();
    assert!(r.profile.confirmed_at.is_none());
}

#[tokio::test]
async fn existing_preferences_migrate_without_changing_them() {
    let app = app(None).await;
    let set = |value, stance| jobhunt_app::preferences::PreferenceInput::Company {
        company: value,
        stance,
    };
    app.update_preferences(
        &PreferenceUpdate {
            statement: Some("remote only, at least USD 120k".into()),
            set: vec![
                set("small-team".into(), StanceInput::Want),
                set("small-company".into(), StanceInput::Want),
                set("early-stage".into(), StanceInput::Want),
            ],
            remove: Vec::new(),
        },
        now(),
    )
    .await
    .unwrap();
    let before = app.profiles().require().await.unwrap().preferences;
    let v = app.taste_profile().await.unwrap();
    for key in [
        "team:small_team",
        "company:small_company",
        "company:early_stage",
    ] {
        let item = find(&v, key).unwrap_or_else(|| panic!("{key}: {v:#?}"));
        assert_eq!(item.origin, "legacy");
        assert_eq!(item.basis, "From your earlier settings");
    }
    assert!(!items(&v).iter().any(|i| i.dimension == "compensation"));
    let kinds: Vec<&str> = v.constraints.iter().map(|c| c.kind.as_str()).collect();
    assert!(
        kinds.contains(&"work_setup") && kinds.contains(&"pay_floor"),
        "{kinds:?}"
    );
    // The earlier words are the starting point, verbatim.
    assert_eq!(v.looking_for_source.as_deref(), Some("statements"));
    assert_eq!(
        v.looking_for.as_deref(),
        Some("remote only, at least USD 120k")
    );
    // Reading the profile wrote nothing.
    let after = app.profiles().require().await.unwrap();
    assert_eq!(after.preferences, before);
    assert!(after.taste.is_empty() && after.taste_brief.is_none());
    // Removing a migrated statement removes the setting ranking reads.
    let early = find(&v, "company:early_stage").unwrap().id.clone();
    app.review_taste(&TasteAction::Remove { id: early }, later(1))
        .await
        .unwrap();
    let data = app.profiles().require().await.unwrap();
    assert!(!data.preferences.iter().any(|p| p.active
        && p.value
            == PreferenceValue::Company {
                company: CompanyTrait::EarlyStage
            }));
    // Changing one's polarity changes the setting's stance.
    let v = app.taste_profile().await.unwrap();
    let small = find(&v, "company:small_company").unwrap().id.clone();
    app.review_taste(
        &TasteAction::Correct {
            id: small,
            text: None,
            polarity: Some(PolarityInput::Avoid),
        },
        later(2),
    )
    .await
    .unwrap();
    let data = app.profiles().require().await.unwrap();
    let small = data
        .preferences
        .iter()
        .find(|p| {
            p.active
                && p.value
                    == PreferenceValue::Company {
                        company: CompanyTrait::SmallCompany,
                    }
        })
        .unwrap();
    assert_eq!(small.stance, Stance::Unwanted);
    // Interpreting the earlier words makes them the description.
    let r = app
        .review_taste(&TasteAction::Reinterpret, later(3))
        .await
        .unwrap();
    assert_eq!(r.profile.looking_for_source.as_deref(), Some("description"));
}

#[tokio::test]
async fn a_new_description_replaces_what_the_old_one_set() {
    let app = app(None).await;
    app.describe_taste("Startups, remote only.", now())
        .await
        .unwrap();
    app.describe_taste("Large companies, hybrid is fine.", later(1))
        .await
        .unwrap();
    let data = app.profiles().require().await.unwrap();
    assert!(
        !data.preferences.iter().any(|p| p.active
            && p.value
                == PreferenceValue::Company {
                    company: CompanyTrait::Startup
                }),
        "the old description's settings went with it"
    );
    let v = app.taste_profile().await.unwrap();
    assert!(find(&v, "company:startup").is_none());
    assert!(find(&v, "company:large_company").is_some());
}

#[tokio::test]
async fn added_sentences_and_invalid_input() {
    let app = app(None).await;
    app.describe_taste(SENIOR, now()).await.unwrap();
    let r = app
        .review_taste(
            &TasteAction::Add {
                text: "I'd love developer tooling.".into(),
            },
            later(1),
        )
        .await
        .unwrap();
    let tooling = find(&r.profile, "work_shape:developer_tooling").unwrap();
    assert_eq!(tooling.basis, "You said");
    assert!(app.describe_taste("   ", now()).await.is_err());
    assert!(app.describe_taste(&"x".repeat(2001), now()).await.is_err());
    assert!(
        app.review_taste(
            &TasteAction::Remove {
                id: "taste_nope".into()
            },
            now()
        )
        .await
        .is_err()
    );
}
