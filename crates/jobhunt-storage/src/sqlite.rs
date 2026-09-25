//! Local SQLite job store.

use std::path::Path;
use std::str::FromStr;
use std::sync::LazyLock;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_core::text::search_key;
use jobhunt_core::{BoxError, CanonicalUrl, Provenance, SourceKey, UpsertOutcome};
use jobhunt_jobs::{
    Compensation, EmploymentType, JobId, JobPosting, JobQuery, JobRecord, JobRepository,
    SourceLocation, StorageError, WorkplaceType,
};
use sqlx::sqlite::{
    SqliteArguments, SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions,
    SqliteRow, SqliteSynchronous,
};
use sqlx::{QueryBuilder, Row, Sqlite};
use tracing::{debug, info};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations/sqlite");

/// Content columns written on insert and on content update, in bind order.
/// Must match [`ContentValues::bind`].
const CONTENT_COLUMNS: [&str; 22] = [
    "source_kind",
    "source_instance",
    "source_job_id",
    "fetched_from",
    "url",
    "apply_url",
    "company",
    "title",
    "department",
    "team",
    "location",
    "locations",
    "employment_type",
    "workplace_type",
    "is_remote",
    "compensation",
    "description_text",
    "description_html",
    "posted_at",
    "source_updated_at",
    "search_text",
    "fingerprint",
];

static INSERT_SQL: LazyLock<String> = LazyLock::new(|| {
    let columns = CONTENT_COLUMNS.join(", ");
    let placeholders = vec!["?"; CONTENT_COLUMNS.len() + 4].join(", ");
    format!(
        "INSERT INTO jobs (id, {columns}, first_seen_at, last_seen_at, content_updated_at) \
         VALUES ({placeholders})"
    )
});

static UPDATE_CONTENT_SQL: LazyLock<String> = LazyLock::new(|| {
    let assignments: Vec<String> = CONTENT_COLUMNS.iter().map(|c| format!("{c} = ?")).collect();
    format!(
        "UPDATE jobs SET {}, last_seen_at = ?, content_updated_at = ? WHERE id = ?",
        assignments.join(", ")
    )
});

/// SQLite implementation of [`JobRepository`].
///
/// Opening a store creates the database file (and its directory) if needed
/// and applies pending migrations, so callers never see an uninitialized
/// schema.
#[derive(Debug, Clone)]
pub struct SqliteJobStore {
    pool: SqlitePool,
    location: String,
}

impl SqliteJobStore {
    /// Opens (creating if necessary) the database at `path` and migrates it.
    pub async fn open(path: &Path) -> Result<Self, StorageError> {
        let location = path.display().to_string();
        let open_error = |source: BoxError| StorageError::Open {
            location: location.clone(),
            source,
        };

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| open_error(Box::new(e)))?;
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|e| open_error(Box::new(e)))?;
        Self::initialize(pool, location).await
    }

    /// Opens a private in-memory database, mainly for tests.
    pub async fn open_in_memory() -> Result<Self, StorageError> {
        let location = ":memory:".to_owned();
        let open_error = |source: sqlx::Error| StorageError::Open {
            location: location.clone(),
            source: Box::new(source),
        };
        let options = SqliteConnectOptions::from_str("sqlite::memory:")
            .map_err(open_error)?
            .foreign_keys(true);
        // Every in-memory connection is a separate database, so keep exactly
        // one connection alive for the lifetime of the pool.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .min_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(options)
            .await
            .map_err(open_error)?;
        Self::initialize(pool, location).await
    }

    async fn initialize(pool: SqlitePool, location: String) -> Result<Self, StorageError> {
        MIGRATOR
            .run(&pool)
            .await
            .map_err(|e| StorageError::Migration(Box::new(e)))?;
        info!(database = %location, "job store ready");
        Ok(Self { pool, location })
    }

    /// Human-readable location of the database (a path or `:memory:`).
    pub fn location(&self) -> &str {
        &self.location
    }

    /// Closes all connections, flushing the write-ahead log.
    pub async fn close(self) {
        self.pool.close().await;
    }
}

