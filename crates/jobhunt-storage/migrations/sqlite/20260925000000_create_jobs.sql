-- Canonical jobs discovered from sources.
--
-- Timestamps are stored as fixed-width UTC RFC 3339 text
-- (YYYY-MM-DDTHH:MM:SS.ffffffZ) so they sort correctly as strings.
-- JSON columns hold small structured values (locations, compensation) that
-- the application reads and writes as a whole.
CREATE TABLE jobs (
    -- Stable internal id: job_<hex>, derived from source + source job id.
    id                  TEXT PRIMARY KEY NOT NULL,

    -- Provenance
    source_kind         TEXT NOT NULL,
    source_instance     TEXT NOT NULL,
    source_job_id       TEXT,
    fetched_from        TEXT,

    -- Canonical content
    url                 TEXT NOT NULL,
    apply_url           TEXT,
    company             TEXT NOT NULL,
    title               TEXT NOT NULL,
    department          TEXT,
    team                TEXT,
    location            TEXT,
    locations           TEXT NOT NULL DEFAULT '[]',   -- JSON array
    employment_type     TEXT,
    workplace_type      TEXT,
    is_remote           INTEGER CHECK (is_remote IN (0, 1)),
    compensation        TEXT,                         -- JSON object
    description_text    TEXT,
    description_html    TEXT,
    posted_at           TEXT,
    source_updated_at   TEXT,

    -- Derived: normalized words for search (see JobPosting::search_document)
    search_text         TEXT NOT NULL,

    -- Change detection and bookkeeping
    fingerprint         TEXT NOT NULL,
    first_seen_at       TEXT NOT NULL,
    last_seen_at        TEXT NOT NULL,
    content_updated_at  TEXT NOT NULL
) STRICT;

-- A source never yields two jobs with the same source-native id.
CREATE UNIQUE INDEX jobs_source_identity
    ON jobs (source_kind, source_instance, source_job_id)
    WHERE source_job_id IS NOT NULL;

CREATE INDEX jobs_url ON jobs (url);
CREATE INDEX jobs_last_seen_at ON jobs (last_seen_at);
CREATE INDEX jobs_posted_at ON jobs (posted_at);
