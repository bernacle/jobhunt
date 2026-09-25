//! The JobHunt MCP server: the local product's use cases, exposed as
//! Model Context Protocol tools so an assistant (Claude, ChatGPT, Codex,
//! or any MCP client) can work with the same profile, jobs, rankings and
//! feedback as the `jobhunt` command.
//!
//! `jobhunt mcp` runs it over stdio: newline-delimited JSON-RPC on stdin
//! and stdout, as the MCP specification defines, using the official Rust
//! SDK (`rmcp`). Stdout carries protocol messages only; logs go to stderr.
//!
//! Every tool is a thin adapter: arguments in, one [`LocalApp`] use case,
//! the use case's typed view out as structured content (with an output
//! schema). No ranking, verification, eligibility, preference or evidence
//! rule is implemented here.
//!
//! Tools that change the person's state (`update_preferences`, `save_job`,
//! `reject_job`, `mark_applied`, `record_feedback`) are marked
//! `readOnlyHint: false`, and are safe to retry: a repeat of an action
//! already in effect is not recorded again. Reading tools are marked
//! `readOnlyHint: true`. `search_jobs` and `verify_job` may read job
//! boards on the internet (`openWorldHint: true`) and refresh the local job
//! cache, but never change the person's state.
//!
//! Failures a client can act on (an unknown or ambiguous id, no profile
//! yet, an invalid preference, sources unreachable, …) come back as tool
//! results with `isError: true` and a JSON body
//! `{"error": {"code", "message", "hint"}}`; malformed arguments are
//! JSON-RPC `invalid params` errors. Storage failures are reported without
//! local paths or internal detail (that goes to the server's stderr log).

use std::sync::Arc;

use jobhunt_app::context::ApplicationContext;
use jobhunt_app::feedback::{FeedbackResult, PipelineView};
use jobhunt_app::inspect::{JobDetail, VerificationReport};
use jobhunt_app::preferences::{PreferenceInput, PreferenceUpdate, PreferenceUpdateResult};
use jobhunt_app::profile_view::ProfileView;
use jobhunt_app::shortlist::MAX_LIMIT;
use jobhunt_app::{
    AppError, FindRequest, LocalApp, Progress, ProgressEvent, RefreshMode, SearchResults, now,
};
use jobhunt_jobs::verification::VerifyMode;
use jobhunt_ranking::FeedbackAction;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::tool::IntoCallToolResult;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::model::{
    CallToolResponse, CallToolResult, ContentBlock, Implementation, ServerCapabilities,
    ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::Deserialize;

/// What the server tells clients about itself at initialization.
pub const INSTRUCTIONS: &str = "JobHunt is the person's local job-search assistant: it discovers \
jobs from company job boards, verifies them at the employer's own sources, checks eligibility \
against the person's profile, and ranks them by what the person wants and has told it through \
feedback. Everything is stored locally; these tools work on the same state as the `jobhunt` \
command.\n\
Typical flow: search_jobs for the short list worth the person's time; get_job or verify_job to \
look closer; save_job, reject_job (with the person's reason, verbatim) or mark_applied to record \
what they decide; update_preferences when they say what they want. Later searches reflect all \
of it. prepare_application_context gathers the evidence the person has approved for an \
application: use only the facts it returns, as written.\n\
Ids: opportunity ids look like opp_<32 hex>; job ids (job_…) and unique prefixes are accepted \
too. Tiers are coarse on purpose (strong_fit, worth_reviewing, maybe, low_priority); there is no \
match percentage. Record feedback only when the person asked for it.";

/// A tool failure a client can act on.
#[derive(Debug)]
pub struct ToolError {
    code: &'static str,
    message: String,
    hint: Option<&'static str>,
}

impl From<AppError> for ToolError {
    fn from(error: AppError) -> Self {
        if error.kind() == jobhunt_app::ErrorKind::Storage
            || error.kind() == jobhunt_app::ErrorKind::Config
        {
            // The full cause stays on this machine.
            tracing::error!(error = %jobhunt_app_error_chain(&error), "tool failed");
        } else {
            tracing::debug!(%error, "tool failed");
        }
        Self {
            code: error.kind().as_str(),
            message: error.public_message(),
            hint: error.hint(),
        }
    }
}

fn jobhunt_app_error_chain(error: &AppError) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

impl ToolError {
    fn cancelled() -> Self {
        Self {
            code: "cancelled",
            message: "The request was cancelled.".into(),
            hint: None,
        }
    }
}

impl IntoCallToolResult for ToolError {
    fn into_call_tool_result(self) -> Result<CallToolResponse, ErrorData> {
        let body = serde_json::json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "hint": self.hint,
            }
        });
        Ok(CallToolResult::error(vec![ContentBlock::text(body.to_string())]).into())
    }
}