#[async_trait]
impl JobRepository for SqliteJobStore {
    async fn upsert_postings(
        &self,
        postings: &[JobPosting],
        observed_at: DateTime<Utc>,
    ) -> Result<Vec<UpsertOutcome>, StorageError> {
        let observed = encode_timestamp(observed_at);
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        let mut outcomes = Vec::with_capacity(postings.len());

        for posting in postings {
            let id = posting.id().to_string();
            let values = ContentValues::from_posting(posting)?;
            let existing: Option<String> =
                sqlx::query_scalar("SELECT fingerprint FROM jobs WHERE id = ?")
                    .bind(&id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(query_error("looking up a stored job"))?;

            let outcome = match existing {
                None => {
                    values
                        .bind(sqlx::query(&INSERT_SQL).bind(&id))
                        .bind(&observed)
                        .bind(&observed)
                        .bind(&observed)
                        .execute(&mut *tx)
                        .await
                        .map_err(query_error("inserting a job"))?;
                    UpsertOutcome::Inserted
                }
                Some(stored) if stored == values.fingerprint => {
                    // search_text is derived, so refresh it too in case the
                    // normalization rules changed since it was written.
                    sqlx::query("UPDATE jobs SET last_seen_at = ?, search_text = ? WHERE id = ?")
                        .bind(&observed)
                        .bind(&values.search_text)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(query_error("marking a job as seen"))?;
                    UpsertOutcome::Unchanged
                }
                Some(_) => {
                    values
                        .bind(sqlx::query(&UPDATE_CONTENT_SQL))
                        .bind(&observed)
                        .bind(&observed)
                        .bind(&id)
                        .execute(&mut *tx)
                        .await
                        .map_err(query_error("updating a job"))?;
                    UpsertOutcome::Updated
                }
            };
            outcomes.push(outcome);
        }

        tx.commit()
            .await
            .map_err(query_error("committing stored jobs"))?;
        debug!(count = postings.len(), "upserted postings");
        Ok(outcomes)
    }

    async fn get(&self, id: JobId) -> Result<Option<JobRecord>, StorageError> {
        let row = sqlx::query("SELECT * FROM jobs WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(query_error("loading a job"))?;
        row.as_ref().map(decode_record).transpose()
    }

    async fn search(&self, query: &JobQuery) -> Result<Vec<JobRecord>, StorageError> {
        let mut builder = QueryBuilder::<Sqlite>::new("SELECT * FROM jobs");
        push_filters(&mut builder, query);
        builder.push(" ORDER BY posted_at IS NULL, posted_at DESC, first_seen_at DESC, id");
        if let Some(limit) = query.limit {
            builder
                .push(" LIMIT ")
                .push_bind(i64::try_from(limit).unwrap_or(i64::MAX));
        }
        let rows = builder
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(query_error("searching jobs"))?;
        rows.iter().map(decode_record).collect()
    }

    async fn count(&self, query: &JobQuery) -> Result<u64, StorageError> {
        let mut builder = QueryBuilder::<Sqlite>::new("SELECT COUNT(*) FROM jobs");
        push_filters(&mut builder, query);
        let count: i64 = builder
            .build_query_scalar()
            .fetch_one(&self.pool)
            .await
            .map_err(query_error("counting jobs"))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }
}

fn push_filters(builder: &mut QueryBuilder<'_, Sqlite>, query: &JobQuery) {
    builder.push(" WHERE 1 = 1");
    for term in &query.terms {
        // search_text is " word word ... " (normalized in Rust), so matching
        // " term" finds terms at the start of a word. Terms are normalized
        // the same way, which makes case handling Unicode-aware; escaping is
        // defensive since normalized terms contain no LIKE wildcards.
        let pattern = format!("% {}%", escape_like(&search_key(term)));
        builder
            .push(" AND search_text LIKE ")
            .push_bind(pattern)
            .push(" ESCAPE '\\'");
    }
    if !query.sources.is_empty() {
        builder.push(" AND (");
        for (i, source) in query.sources.iter().enumerate() {
            if i > 0 {
                builder.push(" OR ");
            }
            builder
                .push("(source_kind = ")
                .push_bind(source.kind().to_owned())
                .push(" AND source_instance = ")
                .push_bind(source.instance().to_owned())
                .push(")");
        }
        builder.push(")");
    }
    if let Some(since) = query.seen_since {
        builder
            .push(" AND last_seen_at >= ")
            .push_bind(encode_timestamp(since));
    }
}

fn escape_like(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for c in term.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A posting flattened into column values.
struct ContentValues {
    source_kind: String,
    source_instance: String,
    source_job_id: Option<String>,
    fetched_from: Option<String>,
    url: String,
    apply_url: Option<String>,
    company: String,
    title: String,
    department: Option<String>,
    team: Option<String>,
    location: Option<String>,
    locations: String,
    employment_type: Option<String>,
    workplace_type: Option<String>,
    is_remote: Option<bool>,
    compensation: Option<String>,
    description_text: Option<String>,
    description_html: Option<String>,
    posted_at: Option<String>,
    source_updated_at: Option<String>,
    search_text: String,
    fingerprint: String,
}

impl ContentValues {
    fn from_posting(p: &JobPosting) -> Result<Self, StorageError> {
        let encode_error = |e: serde_json::Error| StorageError::Query {
            operation: "encoding a job for storage",
            source: Box::new(e),
        };
        Ok(Self {
            source_kind: p.provenance.source.kind().to_owned(),
            source_instance: p.provenance.source.instance().to_owned(),
            source_job_id: p.provenance.source_record_id.clone(),
            fetched_from: p.provenance.fetched_from.as_ref().map(|u| u.to_string()),
            url: p.url.to_string(),
            apply_url: p.apply_url.as_ref().map(|u| u.to_string()),
            company: p.company.clone(),
            title: p.title.clone(),
            department: p.department.clone(),
            team: p.team.clone(),
            location: p.location.clone(),
            locations: serde_json::to_string(&p.locations).map_err(encode_error)?,
            employment_type: p.employment_type.as_ref().map(|t| t.as_str().to_owned()),
            workplace_type: p.workplace_type.as_ref().map(|t| t.as_str().to_owned()),
            is_remote: p.is_remote,
            compensation: p
                .compensation
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(encode_error)?,
            description_text: p.description_text.clone(),
            description_html: p.description_html.clone(),
            posted_at: p.posted_at.map(encode_timestamp),
            source_updated_at: p.source_updated_at.map(encode_timestamp),
            search_text: p.search_document(),
            fingerprint: p.fingerprint().to_hex(),
        })
    }

    /// Binds the values in [`CONTENT_COLUMNS`] order.
    fn bind<'q>(
        &'q self,
        query: sqlx::query::Query<'q, Sqlite, SqliteArguments<'q>>,
    ) -> sqlx::query::Query<'q, Sqlite, SqliteArguments<'q>> {
        query
            .bind(&self.source_kind)
            .bind(&self.source_instance)
            .bind(&self.source_job_id)
            .bind(&self.fetched_from)
            .bind(&self.url)
            .bind(&self.apply_url)
            .bind(&self.company)
            .bind(&self.title)
            .bind(&self.department)
            .bind(&self.team)
            .bind(&self.location)
            .bind(&self.locations)
            .bind(&self.employment_type)
            .bind(&self.workplace_type)
            .bind(self.is_remote)
            .bind(&self.compensation)
            .bind(&self.description_text)
            .bind(&self.description_html)
            .bind(&self.posted_at)
            .bind(&self.source_updated_at)
            .bind(&self.search_text)
            .bind(&self.fingerprint)
    }
}

fn decode_record(row: &SqliteRow) -> Result<JobRecord, StorageError> {
    let raw_id: String = row
        .try_get("id")
        .map_err(|e| corrupt("<unknown>", format!("id: {e}")))?;
    let get_text = |column: &'static str| -> Result<String, StorageError> {
        row.try_get(column)
            .map_err(|e| corrupt(&raw_id, format!("{column}: {e}")))
    };
    let get_opt = |column: &'static str| -> Result<Option<String>, StorageError> {
        row.try_get(column)
            .map_err(|e| corrupt(&raw_id, format!("{column}: {e}")))
    };
    let url = |column: &'static str, value: &str| {
        CanonicalUrl::parse(value).map_err(|e| corrupt(&raw_id, format!("{column}: {e}")))
    };
    let timestamp = |column: &'static str, value: &str| {
        decode_timestamp(value).map_err(|e| corrupt(&raw_id, format!("{column}: {e}")))
    };

    let id: JobId = raw_id
        .parse()
        .map_err(|e| corrupt(&raw_id, format!("id: {e}")))?;
    let source = SourceKey::new(&get_text("source_kind")?, &get_text("source_instance")?)
        .map_err(|e| corrupt(&raw_id, format!("source: {e}")))?;
    let locations: Vec<SourceLocation> = serde_json::from_str(&get_text("locations")?)
        .map_err(|e| corrupt(&raw_id, format!("locations: {e}")))?;
    let compensation: Option<Compensation> = get_opt("compensation")?
        .map(|json| serde_json::from_str(&json))
        .transpose()
        .map_err(|e| corrupt(&raw_id, format!("compensation: {e}")))?;
    let is_remote: Option<bool> = row
        .try_get("is_remote")
        .map_err(|e| corrupt(&raw_id, format!("is_remote: {e}")))?;

    let posting = JobPosting {
        provenance: Provenance {
            source,
            source_record_id: get_opt("source_job_id")?,
            fetched_from: get_opt("fetched_from")?
                .map(|v| url("fetched_from", &v))
                .transpose()?,
        },
        url: url("url", &get_text("url")?)?,
        apply_url: get_opt("apply_url")?
            .map(|v| url("apply_url", &v))
            .transpose()?,
        company: get_text("company")?,
        title: get_text("title")?,
        department: get_opt("department")?,
        team: get_opt("team")?,
        location: get_opt("location")?,
        locations,
        employment_type: get_opt("employment_type")?
            .as_deref()
            .map(EmploymentType::from_canonical),
        workplace_type: get_opt("workplace_type")?
            .as_deref()
            .map(WorkplaceType::from_canonical),
        is_remote,
        compensation,
        description_text: get_opt("description_text")?,
        description_html: get_opt("description_html")?,
        posted_at: get_opt("posted_at")?
            .map(|v| timestamp("posted_at", &v))
            .transpose()?,
        source_updated_at: get_opt("source_updated_at")?
            .map(|v| timestamp("source_updated_at", &v))
            .transpose()?,
    };

    Ok(JobRecord {
        id,
        posting,
        first_seen_at: timestamp("first_seen_at", &get_text("first_seen_at")?)?,
        last_seen_at: timestamp("last_seen_at", &get_text("last_seen_at")?)?,
        content_updated_at: timestamp("content_updated_at", &get_text("content_updated_at")?)?,
    })
}

