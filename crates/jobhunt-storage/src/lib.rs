//! Storage backends for JobHunt.
//!
//! Each backend implements [`jobhunt_jobs::JobRepository`] and
//! [`jobhunt_profile::ProfileRepository`] and owns its own schema and
//! migrations. Today there is one: [`SqliteJobStore`], the local store (one
//! SQLite file holds jobs and the career profile). A Postgres backend for the
//! cloud would live next to it as its own module (with its own
//! `migrations/postgres` directory) implementing the same traits; nothing in
//! the jobs or profile domains, or the discovery pipeline, would change.

mod profile;
mod sqlite;

pub use sqlite::SqliteJobStore;