type ToolResult<T> = Result<Json<T>, ToolError>;

/// Whether to read job boards before answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefreshInput {
    /// Refresh only when the stored jobs are older than the configured
    /// freshness window (the default; usually a few hours).
    #[default]
    Auto,
    /// Read every configured job board now (slower: many requests).
    Always,
    /// Work only from stored jobs: no network at all, including
    /// verification.
    Never,
}

/// Arguments of `search_jobs`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchJobsParams {
    /// Words every job must contain (in its title, company, location,
    /// department or team), e.g. "rust backend". Omit to search everything.
    #[serde(default)]
    pub query: Option<String>,
    /// How many opportunities to return, 1 to 25. Default 5: the short
    /// list is meant to be short.
    #[serde(default)]
    #[schemars(range(min = 1, max = 25))]
    pub limit: Option<u8>,
    /// Whether to read job boards first. Default "auto".
    #[serde(default)]
    pub refresh: RefreshInput,
    /// Verify the best candidates at their employers' sources when their
    /// last verification isn't recent (default true; ignored with refresh
    /// "never").
    #[serde(default)]
    pub verify: Option<bool>,
    /// Also return opportunities ranked maybe or low priority (default
    /// false).
    #[serde(default)]
    pub include_lower_tiers: bool,
}

/// Arguments of tools about one opportunity.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetJobParams {
    /// The opportunity id from search results (opp_…); a job id (job_…) or
    /// a unique prefix of either also works.
    pub id: String,
    /// Include every source record listing it, with per-source
    /// verification (default false).
    #[serde(default)]
    pub include_sources: bool,
    /// Include the whole description instead of its first part (default
    /// false).
    #[serde(default)]
    pub full_description: bool,
}

/// Arguments of `verify_job`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VerifyJobParams {
    /// The opportunity id (opp_…), a job id (job_…), or a unique prefix.
    pub id: String,
    /// Ask the sources even if they were asked in the last few minutes
    /// (default false: a recent attempt is reused).
    #[serde(default)]
    pub force: bool,
}

/// Arguments of `save_job`, `reject_job` and `mark_applied`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeedbackParams {
    /// The opportunity id (opp_…), a job id (job_…), or a unique prefix.
    pub id: String,
    /// Why, in the person's own words ("too corporate", "tiny team and
    /// strong ownership"). Stored verbatim and learned from. Pass the
    /// person's words; don't invent a reason.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Other feedback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OtherFeedback {
    /// The person likes it (independent of saving or applying).
    Like,
    /// The person dislikes it (independent of rejecting it).
    Dislike,
    /// Take it off the saved list without rejecting it.
    Unsave,
    /// The person is interviewing for it.
    Interview,
    /// The person got an offer.
    Offer,
}

/// Arguments of `record_feedback`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordFeedbackParams {
    /// The opportunity id (opp_…), a job id (job_…), or a unique prefix.
    pub id: String,
    pub action: OtherFeedback,
    /// Why, in the person's own words. Stored verbatim.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Arguments of `update_preferences`. Give at least one of `statement`,
/// `set` or `remove`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdatePreferencesParams {
    /// What the person wants, in their own words, e.g. "I want small
    /// product teams, backend or platform work, remote from Brazil, at
    /// least USD 120k". Stored verbatim; what JobHunt understands from it
    /// is returned, including the parts it didn't understand.
    #[serde(default)]
    pub statement: Option<String>,
    /// Precise preferences to set.
    #[serde(default)]
    pub set: Vec<PreferenceInput>,
    /// Preference (pref_…) or statement (stmt_…) ids to remove.
    #[serde(default)]
    pub remove: Vec<String>,
}

/// Arguments of `get_pipeline`.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PipelineParams {
    /// Also list rejected opportunities (default false).
    #[serde(default)]
    pub include_rejected: bool,
}

/// Arguments of `prepare_application_context`.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextParams {
    /// The opportunity id (opp_…), a job id (job_…), or a unique prefix.
    pub id: String,
    /// Include the person's name and contact details (email, phone,
    /// links). Default false: only ask for them when the person wants
    /// help filling in an application form.
    #[serde(default)]
    pub include_contact_details: bool,
}