/// Fixed-width UTC RFC 3339 with microseconds, so values sort as text.
/// Sub-microsecond precision is dropped.
fn encode_timestamp(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

fn decode_timestamp(value: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(value).map(|t| t.with_timezone(&Utc))
}

fn corrupt(id: &str, detail: String) -> StorageError {
    StorageError::Corrupt {
        id: id.to_owned(),
        detail,
    }
}

fn query_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |source| StorageError::Query {
        operation,
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use jobhunt_jobs::{CompensationComponent, CompensationKind, PayInterval};

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
    }

    fn posting(instance: &str, native_id: &str, title: &str) -> JobPosting {
        JobPosting {
            provenance: Provenance {
                source: SourceKey::new("ashby", instance).unwrap(),
                source_record_id: Some(native_id.to_owned()),
                fetched_from: Some(
                    CanonicalUrl::parse(&format!(
                        "https://api.ashbyhq.com/posting-api/job-board/{instance}?includeCompensation=true"
                    ))
                    .unwrap(),
                ),
            },
            url: CanonicalUrl::parse(&format!("https://jobs.ashbyhq.com/{instance}/{native_id}"))
                .unwrap(),
            apply_url: Some(
                CanonicalUrl::parse(&format!(
                    "https://jobs.ashbyhq.com/{instance}/{native_id}/application"
                ))
                .unwrap(),
            ),
            company: instance.to_uppercase(),
            title: title.to_owned(),
            department: Some("Engineering".into()),
            team: Some("Platform".into()),
            location: Some("New York, NY (HQ)".into()),
            locations: vec![
                SourceLocation {
                    name: Some("New York, NY (HQ)".into()),
                    locality: Some("New York City".into()),
                    region: Some("NY".into()),
                    country: Some("USA".into()),
                },
                SourceLocation {
                    name: Some("Remote (Canada)".into()),
                    country: Some("Canada".into()),
                    ..Default::default()
                },
            ],
            employment_type: Some(EmploymentType::FullTime),
            workplace_type: Some(WorkplaceType::Hybrid),
            is_remote: Some(true),
            compensation: Some(Compensation {
                summary: Some("$211.4K – $290.6K • Offers Equity".into()),
                components: vec![
                    CompensationComponent {
                        kind: CompensationKind::Salary,
                        currency: Some("USD".into()),
                        min: Some(211_400.0),
                        max: Some(290_600.0),
                        interval: Some(PayInterval::Year),
                    },
                    CompensationComponent {
                        kind: CompensationKind::EquityPercentage,
                        currency: None,
                        min: None,
                        max: None,
                        interval: None,
                    },
                ],
            }),
            description_text: Some("Build things.\n\nWith care.".into()),
            description_html: Some("<p>Build things.</p><p>With care.</p>".into()),
            posted_at: Some(Utc.with_ymd_and_hms(2026, 4, 7, 17, 12, 35).unwrap()),
            source_updated_at: None,
        }
    }

    async fn store() -> SqliteJobStore {
        SqliteJobStore::open_in_memory().await.unwrap()
    }

    #[tokio::test]
    async fn migrations_create_the_schema() {
        let store = store().await;
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(applied, MIGRATOR.iter().count() as i64);
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'jobs'",
        )
        .fetch_all(&store.pool)
        .await
        .unwrap();
        assert_eq!(tables, vec!["jobs"]);
    }

    #[tokio::test]
    async fn reopening_a_database_file_keeps_data_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("jobhunt.db");

        let first = SqliteJobStore::open(&path).await.unwrap();
        first
            .upsert_postings(&[posting("ramp", "1", "Engineer")], at(0))
            .await
            .unwrap();
        first.close().await;

        let second = SqliteJobStore::open(&path).await.unwrap();
        assert_eq!(second.count(&JobQuery::default()).await.unwrap(), 1);
        let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations")
            .fetch_one(&second.pool)
            .await
            .unwrap();
        assert_eq!(applied, MIGRATOR.iter().count() as i64);
    }

    #[tokio::test]
    async fn round_trips_every_field() {
        let store = store().await;
        let original = posting("ramp", "34413f8d", "Security Engineer, Cloud");
        store
            .upsert_postings(std::slice::from_ref(&original), at(0))
            .await
            .unwrap();

        let record = store.get(original.id()).await.unwrap().unwrap();
        assert_eq!(record.id, original.id());
        assert_eq!(record.posting, original);
        assert_eq!(record.first_seen_at, at(0));
        assert_eq!(record.last_seen_at, at(0));
        assert_eq!(record.content_updated_at, at(0));
        assert_eq!(record.posting.fingerprint(), original.fingerprint());
    }

    #[tokio::test]
    async fn upsert_distinguishes_inserted_unchanged_and_updated() {
        let store = store().await;
        let a = posting("ramp", "1", "Engineer");
        let b = posting("ramp", "2", "Designer");

        let outcomes = store
            .upsert_postings(&[a.clone(), b.clone()], at(0))
            .await
            .unwrap();
        assert_eq!(
            outcomes,
            vec![UpsertOutcome::Inserted, UpsertOutcome::Inserted]
        );

        let mut a_edited = a.clone();
        a_edited.title = "Senior Engineer".into();
        let outcomes = store
            .upsert_postings(&[a_edited.clone(), b.clone()], at(5))
            .await
            .unwrap();
        assert_eq!(
            outcomes,
            vec![UpsertOutcome::Updated, UpsertOutcome::Unchanged]
        );
        assert_eq!(store.count(&JobQuery::default()).await.unwrap(), 2);

        let a_record = store.get(a.id()).await.unwrap().unwrap();
        assert_eq!(a_record.posting.title, "Senior Engineer");
        assert_eq!(a_record.first_seen_at, at(0));
        assert_eq!(a_record.last_seen_at, at(5));
        assert_eq!(a_record.content_updated_at, at(5));

        let b_record = store.get(b.id()).await.unwrap().unwrap();
        assert_eq!(b_record.first_seen_at, at(0));
        assert_eq!(b_record.last_seen_at, at(5));
        assert_eq!(b_record.content_updated_at, at(0));
    }

    #[tokio::test]
    async fn schema_rejects_two_rows_for_one_source_job() {
        let store = store().await;
        let p = posting("ramp", "1", "Engineer");
        store
            .upsert_postings(std::slice::from_ref(&p), at(0))
            .await
            .unwrap();
        let values = ContentValues::from_posting(&p).unwrap();
        let stamp = encode_timestamp(at(1));
        let result = values
            .bind(sqlx::query(&INSERT_SQL).bind("job_ffffffffffffffffffffffffffffffff"))
            .bind(&stamp)
            .bind(&stamp)
            .bind(&stamp)
            .execute(&store.pool)
            .await;
        assert!(
            result.is_err(),
            "duplicate source identity must be rejected"
        );
    }

    #[tokio::test]
    async fn search_filters_orders_and_limits() {
        let store = store().await;
        let mut old = posting("ramp", "1", "Backend Engineer");
        old.posted_at = Some(at(1));
        let mut new = posting("ramp", "2", "Frontend Engineer");
        new.posted_at = Some(at(2));
        let mut undated = posting("linear", "3", "Product Designer");
        undated.posted_at = None;
        undated.department = Some("Design".into());
        undated.team = None;
        store
            .upsert_postings(&[old, new, undated], at(10))
            .await
            .unwrap();

        let titles = |records: Vec<JobRecord>| -> Vec<String> {
            records.into_iter().map(|r| r.posting.title).collect()
        };

        let all = store.search(&JobQuery::default()).await.unwrap();
        assert_eq!(
            titles(all),
            vec!["Frontend Engineer", "Backend Engineer", "Product Designer"]
        );

        let engineers = JobQuery::default().with_text("ENGINEER");
        assert_eq!(store.count(&engineers).await.unwrap(), 2);

        let multi = JobQuery::default().with_text("engineer backend");
        assert_eq!(
            titles(store.search(&multi).await.unwrap()),
            vec!["Backend Engineer"]
        );

        let by_company = JobQuery::default().with_text("linear");
        assert_eq!(
            titles(store.search(&by_company).await.unwrap()),
            vec!["Product Designer"]
        );

        let by_source = JobQuery {
            sources: vec!["ashby:ramp".parse().unwrap()],
            limit: Some(1),
            ..Default::default()
        };
        assert_eq!(
            titles(store.search(&by_source).await.unwrap()),
            vec!["Frontend Engineer"]
        );
        assert_eq!(
            store.count(&by_source).await.unwrap(),
            2,
            "count ignores limit"
        );
    }

    #[tokio::test]
    async fn search_by_seen_since_excludes_jobs_not_seen_recently() {
        let store = store().await;
        let stale = posting("ramp", "1", "Engineer");
        let fresh = posting("ramp", "2", "Designer");
        store
            .upsert_postings(&[stale, fresh.clone()], at(0))
            .await
            .unwrap();
        store.upsert_postings(&[fresh], at(30)).await.unwrap();

        let query = JobQuery {
            seen_since: Some(at(30)),
            ..Default::default()
        };
        let found = store.search(&query).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].posting.title, "Designer");
    }

    #[tokio::test]
    async fn terms_match_word_starts_not_arbitrary_substrings() {
        let store = store().await;
        let mut trust = posting("ramp", "1", "Program Manager");
        trust.department = Some("Trust & Safety".into());
        trust.team = None;
        let mut rust = posting("ramp", "2", "Backend Engineer (Rust)");
        rust.department = None;
        rust.team = None;
        let mut munich = posting("ramp", "3", "Account Executive");
        munich.location = Some("MÜNCHEN".into());
        munich.department = Some("Sales".into());
        munich.team = None;
        store
            .upsert_postings(&[trust, rust, munich], at(0))
            .await
            .unwrap();

        let titles = |q: &str| {
            let store = store.clone();
            let query = JobQuery::default().with_text(q);
            async move {
                store
                    .search(&query)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|r| r.posting.title)
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(titles("rust").await, vec!["Backend Engineer (Rust)"]);
        assert_eq!(titles("eng").await, vec!["Backend Engineer (Rust)"]);
        assert_eq!(titles("trust safety").await, vec!["Program Manager"]);
        assert_eq!(titles("münchen").await, vec!["Account Executive"]);
        assert_eq!(titles("100%").await, Vec::<String>::new());
    }

    #[test]
    fn escapes_like_wildcards() {
        assert_eq!(escape_like(r"50%_off\"), r"50\%\_off\\");
    }

    #[test]
    fn timestamps_are_fixed_width_and_round_trip() {
        let t = Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).unwrap();
        let encoded = encode_timestamp(t);
        assert_eq!(encoded, "2026-01-02T03:04:05.000000Z");
        assert_eq!(decode_timestamp(&encoded).unwrap(), t);
    }
}
