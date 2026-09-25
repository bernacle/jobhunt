//! The JobHunt jobs domain.
//!
//! * [`model`]: the canonical job representation every source converts into.
//! * [`lifecycle`]: NEW / UNCHANGED / UPDATED / CLOSED / REOPENED rules for a
//!   source scan.
//! * [`identity`]: conservative cross-source equivalence (opportunities).
//! * [`repository`]: the persistence boundary. Storage backends (SQLite today,
//!   Postgres later) implement [`JobRepository`]; nothing in this crate knows
//!   which backend is in use.
//! * [`discovery`]: the pipeline that runs sources, validates and
//!   de-duplicates their output, and persists it through the repository.

pub mod discovery;
pub mod identity;
pub mod lifecycle;
#[cfg(test)]
mod memory;
pub mod model;
pub mod repository;

pub use discovery::{
    Closing, DedupeStats, Discovery, DiscoveryReport, JobSource, ScanKind, ScanStats, SourceReport,
};
pub use identity::{AtsJobRef, IdentityEntry, ats_job_ref, evidence_keys};
pub use model::{
    CANONICAL_REVISION, Compensation, CompensationComponent, CompensationKind, EmploymentType,
    JobId, JobPosting, JobRecord, JobSnapshot, JobStatus, OpportunityId, PayInterval,
    SourceLocation, WorkplaceType,
};
pub use repository::{
    JobEvent, JobEventKind, JobQuery, JobRepository, LastListing, RunId, RunSummary, ScanBody,
    ScanResult, ScanWrite, StorageError,
};