/// Logs progress to stderr (never stdout).
struct LogProgress;

impl Progress for LogProgress {
    fn note(&self, event: ProgressEvent) {
        match event {
            ProgressEvent::Refreshing { sources, reason } => {
                tracing::info!(sources, reason = ?reason, "refreshing job sources");
            }
            ProgressEvent::Verifying { records, sources } => {
                tracing::info!(records, sources, "verifying listings");
            }
        }
    }
}

/// Runs a long use case (refreshing, verifying) until it finishes or the
/// client cancels the request. Dropping the use case on cancellation is
/// safe: every write it makes is its own transaction (one source's scan,
/// one verification attempt).
///
/// The use case runs on a blocking thread driven by the current runtime:
/// discovery and verification drive stream combinators over borrowed data,
/// whose futures the compiler cannot prove `Send` in a generic context
/// (rust-lang/rust#102211), which a tool handler requires.
async fn long_running<T, F, Fut>(
    context: &RequestContext<RoleServer>,
    app: &Arc<LocalApp>,
    work: F,
) -> Result<T, ToolError>
where
    T: Send + 'static,
    F: FnOnce(Arc<LocalApp>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    let cancelled = context.ct.clone();
    let app = Arc::clone(app);
    let handle = tokio::runtime::Handle::current();
    let task = tokio::task::spawn_blocking(move || {
        handle.block_on(async move {
            tokio::select! {
                result = work(app) => Some(result),
                () = cancelled.cancelled() => None,
            }
        })
    });
    match task.await {
        Ok(Some(result)) => result.map_err(ToolError::from),
        Ok(None) => Err(ToolError::cancelled()),
        Err(error) => {
            tracing::error!(%error, "a tool task failed");
            Err(ToolError {
                code: "internal_error",
                message: "The request failed unexpectedly; try again.".into(),
                hint: None,
            })
        }
    }
}

/// The server: one [`LocalApp`], shared by concurrent requests.
#[derive(Clone)]
pub struct JobHuntServer {
    app: Arc<LocalApp>,
    tool_router: ToolRouter<Self>,
}

impl std::fmt::Debug for JobHuntServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobHuntServer")
            .field("app", &self.app)
            .finish_non_exhaustive()
    }
}

#[tool_router]
impl JobHuntServer {
    pub fn new(app: Arc<LocalApp>) -> Self {
        Self {
            app,
            tool_router: Self::tool_router(),
        }
    }

    async fn feedback(
        &self,
        id: &str,
        action: FeedbackAction,
        reason: Option<&str>,
    ) -> ToolResult<FeedbackResult> {
        tracing::info!(id, action = action.as_str(), "recording feedback");
        let opportunity = self.app.resolve(id).await?;
        let outcome = self
            .app
            .record_feedback(&opportunity, action, reason, now())
            .await?;
        Ok(Json(FeedbackResult::of(&outcome)))
    }

