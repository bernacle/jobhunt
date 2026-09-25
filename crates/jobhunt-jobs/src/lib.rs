//! The JobHunt jobs domain.
//!
//! * [`model`]: the canonical job representation every source converts into.
//! * [`repository`]: the persistence boundary. Storage backends (SQLite today,
//!   Postgres later) implement [`JobRepository`]; nothing in this crate knows
//!   which backend is in use.
//! * [`discovery`]: the pipeline that runs sources, validates and
//!   de-duplicates their output, and persists it through the repository.

pub mod discovery;
pub mod model;
pub mod repository;

pub use discovery::{Discovery, DiscoveryReport, JobSource, SourceReport};
pub use model::{
    Compensation, CompensationComponent, CompensationKind, EmploymentType, JobId, JobPosting,
    JobRecord, PayInterval, SourceLocation, WorkplaceType,
};
pub use repository::{JobQuery, JobRepository, StorageError};
