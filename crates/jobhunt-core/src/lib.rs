//! Domain-agnostic discovery primitives.
//!
//! Nothing in this crate knows about jobs. It holds the building blocks any
//! discovery product needs: canonical URLs, deterministic identifiers, content
//! fingerprints, source identity/provenance, the [`source::Source`] adapter
//! contract, and ingest bookkeeping. Product domains (such as `jobhunt-jobs`)
//! build their vocabulary on top of these.

pub mod error;
pub mod fingerprint;
pub mod html;
pub mod id;
pub mod ingest;
pub mod source;
pub mod text;
pub mod urls;

mod hex;

pub use error::{BoxError, ErrorChain};
pub use fingerprint::{Fingerprint, FingerprintBuilder};
pub use id::{ParseIdError, StableId};
pub use ingest::{IngestCounts, UpsertOutcome};
pub use source::{
    FetchRequest, Fetched, Provenance, RecordError, RecordErrorReason, Source, SourceBatch,
    SourceError, SourceKey, SourceKeyError,
};
pub use urls::{CanonicalUrl, UrlError};
