//! Hosted MCP: the local MCP server's tools over Streamable HTTP.
//!
//! The tools are `jobhunt-mcp`'s, unchanged (same names, schemas,
//! structured output and error codes). What differs is where each call's
//! application comes from: [`HostedApps`] builds it for the account the
//! request authenticated as (the API's middleware puts the [`Principal`]
//! in the request; rmcp hands the HTTP request parts to the tool). A call
//! without one fails with `unauthenticated` before any tool code runs.
//!
//! The transport is stateless (no MCP sessions kept in memory; the
//! `2026-07-28` protocol has none, and older clients are served the same
//! way), so any replica can answer any request.

use std::sync::Arc;

use jobhunt_app::App;
use jobhunt_mcp::{AppProvider, INSTRUCTIONS, JobHuntServer, ToolError};
use rmcp::RoleServer;
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

use crate::api::ApiState;
use crate::auth::Principal;

/// What hosted clients are told at initialization: the local instructions,
/// under the public name (Narrow) and without the claim of local storage.
pub fn hosted_instructions() -> String {
    INSTRUCTIONS
        .replace(
            "JobHunt is the person's local job-search assistant",
            "Narrow is the person's job-search assistant",
        )
        .replace(
            "Everything is stored locally; these tools work on the same state as the `jobhunt` \
command.",
            "This is the person's Narrow account, synced with their `jobhunt` command. Job \
boards are read in the background, so searches never wait for them.",
        )
        .replace("what JobHunt believes", "what Narrow believes")
}

/// One application per authenticated request.
pub struct HostedApps {
    state: ApiState,
    instructions: String,
}

impl AppProvider for HostedApps {
    fn app(&self, context: &RequestContext<RoleServer>) -> Result<Arc<App>, ToolError> {
        let principal = context
            .extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Principal>());
        match principal {
            Some(p) => Ok(self.state.app_for(p)),
            None => Err(ToolError::new(
                "unauthenticated",
                "This request is not signed in.",
                Some("Connect with an access token (OAuth or a Narrow personal access token)."),
            )),
        }
    }

    fn on_tool(&self, context: &RequestContext<RoleServer>, tool: &'static str) {
        let user = context
            .extensions
            .get::<http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<Principal>())
            .map(|p| p.user.clone());
        self.state.usage().record(
            user.as_ref(),
            "mcp_tool",
            serde_json::json!({ "tool": tool }),
        );
    }

    fn instructions(&self) -> &str {
        &self.instructions
    }

    fn description(&self) -> &str {
        "High-signal job discovery: your profile, verified opportunities, rankings and \
         feedback, from your Narrow account."
    }
}

/// The `/mcp` service.
pub fn service(state: ApiState) -> StreamableHttpService<JobHuntServer, NeverSessionManager> {
    let mut hosts: Vec<String> = vec!["localhost".into(), "127.0.0.1".into(), "::1".into()];
    if let Some(host) = state
        .config()
        .public_url
        .as_ref()
        .and_then(|u| u.host_str())
    {
        hosts.push(host.to_owned());
    }
    let mut config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_sse_keep_alive(None);
    config = if state.config().public_url.is_some() {
        config.with_allowed_hosts(hosts)
    } else {
        // No public URL configured (development): the bearer token is the
        // protection; don't reject unknown Host headers.
        config.disable_allowed_hosts()
    };
    if !state.config().allowed_origins.is_empty() {
        config = config.with_allowed_origins(state.config().allowed_origins.clone());
    }
    let provider: Arc<dyn AppProvider> = Arc::new(HostedApps {
        state,
        instructions: hosted_instructions(),
    });
    StreamableHttpService::new(
        move || Ok(JobHuntServer::with_provider(Arc::clone(&provider))),
        Arc::new(NeverSessionManager::default()),
        config,
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn hosted_instructions_do_not_claim_local_storage() {
        let text = super::hosted_instructions();
        assert!(text.starts_with("Narrow is the person's job-search assistant"));
        assert!(text.contains("the person's Narrow account"));
        assert!(!text.contains("Everything is stored locally"));
        assert!(
            !text.contains("JobHunt"),
            "the public name is Narrow: {text}"
        );
    }
}
