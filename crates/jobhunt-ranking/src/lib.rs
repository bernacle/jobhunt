//! Ranking: which of the jobs a person *can* take they would likely
//! *want*, and why.
//!
//! Eligibility ([`jobhunt_eligibility`]) answers "could this person work
//! this job?". This crate answers "would they want it?", on top of that
//! answer and never inside it: pay, roles, seniority, domains and company
//! kinds are read here, not by the eligibility rules.
//!
//! * [`feedback`]: durable feedback on opportunities (save, reject,
//!   applied, interview, offer, like, dislike), with the reason in the
//!   person's own words, and the per-opportunity state folded from it
//!   (a pipeline stage, and a separate like/dislike).
//! * [`reason`]: deterministic reading of those reasons into structured
//!   signals ([`RuleReader`]), behind the [`ReasonReader`] seam; the text
//!   is always kept verbatim.
//! * [`facets`](mod@facets)(mod@facets): what a job is, read from its posting with the words
//!   behind every fact (role shape, level, required vs mentioned
//!   technologies, domains, company kind, work style).
//! * [`person`]: what the person stated they want, and what their resume
//!   shows they have done, kept apart.
//! * [`taste`]: patterns learned from feedback, each with its evidence,
//!   confidence and contradictions; explicit preferences always win.
//! * [`signals`], [`rank`](mod@rank)(mod@rank) and [`brief`]: deterministic, independently
//!   inspectable signals; a gate on eligibility and verification; a coarse
//!   tier (strong fit, worth reviewing, maybe, low priority) instead of a
//!   match percentage; and the decision brief.
//! * [`cache`]: stored rankings keyed by every input, and the
//!   [`FeedbackRepository`] / [`RankingRepository`] storage boundaries.
//! * [`service`]: the use cases front-ends call.
//!
//! No SQL, HTTP, CLI formatting or AI vendor code lives here.

pub mod brief;
pub mod cache;
pub mod facets;
pub mod feedback;
pub mod key;
pub mod person;
pub mod rank;
pub mod reason;
pub mod service;
pub mod signals;
pub mod taste;
#[cfg(test)]
mod testing;

pub use brief::DecisionBrief;
pub use cache::{FeedbackRepository, RankKey, RankingRepository, cached_rank};
pub use facets::{JobFacets, JobFunction, Level, facets, facets_of};
pub use feedback::{FeedbackAction, FeedbackEvent, FeedbackId, OpportunityState, Sentiment, Stage};
pub use key::{Dimension, Direction, TasteKey};
pub use person::Person;
pub use rank::{Candidate, Context, Exclusion, Gate, RANKING_VERSION, Ranking, Tier, rank};
pub use reason::{RULE_READER_REVISION, ReasonReader, ReasonReading, RuleReader};
pub use service::{
    Excluded, Explained, PipelineEntry, RankQuery, RankReport, RankTimings, RankingError,
    RankingService, Recorded,
};
pub use signals::{Basis, Signal, SignalGroup, SignalKind};
pub use taste::{Confidence, LearnedTaste, TASTE_VERSION, TasteModel, TasteStatus};
