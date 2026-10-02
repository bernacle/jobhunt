//! Eligibility probe (BRU-325's eligibility patch; see
//! `docs/eligibility-hiring-scope.md`). Not part of the product.
//!
//! Prints every open job's eligibility decision for the configured person,
//! one JSON object per line: status, headline, the chosen option and the
//! geography reasons. Run it on a copy of a frozen corpus before and after
//! a rule change and diff the two by `id`.
//!
//! ```text
//! cargo run --release -p jobhunt-cli --example eligibility_probe -- \
//!     <config.toml> <jobhunt.db> > decisions.jsonl
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::path::Path;

use jobhunt_app::LocalApp;
use jobhunt_jobs::{JobQuery, JobStatus};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let loaded =
        jobhunt_app::config::load(Some(Path::new(&args[1])), Some(Path::new(&args[2])), None)?;
    let app = LocalApp::open(loaded).await?;
    let facts = app.profile_facts().await?.expect("profile");
    let query = JobQuery {
        status: Some(JobStatus::Open),
        ..JobQuery::default()
    };
    let records = app.store().search(&query).await?;
    for r in records {
        let d = jobhunt_eligibility::evaluate_record(&r, &facts);
        let p = &r.posting;
        println!(
            "{}",
            json!({
                "id": r.id.to_string(),
                "company": p.company,
                "title": p.title,
                "location": p.location,
                "locations": p.locations.iter().map(|l| l.name.clone()).collect::<Vec<_>>(),
                "workplace": p.workplace_type.as_ref().map(|w| w.as_str()),
                "status": d.status.as_str(),
                "headline": d.headline,
                "option": d.option,
                "geo": d.reasons.iter()
                    .filter(|x| matches!(
                        x.rule.as_str(),
                        "country_constraint" | "region_constraint" | "remote_scope" | "ambiguity"
                    ))
                    .map(|x| format!("{:?} {}: {}", x.verdict, x.rule.as_str(), x.conclusion))
                    .collect::<Vec<_>>(),
            })
        );
    }
    app.close().await;
    Ok(())
}