    #[tool(
        name = "search_jobs",
        description = "Find the few opportunities currently worth the person's time: \
        personalized to their profile, preferences and feedback, gated on eligibility and \
        verification. Returns a small ranked list (default 5) with a coarse fit tier, \
        verification and eligibility state, why each may be worth it and what to consider, \
        plus the funnel from every open job to the list. Refreshes job boards only when stored \
        jobs are stale (refresh: auto) and verifies the best candidates. Needs a profile.",
        annotations(
            title = "Search opportunities",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn search_jobs(
        &self,
        Parameters(p): Parameters<SearchJobsParams>,
        context: RequestContext<RoleServer>,
    ) -> ToolResult<SearchResults> {
        let limit = usize::from(p.limit.unwrap_or(5));
        if limit == 0 || limit > MAX_LIMIT {
            return Err(AppError::InvalidArguments(format!(
                "limit must be between 1 and {MAX_LIMIT}"
            ))
            .into());
        }
        let request = FindRequest {
            text: p.query.unwrap_or_default(),
            limit,
            all_tiers: p.include_lower_tiers,
            refresh: match p.refresh {
                RefreshInput::Auto => RefreshMode::Auto,
                RefreshInput::Always => RefreshMode::Always,
                RefreshInput::Never => RefreshMode::Never,
            },
            verify: p.refresh != RefreshInput::Never && p.verify.unwrap_or(true),
        };
        tracing::info!(query = %request.text, limit, "search_jobs");
        let at = now();
        let results = long_running(&context, &self.app, move |app| async move {
            let found = app.find(&request, &LogProgress, at).await?;
            Ok(SearchResults::of(&found, at))
        })
        .await?;
        Ok(Json(results))
    }

    #[tool(
        name = "get_job",
        description = "Details of one opportunity: title, company, locations, pay (verified \
        facts when available), a description summary, verification and eligibility with \
        reasons, the decision brief (tier, why, caveats, unknowns) and the person's pipeline \
        state. Set include_sources for every source record and its provenance. Reads what is \
        stored; use verify_job to check the listing now.",
        annotations(
            title = "Get an opportunity",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_job(&self, Parameters(p): Parameters<GetJobParams>) -> ToolResult<JobDetail> {
        let opportunity = self.app.resolve(&p.id).await?;
        let inspection = self.app.inspect(&opportunity, false, now()).await?;
        Ok(Json(JobDetail::of(
            &inspection,
            p.include_sources,
            p.full_description,
        )))
    }

    #[tool(
        name = "verify_job",
        description = "Ask an opportunity's authoritative sources whether it is still open and \
        can be applied to, record what they say, and check it against the person's profile. \
        Returns the listing and application state, who publishes it, the last attempt and last \
        success, compensation facts, the eligibility decision and what remains uncertain. A \
        verification from the last few minutes is reused unless force is true.",
        annotations(
            title = "Verify an opportunity",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = true
        )
    )]
    async fn verify_job(
        &self,
        Parameters(p): Parameters<VerifyJobParams>,
        context: RequestContext<RoleServer>,
    ) -> ToolResult<VerificationReport> {
        let opportunity = self.app.resolve(&p.id).await?;
        let mode = if p.force {
            VerifyMode::Force
        } else {
            VerifyMode::IfDue
        };
        let report = long_running(&context, &self.app, move |app| async move {
            let checked = app.verify(&opportunity, mode, &LogProgress, now()).await?;
            Ok(VerificationReport::of(&opportunity, &checked))
        })
        .await?;
        Ok(Json(report))
    }

