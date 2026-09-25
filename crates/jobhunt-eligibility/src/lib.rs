//! Eligibility and first-party verification: can this user take this job,
//! and is what JobHunt knows about it the company's own, current word?
//!
//! The job side ([`facts`]) reads a stored job's locations, remote
//! metadata, work-authorization field, employment type, pay and description
//! into facts that each carry their [`facts::Evidence`]. The user side
//! ([`user`]) reads the profile's preferences: where the user lives, work
//! modes, time zones, relocation, sponsorship, pay minimums and roles.
//! [`assess`] checks one against the other and answers each dimension with
//! a [`assess::Fit`] (yes, likely, unknown, unlikely, no) and a one-line
//! summary ("Brazil eligible: remote in Latin America", "Remote, but the
//! United States only", "Compensation unknown"). [`verify`] says whether
//! the job's records come from the company's own board and how recently
//! they were seen there.
//!
//! Nothing is inferred from silence. A remote flag with no place is
//! "unknown", not "anywhere"; "$" is a currency only when the job's places
//! settle which dollar; pay is compared only in the same currency. The
//! geography ([`geo`]) and time zones ([`zones`]) are small tables: a place
//! JobHunt does not know stays unrecognized rather than guessed.
//!
//! Assessments are computed on demand and not stored: they depend on both
//! the job and the profile, and either can change at any time.

pub mod assess;
pub mod facts;
pub mod geo;
pub mod user;
pub mod verify;
pub mod zones;

pub use assess::{Assessment, Check, Dimension, Fit, assess, assess_record};
pub use facts::{Evidence, JobFacts, job_facts};
pub use user::UserConstraints;
pub use verify::{Freshness, Verification, verify};
