//! Eligibility: can this person work this job, from where they are, on
//! the terms the posting states? And what is known versus unknown?
//!
//! * [`job`] reads a stored job into [`job::JobRequirements`]: the ways it
//!   can be done (remote in a scope, offices, contractor paths), places it
//!   allows or rules out, work authorization, sponsorship, engagement,
//!   time zones, relocation, and the conflicts between its statements,
//!   each with its [`job::Evidence`].
//! * [`profile`] reads the person's facts from the profile domain.
//! * [`geo`] and [`zones`] normalize places and time zones: ISO country
//!   codes, the documented region definitions, UTC offsets.
//! * [`rules`] are the deterministic rules, one function each, in order.
//! * [`decision`] is the result: eligible, conditional, uncertain or
//!   ineligible, with traceable [`decision::Reason`]s.
//! * [`evaluate`](mod@evaluate) runs the rules per job and per opportunity, and puts the
//!   verification trust gate next to the decision.
//! * [`cache`] stores decisions under a key of everything they depend on.
//! * [`describe`] states a job's requirements as short facts for display.
//!
//! Nothing is inferred from silence: "Remote" is not "anywhere", "no
//! sponsorship" is not "no international applicants", a missing profile
//! fact is unknown, and every conclusion names the words it rests on. This
//! is a compatibility signal from published restrictions and what the
//! person stated, not legal advice about work authorization. Whether a job
//! is desirable (pay, role, ranking) is not decided here.

pub mod cache;
pub mod decision;
pub mod describe;
pub mod evaluate;
pub mod geo;
pub mod job;
pub mod profile;
pub mod rules;
pub mod zones;

pub use cache::{CacheKey, EligibilityRepository, cached_assess};
pub use decision::{
    ConflictNote, Eligibility, EligibilityDecision, EvidenceRef, OptionDecision, ProfileFact,
    RULES_VERSION, Reason, RuleId, Verdict,
};
pub use evaluate::{Assessment, assess, evaluate, evaluate_record, evaluate_sources};
pub use job::{JobRequirements, requirements};
pub use profile::ProfileFacts;
