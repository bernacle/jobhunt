//! [`ProfileRepository`] for the SQLite store.
//!
//! A profile is stored relationally (one table per record kind, see the
//! `career_profile` migration). Saving writes the whole aggregate in one
//! transaction: records are upserted by id, records of the profile that are
//! no longer in the aggregate are deleted, and the derived skill-evidence
//! links are rebuilt. The revision check makes concurrent writers fail
//! instead of overwriting each other.

use std::collections::HashSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use jobhunt_profile::{
    Certainty, Claim, ClaimKind, ClaimQuery, Confidence, Contact, DocumentKind, Education,
    EmploymentKind, Experience, Origin, PartialDate, Preference, PreferenceOrigin,
    PreferenceStatement, PreferenceValue, Profile, ProfileData, ProfileEvent, ProfileEventKind,
    ProfileId, ProfileRepository, Project, Provenance, RecordMeta, Skill, SourceDocument,
    SourceRef, SpokenLanguage, Stance, StatementReading, StorageError, Subject, Verification,
};
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Row, Sqlite, SqliteConnection};

use crate::sqlite::{SqliteJobStore, decode_timestamp, encode_timestamp};

/// Ids bound per statement.
const BATCH: usize = 500;

fn query_error(operation: &'static str) -> impl FnOnce(sqlx::Error) -> StorageError {
    move |source| StorageError::Query {
        operation,
        source: Box::new(source),
    }
}

fn encode_error(operation: &'static str) -> impl FnOnce(serde_json::Error) -> StorageError {
    move |source| StorageError::Query {
        operation,
        source: Box::new(source),
    }
}

fn corrupt(record: &str, detail: impl Into<String>) -> StorageError {
    StorageError::Corrupt {
        record: record.to_owned(),
        detail: detail.into(),
    }
}

/// Typed column access that reports corrupt rows with their id.
struct Cols<'r> {
    row: &'r SqliteRow,
    id: String,
}

impl<'r> Cols<'r> {
    fn new(row: &'r SqliteRow, id_column: &str) -> Self {
        let id = row
            .try_get::<String, _>(id_column)
            .unwrap_or_else(|_| "<unknown>".to_owned());
        Self { row, id }
    }

