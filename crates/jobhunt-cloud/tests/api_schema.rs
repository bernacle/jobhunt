//! The HTTP API's JSON Schema, from the same Rust types the server answers
//! with (the application's views, the MCP tools' arguments, the API's own
//! request and response bodies). The web app generates its TypeScript
//! types from this file (`apps/web`: `npm run types`), so the browser can't
//! drift from the server.
//!
//! This test fails when the committed schema is out of date. Regenerate
//! it with:
//!
//! ```text
//! JOBHUNT_UPDATE_SCHEMA=1 cargo test -p jobhunt-cloud --test api_schema
//! ```

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

macro_rules! types {
    ($generator:ident, $($ty:ty),+ $(,)?) => {{
        let mut roots = Vec::new();
        $(
            $generator.subschema_for::<$ty>();
            roots.push(<$ty as schemars::JsonSchema>::schema_name().into_owned());
        )+
        roots
    }};
}

fn schema() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let roots = types!(
        generator,
        // Answers.
        jobhunt_app::feed::FeedView,
        jobhunt_app::SearchResults,
        jobhunt_app::inspect::JobDetail,
        jobhunt_app::inspect::VerificationReport,
        jobhunt_app::feedback::FeedbackResult,
        jobhunt_app::feedback::PipelineView,
        jobhunt_app::profile_view::ProfileView,
        jobhunt_app::preferences::PreferenceUpdateResult,
        jobhunt_app::taste_view::TasteView,
        jobhunt_app::taste_profile::TasteProfileView,
        jobhunt_app::taste_profile::TasteUpdateResult,
        jobhunt_app::profile_edit::ClaimReview,
        jobhunt_app::profile_edit::ClaimDecisionResult,
        jobhunt_app::profile_edit::ResumeImportResult,
        jobhunt_app::profile_sources::SourceImportResult,
        jobhunt_app::profile_sources::SourceRemovalResult,
        jobhunt_app::context::ApplicationContext,
        jobhunt_cloud::api::types::AccountView,
        jobhunt_cloud::api::types::AuthConfigView,
        jobhunt_cloud::api::types::DevTokenResponse,
        jobhunt_cloud::api::types::TokenList,
        jobhunt_cloud::api::types::CreatedToken,
        jobhunt_cloud::api::types::NotificationSettingsView,
        jobhunt_cloud::api::types::ErrorBody,
        // Requests.
        jobhunt_cloud::api::types::FeedbackRequest,
        jobhunt_cloud::api::types::DecideClaimsRequest,
        jobhunt_cloud::api::types::GithubImportRequest,
        jobhunt_cloud::api::types::UpdateNotificationsRequest,
        jobhunt_cloud::api::types::ConfirmEmailRequest,
        jobhunt_cloud::api::types::CreateTokenRequest,
        jobhunt_mcp::UpdatePreferencesParams,
        jobhunt_app::taste_profile::TasteAction,
        jobhunt_mcp::SearchJobsParams,
    );
    let definitions = generator.take_definitions(true);
    let properties: Map<String, Value> = roots
        .iter()
        .map(|name| (name.clone(), json!({ "$ref": format!("#/$defs/{name}") })))
        .collect();
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "JobHuntApi",
        "description": "Every body the JobHunt HTTP API (/api/v1) accepts or answers with. Generated from the Rust types by crates/jobhunt-cloud/tests/api_schema.rs; do not edit.",
        "type": "object",
        "properties": properties,
        "$defs": definitions,
    })
}

#[test]
fn the_web_apps_schema_is_current() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/src/lib/api-schema.json");
    let generated = format!("{}\n", serde_json::to_string_pretty(&schema()).unwrap());
    if std::env::var_os("JOBHUNT_UPDATE_SCHEMA").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &generated).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == generated,
        "apps/web/src/lib/api-schema.json is out of date: run \
         JOBHUNT_UPDATE_SCHEMA=1 cargo test -p jobhunt-cloud --test api_schema, then \
         `npm run types` in apps/web"
    );
}
