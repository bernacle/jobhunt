//! Storage backends for JobHunt.
//!
//! Each backend implements [`jobhunt_jobs::JobRepository`] and owns its own
//! schema and migrations. Today there is one: [`SqliteJobStore`], the local
//! store. A Postgres backend for the cloud would live next to it as its own
//! module (with its own `migrations/postgres` directory) implementing the same
//! trait; nothing in the jobs domain or discovery pipeline would change.

mod sqlite;

pub use sqlite::SqliteJobStore;