    #[tool(
        name = "get_profile",
        description = "The person's professional profile: headline, location, experiences, \
        technologies with the evidence behind them, domains, role and ownership signals, \
        preferences (and their own words), claims awaiting their review, and gaps. Never \
        includes their name or contact details.",
        annotations(
            title = "Get the profile",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_profile(&self) -> ToolResult<ProfileView> {
        Ok(Json(self.app.profile_view().await?))
    }

    #[tool(
        name = "update_preferences",
        description = "Record what the person wants. Give their words as `statement` (stored \
        verbatim and read into structured preferences), precise values in `set`, and/or ids to \
        `remove`. Returns what was understood, what JobHunt is unsure about, the parts it did \
        not understand (never silently dropped), what the new preferences replaced, and every \
        preference now in effect. Repeating an update already in effect changes nothing.",
        annotations(
            title = "Update preferences",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn update_preferences(
        &self,
        Parameters(p): Parameters<UpdatePreferencesParams>,
    ) -> ToolResult<PreferenceUpdateResult> {
        let update = PreferenceUpdate {
            statement: p.statement,
            set: p.set,
            remove: p.remove,
        };
        let changes = self.app.update_preferences(&update, now()).await?;
        Ok(Json(PreferenceUpdateResult::of(&changes)))
    }

    #[tool(
        name = "save_job",
        description = "Save an opportunity for later (for every source listing it). Optional \
        reason in the person's words. Saving a rejected opportunity brings it back. Returns the \
        resulting pipeline state; saving an already saved opportunity changes nothing.",
        annotations(
            title = "Save an opportunity",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn save_job(
        &self,
        Parameters(p): Parameters<FeedbackParams>,
    ) -> ToolResult<FeedbackResult> {
        self.feedback(&p.id, FeedbackAction::Save, p.reason.as_deref())
            .await
    }

    #[tool(
        name = "reject_job",
        description = "The person is not interested in an opportunity. Pass their reason \
        verbatim ('too corporate', 'pure SRE'): it is stored as written and read into what \
        they want, and later searches stop recommending it and learn from it. Returns the new \
        state, how the reason was interpreted, and whether learned taste changed. The same \
        rejection with the same reason is not recorded twice.",
        annotations(
            title = "Reject an opportunity",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn reject_job(
        &self,
        Parameters(p): Parameters<FeedbackParams>,
    ) -> ToolResult<FeedbackResult> {
        self.feedback(&p.id, FeedbackAction::Reject, p.reason.as_deref())
            .await
    }

    #[tool(
        name = "mark_applied",
        description = "Record that the person applied to an opportunity (only when they say \
        so). It leaves the recommendations and joins their pipeline. No other application \
        details are stored. Marking it again changes nothing.",
        annotations(
            title = "Mark applied",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn mark_applied(
        &self,
        Parameters(p): Parameters<FeedbackParams>,
    ) -> ToolResult<FeedbackResult> {
        self.feedback(&p.id, FeedbackAction::Applied, p.reason.as_deref())
            .await
    }

    #[tool(
        name = "record_feedback",
        description = "Other feedback on an opportunity: like or dislike it (independent of \
        saving or rejecting), take it off the saved list (unsave), or record an interview or an \
        offer. Optional reason in the person's words, stored verbatim.",
        annotations(
            title = "Record feedback",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn record_feedback(
        &self,
        Parameters(p): Parameters<RecordFeedbackParams>,
    ) -> ToolResult<FeedbackResult> {
        let action = match p.action {
            OtherFeedback::Like => FeedbackAction::Like,
            OtherFeedback::Dislike => FeedbackAction::Dislike,
            OtherFeedback::Unsave => FeedbackAction::Unsave,
            OtherFeedback::Interview => FeedbackAction::Interview,
            OtherFeedback::Offer => FeedbackAction::Offer,
        };
        self.feedback(&p.id, action, p.reason.as_deref()).await
    }

    #[tool(
        name = "get_pipeline",
        description = "Opportunities the person saved, applied to, is interviewing for or got \
        an offer from (and rejected ones with include_rejected), with their latest reason.",
        annotations(
            title = "Get the pipeline",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn get_pipeline(
        &self,
        Parameters(p): Parameters<PipelineParams>,
    ) -> ToolResult<PipelineView> {
        let entries = self.app.pipeline(p.include_rejected).await?;
        Ok(Json(PipelineView::of(&entries)))
    }

    #[tool(
        name = "prepare_application_context",
        description = "Evidence for helping the person apply to one opportunity: the job and \
        its decision brief, what it asks for, the person's relevant experience and projects, \
        and technologies with evidence. Only facts the person confirmed or entered, or that are \
        quoted directly from their current resume, are included, each with its source snippet; \
        inferred, outdated and rejected claims are withheld and only counted. Gaps are listed \
        as missing evidence. It writes nothing: use the facts as given, without adding \
        metrics or responsibilities. Contact details only with include_contact_details.",
        annotations(
            title = "Prepare application context",
            read_only_hint = true,
            open_world_hint = false
        )
    )]
    async fn prepare_application_context(
        &self,
        Parameters(p): Parameters<ContextParams>,
    ) -> ToolResult<ApplicationContext> {
        let opportunity = self.app.resolve(&p.id).await?;
        let context = self
            .app
            .application_context(&opportunity, p.include_contact_details, now())
            .await?;
        Ok(Json(context))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for JobHuntServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(
                Implementation::new("jobhunt", env!("CARGO_PKG_VERSION"))
                    .with_title("JobHunt")
                    .with_description(
                        "High-signal job discovery: your profile, verified opportunities, \
                         rankings and feedback, from your local JobHunt database.",
                    ),
            )
            .with_instructions(INSTRUCTIONS)
    }
}

/// The server could not start or stopped abnormally.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("the MCP session could not start: {0}")]
    Start(String),
    #[error("the MCP session ended abnormally: {0}")]
    Join(String),
}

/// Serves MCP on stdin/stdout until the client disconnects (stdin closes),
/// then closes the database.
pub async fn serve_stdio(app: LocalApp) -> Result<(), ServeError> {
    let app = Arc::new(app);
    let server = JobHuntServer::new(Arc::clone(&app));
    tracing::info!("MCP server ready on stdio");
    let running = server
        .serve(rmcp::transport::stdio())
        .await
        .map_err(|e| ServeError::Start(e.to_string()))?;
    let reason = running
        .waiting()
        .await
        .map_err(|e| ServeError::Join(e.to_string()))?;
    tracing::info!(?reason, "MCP session ended");
    if let Ok(app) = Arc::try_unwrap(app) {
        app.close().await;
    }
    Ok(())
}
