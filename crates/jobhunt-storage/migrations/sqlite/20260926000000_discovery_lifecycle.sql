-- Discovery lifecycle, history, scan bookkeeping and cross-source identity.
--
-- Additive: rows written before this migration become open jobs, each in its
-- own opportunity, with a `new` history event at their first_seen_at. They
-- have no material fingerprint yet; the first scan that sees them again
-- records one without reporting them as UPDATED.

-- Lifecycle: a job is open while its source lists it, closed once a complete
-- listing no longer does. Closed jobs are kept (with history) and reopen if
-- they reappear.
ALTER TABLE jobs ADD COLUMN status TEXT NOT NULL DEFAULT 'open'
    CHECK (status IN ('open', 'closed'));
ALTER TABLE jobs ADD COLUMN closed_at TEXT;

-- Digest of the material content (JobSnapshot); `fingerprint` covers the
-- whole stored row. NULL for legacy rows.
ALTER TABLE jobs ADD COLUMN content_fingerprint TEXT;

-- The logical opportunity (opp_<hex>) this source record belongs to. A
-- record alone in its opportunity uses its own job id's hex.
ALTER TABLE jobs ADD COLUMN opportunity_id TEXT NOT NULL DEFAULT '';
UPDATE jobs SET opportunity_id = 'opp_' || substr(id, 5);

CREATE INDEX jobs_source_status ON jobs (source_kind, source_instance, status);
CREATE INDEX jobs_opportunity ON jobs (opportunity_id);

-- One row per `find` (or other) discovery run.
CREATE TABLE discovery_runs (
    id                          INTEGER PRIMARY KEY,
    started_at                  TEXT NOT NULL,
    finished_at                 TEXT,
    sources                     INTEGER NOT NULL DEFAULT 0,
    failed                      INTEGER NOT NULL DEFAULT 0,
    received                    INTEGER NOT NULL DEFAULT 0,
    normalized                  INTEGER NOT NULL DEFAULT 0,
    rejected                    INTEGER NOT NULL DEFAULT 0,
    new                         INTEGER NOT NULL DEFAULT 0,
    updated                     INTEGER NOT NULL DEFAULT 0,
    unchanged                   INTEGER NOT NULL DEFAULT 0,
    reopened                    INTEGER NOT NULL DEFAULT 0,
    closed                      INTEGER NOT NULL DEFAULT 0,
    multi_source_opportunities  INTEGER NOT NULL DEFAULT 0
) STRICT;

-- One row per source per run. `complete` says the adapter vouched for a
-- full listing; `closing_applied` says missing jobs were actually closed
-- (a complete listing can still have closing withheld by a safeguard,
-- recorded in `closing_withheld`). `validator` (an ETag) is kept only for
-- listings that were fully applied, and `revision` is the canonical
-- conversion revision it was recorded under.
CREATE TABLE source_scans (
    id                INTEGER PRIMARY KEY,
    run_id            INTEGER NOT NULL REFERENCES discovery_runs (id),
    source_kind       TEXT NOT NULL,
    source_instance   TEXT NOT NULL,
    status            TEXT NOT NULL CHECK (status IN ('listing', 'not_modified', 'failed')),
    complete          INTEGER NOT NULL CHECK (complete IN (0, 1)),
    closing_applied   INTEGER NOT NULL CHECK (closing_applied IN (0, 1)),
    closing_withheld  TEXT,
    started_at        TEXT NOT NULL,
    finished_at       TEXT NOT NULL,
    received          INTEGER NOT NULL DEFAULT 0,
    normalized        INTEGER NOT NULL DEFAULT 0,
    rejected          INTEGER NOT NULL DEFAULT 0,
    skipped           INTEGER NOT NULL DEFAULT 0,
    duplicates        INTEGER NOT NULL DEFAULT 0,
    new               INTEGER NOT NULL DEFAULT 0,
    updated           INTEGER NOT NULL DEFAULT 0,
    unchanged         INTEGER NOT NULL DEFAULT 0,
    reopened          INTEGER NOT NULL DEFAULT 0,
    closed            INTEGER NOT NULL DEFAULT 0,
    validator         TEXT,
    revision          TEXT NOT NULL,
    error             TEXT
) STRICT;

CREATE INDEX source_scans_by_source
    ON source_scans (source_kind, source_instance, status, id);

-- Job history: one row per change, never per unchanged observation.
-- `previous` is the JSON JobSnapshot a content change replaced, so the
-- current row plus the chain of `previous` snapshots reconstructs every
-- version. `changed_fields` is a JSON array of material field names.
CREATE TABLE job_events (
    id              INTEGER PRIMARY KEY,
    job_id          TEXT NOT NULL REFERENCES jobs (id),
    run_id          INTEGER REFERENCES discovery_runs (id),
    kind            TEXT NOT NULL CHECK (kind IN ('new', 'updated', 'closed', 'reopened')),
    at              TEXT NOT NULL,
    changed_fields  TEXT NOT NULL DEFAULT '[]',
    previous        TEXT
) STRICT;

CREATE INDEX job_events_by_job ON job_events (job_id, id);

INSERT INTO job_events (job_id, kind, at)
    SELECT id, 'new', first_seen_at FROM jobs ORDER BY first_seen_at, id;

-- Identity evidence used for cross-source grouping (see
-- jobhunt_jobs::identity): `url:<canonical url>` and `ats:<system>:<id>`.
CREATE TABLE job_evidence (
    job_id  TEXT NOT NULL REFERENCES jobs (id),
    key     TEXT NOT NULL,
    PRIMARY KEY (job_id, key)
) STRICT, WITHOUT ROWID;

CREATE INDEX job_evidence_by_key ON job_evidence (key);