    fn text(&self, column: &str) -> Result<String, StorageError> {
        self.row
            .try_get(column)
            .map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn opt(&self, column: &str) -> Result<Option<String>, StorageError> {
        self.row
            .try_get(column)
            .map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn int(&self, column: &str) -> Result<i64, StorageError> {
        self.row
            .try_get(column)
            .map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn flag(&self, column: &str) -> Result<bool, StorageError> {
        Ok(self.int(column)? != 0)
    }

    fn u32(&self, column: &str) -> Result<u32, StorageError> {
        u32::try_from(self.int(column)?).map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn time(&self, column: &str) -> Result<DateTime<Utc>, StorageError> {
        decode_timestamp(&self.text(column)?)
            .map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn opt_time(&self, column: &str) -> Result<Option<DateTime<Utc>>, StorageError> {
        self.opt(column)?
            .map(|v| decode_timestamp(&v).map_err(|e| corrupt(&self.id, format!("{column}: {e}"))))
            .transpose()
    }

    fn date(&self, column: &str) -> Result<Option<PartialDate>, StorageError> {
        self.opt(column)?
            .map(|v| {
                v.parse().map_err(|e: jobhunt_profile::ParseDateError| {
                    corrupt(&self.id, format!("{column}: {e}"))
                })
            })
            .transpose()
    }

    fn parse<T: std::str::FromStr>(&self, column: &str) -> Result<T, StorageError>
    where
        T::Err: std::fmt::Display,
    {
        self.text(column)?
            .parse()
            .map_err(|e: T::Err| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn opt_parse<T: std::str::FromStr>(&self, column: &str) -> Result<Option<T>, StorageError>
    where
        T::Err: std::fmt::Display,
    {
        self.opt(column)?
            .map(|v| {
                v.parse()
                    .map_err(|e: T::Err| corrupt(&self.id, format!("{column}: {e}")))
            })
            .transpose()
    }

    fn json<T: serde::de::DeserializeOwned>(&self, column: &str) -> Result<T, StorageError> {
        serde_json::from_str(&self.text(column)?)
            .map_err(|e| corrupt(&self.id, format!("{column}: {e}")))
    }

    fn canonical<T>(&self, column: &str, decode: fn(&str) -> Option<T>) -> Result<T, StorageError> {
        let value = self.text(column)?;
        decode(&value)
            .ok_or_else(|| corrupt(&self.id, format!("{column}: unknown value {value:?}")))
    }

    fn meta(&self) -> Result<RecordMeta, StorageError> {
        let document = self.opt_parse("document_id")?;
        let snippet = self.opt("source_snippet")?;
        Ok(RecordMeta {
            origin: self.canonical("origin", Origin::from_canonical)?,
            source: match (document, snippet) {
                (Some(document), Some(snippet)) => Some(SourceRef {
                    document,
                    snippet,
                    section: self.opt("source_section")?,
                }),
                _ => None,
            },
            import_key: self.opt("import_key")?,
            verification: self.canonical("verification", Verification::from_canonical)?,
            stale_since: self.opt_time("stale_since")?,
            edited_fields: self.json("edited_fields")?,
            notes: self.json("notes")?,
            created_at: self.time("created_at")?,
            updated_at: self.time("updated_at")?,
        })
    }
}

fn date_text(date: Option<PartialDate>) -> Option<String> {
    date.map(|d| d.to_string())
}

/// Binds the shared record columns, in [`META_COLUMNS`] order.
fn bind_meta<'q>(
    q: sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    meta: &'q RecordMeta,
) -> Result<sqlx::query::Query<'q, Sqlite, sqlx::sqlite::SqliteArguments<'q>>, StorageError> {
    let edited =
        serde_json::to_string(&meta.edited_fields).map_err(encode_error("encoding a record"))?;
    let notes = serde_json::to_string(&meta.notes).map_err(encode_error("encoding a record"))?;
    Ok(q.bind(meta.origin.as_str())
        .bind(meta.source.as_ref().map(|s| s.document.to_string()))
        .bind(meta.source.as_ref().map(|s| s.snippet.clone()))
        .bind(meta.source.as_ref().and_then(|s| s.section.clone()))
        .bind(meta.import_key.clone())
        .bind(meta.verification.as_str())
        .bind(meta.stale_since.map(encode_timestamp))
        .bind(edited)
        .bind(notes)
        .bind(encode_timestamp(meta.created_at))
        .bind(encode_timestamp(meta.updated_at)))
}

const META_COLUMNS: [&str; 11] = [
    "origin",
    "document_id",
    "source_snippet",
    "source_section",
    "import_key",
    "verification",
    "stale_since",
    "edited_fields",
    "notes",
    "created_at",
    "updated_at",
];

/// `INSERT … ON CONFLICT (id) DO UPDATE` over `columns` (the first is `id`).
fn upsert_sql(table: &str, columns: &[&str]) -> String {
    let placeholders = vec!["?"; columns.len()].join(", ");
    let updates: Vec<String> = columns
        .iter()
        .skip(1)
        .map(|c| format!("{c} = excluded.{c}"))
        .collect();
    format!(
        "INSERT INTO {table} ({}) VALUES ({placeholders}) ON CONFLICT (id) DO UPDATE SET {}",
        columns.join(", "),
        updates.join(", ")
    )
}

fn with_meta(columns: &[&'static str]) -> Vec<&'static str> {
    let mut all = columns.to_vec();
    all.extend(META_COLUMNS);
    all
}

impl SqliteJobStore {
    async fn load_in(
        conn: &mut SqliteConnection,
        id: ProfileId,
    ) -> Result<Option<ProfileData>, StorageError> {
        let pid = id.to_string();
        let Some(row) = sqlx::query("SELECT * FROM profiles WHERE id = ?")
            .bind(&pid)
            .fetch_optional(&mut *conn)
            .await
            .map_err(query_error("loading a profile"))?
        else {
            return Ok(None);
        };
        let c = Cols::new(&row, "id");
        let profile = Profile {
            id: c.parse("id")?,
            name: c.opt("name")?,
            headline: c.opt("headline")?,
            location: c.opt("location")?,
            summary: c.opt("summary")?,
            contacts: c.json::<Vec<Contact>>("contacts")?,
            languages: c.json::<Vec<SpokenLanguage>>("languages")?,
            edited_fields: c.json("edited_fields")?,
            revision: u64::try_from(c.int("revision")?)
                .map_err(|e| corrupt(&pid, e.to_string()))?,
            created_at: c.time("created_at")?,
            updated_at: c.time("updated_at")?,
        };

        let rows = |sql: &'static str| sqlx::query(sql).bind(pid.clone());
        let documents = rows(
            "SELECT * FROM profile_documents WHERE profile_id = ? ORDER BY first_imported_at, id",
        )
        .fetch_all(&mut *conn)
        .await
        .map_err(query_error("loading profile documents"))?
        .iter()
        .map(|row| {
            let c = Cols::new(row, "id");
            Ok(SourceDocument {
                id: c.parse("id")?,
                kind: c.canonical("kind", DocumentKind::from_canonical)?,
                file_name: c.opt("file_name")?,
                sha256: c.text("sha256")?,
                pages: c
                    .row
                    .try_get::<Option<i64>, _>("pages")
                    .ok()
                    .flatten()
                    .and_then(|p| u32::try_from(p).ok()),
                text: c.text("text")?,
                parser: c.text("parser")?,
                first_imported_at: c.time("first_imported_at")?,
                last_imported_at: c.time("last_imported_at")?,
            })
        })
        .collect::<Result<Vec<_>, StorageError>>()?;
        let experiences =
            rows("SELECT * FROM profile_experiences WHERE profile_id = ? ORDER BY position, id")
                .fetch_all(&mut *conn)
                .await
                .map_err(query_error("loading experiences"))?
                .iter()
                .map(|row| {
                    let c = Cols::new(row, "id");
                    Ok(Experience {
                        id: c.parse("id")?,
                        company: c.opt("company")?,
                        title: c.opt("title")?,
                        employment: c
                            .opt("employment")?
                            .as_deref()
                            .map(EmploymentKind::from_canonical),
                        start: c.date("start_date")?,
                        end: c.date("end_date")?,
                        current: c.flag("is_current")?,
                        location: c.opt("location")?,
                        summary: c.opt("summary")?,
                        position: c.u32("position")?,
                        meta: c.meta()?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
        let projects =
            rows("SELECT * FROM profile_projects WHERE profile_id = ? ORDER BY position, id")
                .fetch_all(&mut *conn)
                .await
                .map_err(query_error("loading projects"))?
                .iter()
                .map(|row| {
                    let c = Cols::new(row, "id");
                    Ok(Project {
                        id: c.parse("id")?,
                        name: c.text("name")?,
                        description: c.opt("description")?,
                        role: c.opt("role")?,
                        url: c.opt("url")?,
                        start: c.date("start_date")?,
                        end: c.date("end_date")?,
                        current: c.flag("is_current")?,
                        experience: c.opt_parse("experience_id")?,
                        position: c.u32("position")?,
                        meta: c.meta()?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
        let education =
            rows("SELECT * FROM profile_education WHERE profile_id = ? ORDER BY position, id")
                .fetch_all(&mut *conn)
                .await
                .map_err(query_error("loading education"))?
                .iter()
                .map(|row| {
                    let c = Cols::new(row, "id");
                    Ok(Education {
                        id: c.parse("id")?,
                        institution: c.text("institution")?,
                        degree: c.opt("degree")?,
                        field: c.opt("field")?,
                        start: c.date("start_date")?,
                        end: c.date("end_date")?,
                        current: c.flag("is_current")?,
                        position: c.u32("position")?,
                        meta: c.meta()?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
        let skills = rows("SELECT * FROM profile_skills WHERE profile_id = ? ORDER BY name, id")
            .fetch_all(&mut *conn)
            .await
            .map_err(query_error("loading skills"))?
            .iter()
            .map(|row| {
                let c = Cols::new(row, "id");
                Ok(Skill {
                    id: c.parse("id")?,
                    name: c.text("name")?,
                    key: c.text("key")?,
                    category: c.opt("category")?,
                    meta: c.meta()?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let claims = rows("SELECT * FROM profile_claims WHERE profile_id = ? ORDER BY subject_kind, subject_id, position, id")
            .fetch_all(&mut *conn)
            .await
            .map_err(query_error("loading claims"))?
            .iter()
            .map(decode_claim)
            .collect::<Result<Vec<_>, StorageError>>()?;
        let statements = rows("SELECT * FROM profile_preference_statements WHERE profile_id = ? ORDER BY created_at, id")
            .fetch_all(&mut *conn)
            .await
            .map_err(query_error("loading preference statements"))?
            .iter()
            .map(|row| {
                let c = Cols::new(row, "id");
                Ok(PreferenceStatement {
                    id: c.parse("id")?,
                    text: c.text("text")?,
                    parser: c.text("parser")?,
                    reading: c.canonical("reading", StatementReading::from_canonical)?,
                    unparsed: c.json("unparsed")?,
                    created_at: c.time("created_at")?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        let preferences =
            rows("SELECT * FROM profile_preferences WHERE profile_id = ? ORDER BY created_at, id")
                .fetch_all(&mut *conn)
                .await
                .map_err(query_error("loading preferences"))?
                .iter()
                .map(|row| {
                    let c = Cols::new(row, "id");
                    Ok(Preference {
                        id: c.parse("id")?,
                        value: c.json::<PreferenceValue>("value")?,
                        stance: c.canonical("stance", Stance::from_canonical)?,
                        origin: c.canonical("origin", PreferenceOrigin::from_canonical)?,
                        statement: c.opt_parse("statement_id")?,
                        snippet: c.opt("snippet")?,
                        certainty: c.canonical("certainty", Certainty::from_canonical)?,
                        active: c.flag("active")?,
                        superseded_by: c.opt_parse("superseded_by")?,
                        created_at: c.time("created_at")?,
                        updated_at: c.time("updated_at")?,
                    })
                })
                .collect::<Result<Vec<_>, StorageError>>()?;
        Ok(Some(ProfileData {
            profile,
            documents,
            experiences,
            projects,
            education,
            skills,
            claims,
            preferences,
            statements,
        }))
    }
}

fn decode_claim(row: &SqliteRow) -> Result<Claim, StorageError> {
    let c = Cols::new(row, "id");
    let subject_kind = c.text("subject_kind")?;
    let subject_id = c.opt("subject_id")?;
    let subject = Subject::from_parts(&subject_kind, subject_id.as_deref())
        .ok_or_else(|| corrupt(&c.id, format!("subject {subject_kind} {subject_id:?}")))?;
    let document = c.opt_parse("document_id")?;
    let snippet = c.opt("source_snippet")?;
    Ok(Claim {
        id: c.parse("id")?,
        kind: c.canonical("kind", ClaimKind::from_canonical)?,
        text: c.text("text")?,
        topic: c.opt("topic")?,
        subject,
        provenance: c.canonical("provenance", Provenance::from_canonical)?,
        confidence: c.canonical("confidence", Confidence::from_canonical)?,
        verification: c.canonical("verification", Verification::from_canonical)?,
        source: match (document, snippet) {
            (Some(document), Some(snippet)) => Some(SourceRef {
                document,
                snippet,
                section: c.opt("source_section")?,
            }),
            _ => None,
        },
        basis: c.opt("basis")?,
        import_key: c.opt("import_key")?,
        supersedes: c.opt_parse("supersedes")?,
        position: c.u32("position")?,
        stale_since: c.opt_time("stale_since")?,
        verified_at: c.opt_time("verified_at")?,
        note: c.opt("note")?,
        edited: c.flag("edited")?,
        created_at: c.time("created_at")?,
        updated_at: c.time("updated_at")?,
    })
}

/// Deletes the rows of `table` belonging to `profile` whose id is not in
/// `keep`.
async fn delete_missing(
    conn: &mut SqliteConnection,
    table: &'static str,
    profile: &str,
    keep: &HashSet<String>,
) -> Result<(), StorageError> {
    let stored: Vec<String> =
        sqlx::query_scalar(&format!("SELECT id FROM {table} WHERE profile_id = ?"))
            .bind(profile)
            .fetch_all(&mut *conn)
            .await
            .map_err(query_error("listing stored profile records"))?;
    let gone: Vec<&String> = stored.iter().filter(|id| !keep.contains(*id)).collect();
    for chunk in gone.chunks(BATCH) {
        let mut builder = QueryBuilder::<Sqlite>::new(format!("DELETE FROM {table} WHERE id IN ("));
        let mut ids = builder.separated(", ");
        for id in chunk {
            ids.push_bind(id.as_str());
        }
        builder.push(")");
        builder
            .build()
            .execute(&mut *conn)
            .await
            .map_err(query_error("deleting profile records"))?;
    }
    Ok(())
}

fn ids<T: ToString>(items: impl Iterator<Item = T>) -> HashSet<String> {
    items.map(|i| i.to_string()).collect()
}

async fn write_profile(
    conn: &mut SqliteConnection,
    data: &ProfileData,
) -> Result<(), StorageError> {
    let p = &data.profile;
    let pid = p.id.to_string();
    let enc = encode_error("encoding a profile");
    sqlx::query(&upsert_sql(
        "profiles",
        &[
            "id",
            "name",
            "headline",
            "location",
            "summary",
            "contacts",
            "languages",
            "edited_fields",
            "revision",
            "created_at",
            "updated_at",
        ],
    ))
    .bind(&pid)
    .bind(&p.name)
    .bind(&p.headline)
    .bind(&p.location)
    .bind(&p.summary)
    .bind(serde_json::to_string(&p.contacts).map_err(enc)?)
    .bind(serde_json::to_string(&p.languages).map_err(encode_error("encoding a profile"))?)
    .bind(serde_json::to_string(&p.edited_fields).map_err(encode_error("encoding a profile"))?)
    .bind(i64::try_from(p.revision).unwrap_or(i64::MAX))
    .bind(encode_timestamp(p.created_at))
    .bind(encode_timestamp(p.updated_at))
    .execute(&mut *conn)
    .await
    .map_err(query_error("storing a profile"))?;

    // Children first, so that nothing still points at a deleted row.
    delete_missing(
        conn,
        "profile_preferences",
        &pid,
        &ids(data.preferences.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_claims",
        &pid,
        &ids(data.claims.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_skills",
        &pid,
        &ids(data.skills.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_projects",
        &pid,
        &ids(data.projects.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_education",
        &pid,
        &ids(data.education.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_experiences",
        &pid,
        &ids(data.experiences.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_preference_statements",
        &pid,
        &ids(data.statements.iter().map(|x| x.id)),
    )
    .await?;
    delete_missing(
        conn,
        "profile_documents",
        &pid,
        &ids(data.documents.iter().map(|x| x.id)),
    )
    .await?;

    let sql = upsert_sql(
        "profile_documents",
        &[
            "id",
            "profile_id",
            "kind",
            "file_name",
            "sha256",
            "pages",
            "text",
            "parser",
            "first_imported_at",
            "last_imported_at",
        ],
    );
    for d in &data.documents {
        sqlx::query(&sql)
            .bind(d.id.to_string())
            .bind(&pid)
            .bind(d.kind.as_str())
            .bind(&d.file_name)
            .bind(&d.sha256)
            .bind(d.pages.map(i64::from))
            .bind(&d.text)
            .bind(&d.parser)
            .bind(encode_timestamp(d.first_imported_at))
            .bind(encode_timestamp(d.last_imported_at))
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a profile document"))?;
    }

    // Import keys are unique per profile. Clearing the keys of rows about
    // to be rewritten lets two records swap keys within one save.
    for table in [
        "profile_experiences",
        "profile_projects",
        "profile_education",
        "profile_claims",
    ] {
        sqlx::query(&format!(
            "UPDATE {table} SET import_key = NULL WHERE profile_id = ?"
        ))
        .bind(&pid)
        .execute(&mut *conn)
        .await
        .map_err(query_error("preparing profile records"))?;
    }

    let sql = upsert_sql(
        "profile_experiences",
        &with_meta(&[
            "id",
            "profile_id",
            "position",
            "company",
            "title",
            "employment",
            "start_date",
            "end_date",
            "is_current",
            "location",
            "summary",
        ]),
    );
    for e in &data.experiences {
        let q = sqlx::query(&sql)
            .bind(e.id.to_string())
            .bind(&pid)
            .bind(i64::from(e.position))
            .bind(&e.company)
            .bind(&e.title)
            .bind(e.employment.as_ref().map(|k| k.as_str().to_owned()))
            .bind(date_text(e.start))
            .bind(date_text(e.end))
            .bind(e.current)
            .bind(&e.location)
            .bind(&e.summary);
        bind_meta(q, &e.meta)?
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing an experience"))?;
    }

    let sql = upsert_sql(
        "profile_projects",
        &with_meta(&[
            "id",
            "profile_id",
            "position",
            "name",
            "description",
            "role",
            "url",
            "start_date",
            "end_date",
            "is_current",
            "experience_id",
        ]),
    );
    for x in &data.projects {
        let q = sqlx::query(&sql)
            .bind(x.id.to_string())
            .bind(&pid)
            .bind(i64::from(x.position))
            .bind(&x.name)
            .bind(&x.description)
            .bind(&x.role)
            .bind(&x.url)
            .bind(date_text(x.start))
            .bind(date_text(x.end))
            .bind(x.current)
            .bind(x.experience.map(|e| e.to_string()));
        bind_meta(q, &x.meta)?
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a project"))?;
    }

    let sql = upsert_sql(
        "profile_education",
        &with_meta(&[
            "id",
            "profile_id",
            "position",
            "institution",
            "degree",
            "field",
            "start_date",
            "end_date",
            "is_current",
        ]),
    );
    for x in &data.education {
        let q = sqlx::query(&sql)
            .bind(x.id.to_string())
            .bind(&pid)
            .bind(i64::from(x.position))
            .bind(&x.institution)
            .bind(&x.degree)
            .bind(&x.field)
            .bind(date_text(x.start))
            .bind(date_text(x.end))
            .bind(x.current);
        bind_meta(q, &x.meta)?
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing an education entry"))?;
    }

    let sql = upsert_sql(
        "profile_skills",
        &with_meta(&["id", "profile_id", "name", "key", "category"]),
    );
    for x in &data.skills {
        let q = sqlx::query(&sql)
            .bind(x.id.to_string())
            .bind(&pid)
            .bind(&x.name)
            .bind(&x.key)
            .bind(&x.category);
        bind_meta(q, &x.meta)?
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a skill"))?;
    }

    let sql = upsert_sql(
        "profile_preference_statements",
        &[
            "id",
            "profile_id",
            "text",
            "parser",
            "reading",
            "unparsed",
            "created_at",
        ],
    );
    for s in &data.statements {
        sqlx::query(&sql)
            .bind(s.id.to_string())
            .bind(&pid)
            .bind(&s.text)
            .bind(&s.parser)
            .bind(s.reading.as_str())
            .bind(serde_json::to_string(&s.unparsed).map_err(encode_error("encoding a statement"))?)
            .bind(encode_timestamp(s.created_at))
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a preference statement"))?;
    }

    let sql = upsert_sql(
        "profile_claims",
        &[
            "id",
            "profile_id",
            "kind",
            "text",
            "topic",
            "subject_kind",
            "subject_id",
            "provenance",
            "confidence",
            "verification",
            "document_id",
            "source_snippet",
            "source_section",
            "basis",
            "import_key",
            "supersedes",
            "position",
            "stale_since",
            "verified_at",
            "note",
            "edited",
            "created_at",
            "updated_at",
        ],
    );
    for c in &data.claims {
        sqlx::query(&sql)
            .bind(c.id.to_string())
            .bind(&pid)
            .bind(c.kind.as_str())
            .bind(&c.text)
            .bind(&c.topic)
            .bind(c.subject.kind_str())
            .bind(c.subject.id_string())
            .bind(c.provenance.as_str())
            .bind(c.confidence.as_str())
            .bind(c.verification.as_str())
            .bind(c.source.as_ref().map(|s| s.document.to_string()))
            .bind(c.source.as_ref().map(|s| s.snippet.clone()))
            .bind(c.source.as_ref().and_then(|s| s.section.clone()))
            .bind(&c.basis)
            .bind(&c.import_key)
            .bind(c.supersedes.map(|s| s.to_string()))
            .bind(i64::from(c.position))
            .bind(c.stale_since.map(encode_timestamp))
            .bind(c.verified_at.map(encode_timestamp))
            .bind(&c.note)
            .bind(c.edited)
            .bind(encode_timestamp(c.created_at))
            .bind(encode_timestamp(c.updated_at))
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a claim"))?;
    }

    let sql = upsert_sql(
        "profile_preferences",
        &[
            "id",
            "profile_id",
            "category",
            "key",
            "value",
            "stance",
            "origin",
            "statement_id",
            "snippet",
            "certainty",
            "active",
            "superseded_by",
            "created_at",
            "updated_at",
        ],
    );
    for x in &data.preferences {
        sqlx::query(&sql)
            .bind(x.id.to_string())
            .bind(&pid)
            .bind(x.value.category().as_str())
            .bind(x.value.key())
            .bind(serde_json::to_string(&x.value).map_err(encode_error("encoding a preference"))?)
            .bind(x.stance.as_str())
            .bind(x.origin.as_str())
            .bind(x.statement.map(|s| s.to_string()))
            .bind(&x.snippet)
            .bind(x.certainty.as_str())
            .bind(x.active)
            .bind(x.superseded_by.map(|s| s.to_string()))
            .bind(encode_timestamp(x.created_at))
            .bind(encode_timestamp(x.updated_at))
            .execute(&mut *conn)
            .await
            .map_err(query_error("storing a preference"))?;
    }

    // Derived links: every technology or skill claim backs the skill with
    // its topic.
    sqlx::query(
        "DELETE FROM profile_skill_evidence WHERE skill_id IN \
         (SELECT id FROM profile_skills WHERE profile_id = ?)",
    )
    .bind(&pid)
    .execute(&mut *conn)
    .await
    .map_err(query_error("clearing skill evidence"))?;
    for skill in &data.skills {
        for claim in data.skill_claims(skill) {
            sqlx::query("INSERT INTO profile_skill_evidence (skill_id, claim_id) VALUES (?, ?)")
                .bind(skill.id.to_string())
                .bind(claim.id.to_string())
                .execute(&mut *conn)
                .await
                .map_err(query_error("storing skill evidence"))?;
        }
    }
    Ok(())
}

#[async_trait]
impl ProfileRepository for SqliteJobStore {
    async fn load_profile(&self, id: ProfileId) -> Result<Option<ProfileData>, StorageError> {
        // One transaction, so the aggregate is a consistent snapshot.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        let data = Self::load_in(&mut tx, id).await?;
        tx.commit().await.map_err(query_error("finishing a read"))?;
        Ok(data)
    }

    async fn save_profile(
        &self,
        data: &ProfileData,
        expected_revision: u64,
        events: &[ProfileEvent],
    ) -> Result<(), StorageError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(query_error("starting a transaction"))?;
        let pid = data.id().to_string();
        let stored: Option<i64> = sqlx::query_scalar("SELECT revision FROM profiles WHERE id = ?")
            .bind(&pid)
            .fetch_optional(&mut *tx)
            .await
            .map_err(query_error("checking the profile revision"))?;
        let found = stored.map_or(0, |r| u64::try_from(r).unwrap_or(0));
        if found != expected_revision {
            return Err(StorageError::Conflict {
                expected: expected_revision,
                found,
            });
        }
        write_profile(&mut tx, data).await?;
        for event in events {
            sqlx::query(
                "INSERT INTO profile_events (profile_id, at, kind, record, detail) \
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(&pid)
            .bind(encode_timestamp(event.at))
            .bind(event.kind.as_str())
            .bind(&event.record)
            .bind(&event.detail)
            .execute(&mut *tx)
            .await
            .map_err(query_error("recording profile history"))?;
        }
        tx.commit()
            .await
            .map_err(query_error("committing a profile"))?;
        Ok(())
    }

    async fn find_claims(
        &self,
        profile: ProfileId,
        query: &ClaimQuery,
    ) -> Result<Vec<Claim>, StorageError> {
        let pid = profile.to_string();
        let mut b = QueryBuilder::<Sqlite>::new("SELECT * FROM profile_claims WHERE profile_id = ");
        b.push_bind(pid.clone());
        if !query.kinds.is_empty() {
            b.push(" AND kind IN (");
            let mut kinds = b.separated(", ");
            for kind in &query.kinds {
                kinds.push_bind(kind.as_str());
            }
            b.push(")");
        }
        if let Some(subject) = query.subject {
            b.push(" AND subject_kind = ").push_bind(subject.kind_str());
            match subject.id_string() {
                Some(id) => b.push(" AND subject_id = ").push_bind(id),
                None => b.push(" AND subject_id IS NULL"),
            };
        }
        if let Some(provenance) = query.provenance {
            b.push(" AND provenance = ").push_bind(provenance.as_str());
        }
        if let Some(verification) = query.verification {
            b.push(" AND verification = ")
                .push_bind(verification.as_str());
        }
        if let Some(topic) = &query.topic {
            b.push(" AND topic = ").push_bind(topic.clone());
        }
        if query.exclude_stale {
            b.push(" AND stale_since IS NULL");
        }
        b.push(" ORDER BY subject_kind, subject_id, kind, position, id");
        let rows = b
            .build()
            .fetch_all(&self.pool)
            .await
            .map_err(query_error("finding claims"))?;
        let claims: Vec<Claim> = rows.iter().map(decode_claim).collect::<Result<_, _>>()?;
        if !query.needs_review && !query.usable_only {
            return Ok(claims);
        }
        // The evidence policy lives in the domain; a claim about a rejected
        // record counts as rejected.
        let rejected: HashSet<String> = sqlx::query_scalar(
            "SELECT id FROM profile_experiences WHERE profile_id = ?1 AND verification = 'rejected' \
             UNION SELECT id FROM profile_projects WHERE profile_id = ?1 AND verification = 'rejected' \
             UNION SELECT id FROM profile_education WHERE profile_id = ?1 AND verification = 'rejected'",
        )
        .bind(&pid)
        .fetch_all(&self.pool)
        .await
        .map_err(query_error("finding rejected records"))?
        .into_iter()
        .collect();
        Ok(claims
            .into_iter()
            .filter(|c| {
                let subject_rejected = c
                    .subject
                    .id_string()
                    .is_some_and(|id| rejected.contains(&id));
                let standing = if subject_rejected {
                    jobhunt_profile::Standing::Rejected
                } else {
                    c.standing()
                };
                (!query.needs_review || standing.needs_review())
                    && (!query.usable_only || standing.is_usable())
            })
            .collect())
    }

    async fn profile_events(
        &self,
        profile: ProfileId,
        limit: usize,
    ) -> Result<Vec<ProfileEvent>, StorageError> {
        let rows = sqlx::query(
            "SELECT id, at, kind, record, detail FROM profile_events WHERE profile_id = ? \
             ORDER BY id DESC LIMIT ?",
        )
        .bind(profile.to_string())
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .fetch_all(&self.pool)
        .await
        .map_err(query_error("loading profile history"))?;
        rows.iter()
            .map(|row| {
                let id: i64 = row.try_get("id").unwrap_or_default();
                let c = Cols {
                    row,
                    id: format!("profile event {id}"),
                };
                Ok(ProfileEvent {
                    at: c.time("at")?,
                    kind: c.canonical("kind", ProfileEventKind::from_canonical)?,
                    record: c.opt("record")?,
                    detail: c.text("detail")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use chrono::TimeZone;
    use jobhunt_jobs::JobRepository;
    use jobhunt_profile::{
        ClaimQuery, DocumentKind, ParsedExperience, ParsedResume, ParsedSkillLine, ProfileService,
        RuleParser, merge_resume,
    };
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
    }

    /// 64 hex characters derived from the text, standing in for a SHA-256.
    fn hex_digest(text: &str) -> String {
        let id = jobhunt_core::StableId::derive("test.sha", &[text]).to_hex();
        format!("{id}{id}")
    }

    fn document(text: &str) -> SourceDocument {
        SourceDocument {
            id: jobhunt_profile::DocumentId::derive(&["x"]),
            kind: DocumentKind::Text,
            file_name: Some("resume.txt".into()),
            sha256: hex_digest(text),
            pages: Some(1),
            text: text.into(),
            parser: "test".into(),
            first_imported_at: at(0),
            last_imported_at: at(0),
        }
    }

    fn parsed() -> ParsedResume {
        ParsedResume {
            experiences: vec![ParsedExperience {
                company: Some("Ledgerly".into()),
                title: Some("Senior Backend Engineer".into()),
                start: Some("2022-03".parse().unwrap()),
                current: true,
                header: "Ledgerly — Senior Backend Engineer, Mar 2022 – Present".into(),
                bullets: vec![
                    "Integrated four payment providers behind one settlement API.".into(),
                    "Mentored three engineers.".into(),
                ],
                tech_line: Some("Tech: Rust, PostgreSQL".into()),
                technologies: vec!["Rust".into(), "PostgreSQL".into()],
                ..ParsedExperience::default()
            }],
            skills: vec![ParsedSkillLine {
                category: Some("Languages".into()),
                skills: vec!["Rust".into(), "Go".into()],
                line: "Languages: Rust, Go".into(),
            }],
            ..ParsedResume::default()
        }
    }

    async fn imported(store: &SqliteJobStore) -> ProfileData {
        let service = ProfileService::new(store);
        service
            .import_resume(document("resume v1"), &parsed(), at(1))
            .await
            .unwrap();
        service
            .add_statement(
                "I want backend roles and at least €90k. No agencies.",
                &RuleParser,
                at(2),
            )
            .await
            .unwrap();
        service.require().await.unwrap()
    }

    #[tokio::test]
    async fn profiles_round_trip_exactly() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let data = imported(&store).await;
        assert!(!data.claims.is_empty());
        assert_eq!(data.preferences.len(), 3);
        let loaded = store.load_profile(data.id()).await.unwrap().unwrap();
        assert_eq!(loaded, data, "every field survives storage");
        assert_eq!(loaded.profile.revision, 2);
        assert_eq!(
            store
                .load_profile(ProfileId::derive(&["other"]))
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn stale_writers_are_rejected() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let data = imported(&store).await;
        let mut stale = data.clone();
        stale.profile.revision += 1;
        let err = store
            .save_profile(&stale, data.profile.revision - 1, &[])
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                StorageError::Conflict {
                    expected: 1,
                    found: 2
                }
            ),
            "{err}"
        );
        assert_eq!(store.load_profile(data.id()).await.unwrap().unwrap(), data);
    }

    #[tokio::test]
    async fn records_missing_from_the_aggregate_are_deleted() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let mut data = imported(&store).await;
        let removed = data.claims.pop().unwrap();
        data.statements.clear();
        data.preferences.clear();
        data.profile.revision += 1;
        store
            .save_profile(&data, data.profile.revision - 1, &[])
            .await
            .unwrap();
        let loaded = store.load_profile(data.id()).await.unwrap().unwrap();
        assert!(loaded.claim(removed.id).is_none());
        assert!(loaded.preferences.is_empty() && loaded.statements.is_empty());
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn skill_evidence_links_follow_claims() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let data = imported(&store).await;
        let rust = data.skills.iter().find(|s| s.key == "rust").unwrap();
        let links: Vec<String> = sqlx::query_scalar(
            "SELECT c.kind FROM profile_skill_evidence e JOIN profile_claims c ON c.id = e.claim_id \
             WHERE e.skill_id = ? ORDER BY c.kind",
        )
        .bind(rust.id.to_string())
        .fetch_all(&store.pool)
        .await
        .unwrap();
        assert_eq!(links, ["skill", "technology"], "listed and used");
    }

    #[tokio::test]
    async fn claim_queries_filter_in_sql_and_by_policy() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let data = imported(&store).await;
        let pid = data.id();
        let experience = data.experiences[0].id;

        let technologies = store
            .find_claims(
                pid,
                &ClaimQuery {
                    kinds: vec![ClaimKind::Technology],
                    subject: Some(Subject::Experience(experience)),
                    ..ClaimQuery::default()
                },
            )
            .await
            .unwrap();
        let topics: Vec<&str> = technologies
            .iter()
            .filter_map(|c| c.topic.as_deref())
            .collect();
        assert_eq!(topics, ["rust", "postgresql"]);

        let payments = store
            .find_claims(
                pid,
                &ClaimQuery {
                    kinds: vec![ClaimKind::Domain],
                    topic: Some("payments".into()),
                    ..ClaimQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(payments.len(), 1);
        assert_eq!(payments[0].provenance, Provenance::Inferred);

        let review = store
            .find_claims(
                pid,
                &ClaimQuery {
                    needs_review: true,
                    ..ClaimQuery::default()
                },
            )
            .await
            .unwrap();
        assert!(review.iter().all(|c| c.provenance == Provenance::Inferred));
        assert_eq!(review.len(), data.review_queue().len());

        // Confirming moves a claim out of the review queue; rejecting the
        // experience excludes all of its claims from use.
        let service = ProfileService::new(&store);
        service
            .decide_claims(
                &[payments[0].id.to_string()],
                Verification::Confirmed,
                None,
                at(3),
            )
            .await
            .unwrap();
        let confirmed = store
            .find_claims(
                pid,
                &ClaimQuery {
                    verification: Some(Verification::Confirmed),
                    ..ClaimQuery::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(confirmed.len(), 1);
        service
            .remove(&experience.to_string(), at(4))
            .await
            .unwrap();
        let usable = store
            .find_claims(
                pid,
                &ClaimQuery {
                    usable_only: true,
                    ..ClaimQuery::default()
                },
            )
            .await
            .unwrap();
        assert!(usable.iter().all(|c| c.subject == Subject::Profile));
    }

    #[tokio::test]
    async fn history_is_recorded_newest_first() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let data = imported(&store).await;
        let events = store.profile_events(data.id(), 10).await.unwrap();
        let kinds: Vec<ProfileEventKind> = events.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            [
                ProfileEventKind::StatementAdded,
                ProfileEventKind::ResumeImported
            ]
        );
        assert_eq!(store.profile_events(data.id(), 1).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn merge_results_store_without_constraint_violations_on_reimport() {
        let store = SqliteJobStore::open_in_memory().await.unwrap();
        let mut data = imported(&store).await;
        // Re-import the same resume through the pure merge and save.
        let expected = data.profile.revision;
        let _ = merge_resume(&mut data, document("resume v1"), &parsed(), at(5));
        data.profile.revision += 1;
        store.save_profile(&data, expected, &[]).await.unwrap();
        let loaded = store.load_profile(data.id()).await.unwrap().unwrap();
        assert_eq!(loaded.experiences.len(), 1);
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn upgrading_a_jobs_database_keeps_jobs_and_adds_profiles() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jobhunt.db");
        {
            let pool = SqlitePool::connect_with(
                SqliteConnectOptions::new()
                    .filename(&path)
                    .create_if_missing(true),
            )
            .await
            .unwrap();
            let before_profiles = sqlx::migrate::Migrator {
                migrations: Cow::Owned(crate::sqlite::MIGRATOR.iter().take(2).cloned().collect()),
                ..sqlx::migrate::Migrator::DEFAULT
            };
            before_profiles.run(&pool).await.unwrap();
            sqlx::query(
                "INSERT INTO discovery_runs (started_at) VALUES ('2026-09-01T00:00:00.000000Z')",
            )
            .execute(&pool)
            .await
            .unwrap();
            pool.close().await;
        }
        let store = SqliteJobStore::open(&path).await.unwrap();
        let run = store.begin_run(at(0)).await.unwrap();
        assert_eq!(run.0, 2, "existing discovery data is kept");
        let data = imported(&store).await;
        assert_eq!(store.load_profile(data.id()).await.unwrap().unwrap(), data);
    }
}
