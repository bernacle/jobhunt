-- Career profile, evidence graph and preferences (BRU-289).
--
-- Additive: only new tables; the jobs tables are untouched. Every table is
-- keyed by profile_id so several profiles can share one database later; a
-- local install uses one. Records are relational rows; JSON appears only
-- for small structured values the application reads and writes whole
-- (contact lists, a preference's typed value, lists of field names).
-- Timestamps use the same fixed-width UTC text as the jobs tables, and
-- partial dates are 'YYYY' or 'YYYY-MM' text; both map directly to
-- Postgres (timestamptz, text/jsonb).

CREATE TABLE profiles (
    id              TEXT PRIMARY KEY NOT NULL,        -- prof_<hex>
    name            TEXT,
    headline        TEXT,
    location        TEXT,
    summary         TEXT,
    contacts        TEXT NOT NULL DEFAULT '[]',       -- JSON [{kind, value}]
    languages       TEXT NOT NULL DEFAULT '[]',       -- JSON [{name, level}]
    edited_fields   TEXT NOT NULL DEFAULT '[]',       -- JSON: basics set by hand
    revision        INTEGER NOT NULL CHECK (revision >= 0),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;

-- Imported resumes, with the text the parser read (snippets point here).
CREATE TABLE profile_documents (
    id                  TEXT PRIMARY KEY NOT NULL,    -- doc_<hex>
    profile_id          TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind                TEXT NOT NULL CHECK (kind IN ('pdf', 'text', 'markdown')),
    file_name           TEXT,
    sha256              TEXT NOT NULL,
    pages               INTEGER,
    text                TEXT NOT NULL,
    parser              TEXT NOT NULL,
    first_imported_at   TEXT NOT NULL,
    last_imported_at    TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX profile_documents_file ON profile_documents (profile_id, sha256);

-- Record bookkeeping shared by experiences, projects, education and skills:
--   origin          'resume' | 'user'
--   document_id,
--   source_snippet,
--   source_section  where it was read (resume records)
--   import_key      identity across resume re-imports
--   verification    'unverified' | 'confirmed' | 'rejected'
--   stale_since     set when the latest resume no longer contains it
--   edited_fields   JSON: fields the user set (re-imports keep them)
--   notes           JSON: what the parser was unsure about

CREATE TABLE profile_experiences (
    id              TEXT PRIMARY KEY NOT NULL,        -- exp_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    company         TEXT,
    title           TEXT,
    employment      TEXT,
    start_date      TEXT,
    end_date        TEXT,
    is_current      INTEGER NOT NULL CHECK (is_current IN (0, 1)),
    location        TEXT,
    summary         TEXT,
    origin          TEXT NOT NULL CHECK (origin IN ('resume', 'user')),
    document_id     TEXT REFERENCES profile_documents (id) ON DELETE SET NULL,
    source_snippet  TEXT,
    source_section  TEXT,
    import_key      TEXT,
    verification    TEXT NOT NULL CHECK (verification IN ('unverified', 'confirmed', 'rejected')),
    stale_since     TEXT,
    edited_fields   TEXT NOT NULL DEFAULT '[]',
    notes           TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
CREATE INDEX profile_experiences_profile ON profile_experiences (profile_id);
CREATE UNIQUE INDEX profile_experiences_import
    ON profile_experiences (profile_id, import_key) WHERE import_key IS NOT NULL;

CREATE TABLE profile_projects (
    id              TEXT PRIMARY KEY NOT NULL,        -- proj_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    role            TEXT,
    url             TEXT,
    start_date      TEXT,
    end_date        TEXT,
    is_current      INTEGER NOT NULL CHECK (is_current IN (0, 1)),
    experience_id   TEXT REFERENCES profile_experiences (id) ON DELETE SET NULL,
    origin          TEXT NOT NULL CHECK (origin IN ('resume', 'user')),
    document_id     TEXT REFERENCES profile_documents (id) ON DELETE SET NULL,
    source_snippet  TEXT,
    source_section  TEXT,
    import_key      TEXT,
    verification    TEXT NOT NULL CHECK (verification IN ('unverified', 'confirmed', 'rejected')),
    stale_since     TEXT,
    edited_fields   TEXT NOT NULL DEFAULT '[]',
    notes           TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
CREATE INDEX profile_projects_profile ON profile_projects (profile_id);
CREATE UNIQUE INDEX profile_projects_import
    ON profile_projects (profile_id, import_key) WHERE import_key IS NOT NULL;

CREATE TABLE profile_education (
    id              TEXT PRIMARY KEY NOT NULL,        -- edu_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    institution     TEXT NOT NULL,
    degree          TEXT,
    field           TEXT,
    start_date      TEXT,
    end_date        TEXT,
    is_current      INTEGER NOT NULL CHECK (is_current IN (0, 1)),
    origin          TEXT NOT NULL CHECK (origin IN ('resume', 'user')),
    document_id     TEXT REFERENCES profile_documents (id) ON DELETE SET NULL,
    source_snippet  TEXT,
    source_section  TEXT,
    import_key      TEXT,
    verification    TEXT NOT NULL CHECK (verification IN ('unverified', 'confirmed', 'rejected')),
    stale_since     TEXT,
    edited_fields   TEXT NOT NULL DEFAULT '[]',
    notes           TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
CREATE INDEX profile_education_profile ON profile_education (profile_id);
CREATE UNIQUE INDEX profile_education_import
    ON profile_education (profile_id, import_key) WHERE import_key IS NOT NULL;

CREATE TABLE profile_skills (
    id              TEXT PRIMARY KEY NOT NULL,        -- skill_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    name            TEXT NOT NULL,
    key             TEXT NOT NULL,                    -- normalized name
    category        TEXT,
    origin          TEXT NOT NULL CHECK (origin IN ('resume', 'user')),
    document_id     TEXT REFERENCES profile_documents (id) ON DELETE SET NULL,
    source_snippet  TEXT,
    source_section  TEXT,
    import_key      TEXT,
    verification    TEXT NOT NULL CHECK (verification IN ('unverified', 'confirmed', 'rejected')),
    stale_since     TEXT,
    edited_fields   TEXT NOT NULL DEFAULT '[]',
    notes           TEXT NOT NULL DEFAULT '[]',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
CREATE UNIQUE INDEX profile_skills_key ON profile_skills (profile_id, key);

-- The evidence graph: one row per professional claim.
CREATE TABLE profile_claims (
    id              TEXT PRIMARY KEY NOT NULL,        -- clm_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    kind            TEXT NOT NULL CHECK (kind IN (
                        'employment', 'responsibility', 'accomplishment', 'technology',
                        'skill', 'domain', 'role', 'ownership', 'education', 'project',
                        'other')),
    text            TEXT NOT NULL,
    topic           TEXT,                             -- normalized technology/domain/role
    subject_kind    TEXT NOT NULL CHECK (subject_kind IN (
                        'profile', 'experience', 'project', 'education')),
    subject_id      TEXT,                             -- exp_/proj_/edu_ id; NULL for profile
    provenance      TEXT NOT NULL CHECK (provenance IN ('extracted', 'inferred', 'user_entered')),
    confidence      TEXT NOT NULL CHECK (confidence IN ('low', 'medium', 'high')),
    verification    TEXT NOT NULL CHECK (verification IN ('unverified', 'confirmed', 'rejected')),
    document_id     TEXT REFERENCES profile_documents (id) ON DELETE SET NULL,
    source_snippet  TEXT,                             -- the document's own words
    source_section  TEXT,
    basis           TEXT,                             -- why an inference was made
    import_key      TEXT,
    supersedes      TEXT,                             -- clm_ id of the claim it replaced
    position        INTEGER NOT NULL,
    stale_since     TEXT,
    verified_at     TEXT,
    note            TEXT,
    edited          INTEGER NOT NULL CHECK (edited IN (0, 1)),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    CHECK ((subject_kind = 'profile') = (subject_id IS NULL))
) STRICT;
CREATE INDEX profile_claims_subject ON profile_claims (profile_id, subject_kind, subject_id);
CREATE INDEX profile_claims_topic ON profile_claims (profile_id, kind, topic);
CREATE INDEX profile_claims_verification ON profile_claims (profile_id, verification);
CREATE UNIQUE INDEX profile_claims_import
    ON profile_claims (profile_id, import_key) WHERE import_key IS NOT NULL;

-- Which claims back which skill (derived from claim topics on every save,
-- kept as a table so "technologies with evidence" is one join).
CREATE TABLE profile_skill_evidence (
    skill_id        TEXT NOT NULL REFERENCES profile_skills (id) ON DELETE CASCADE,
    claim_id        TEXT NOT NULL REFERENCES profile_claims (id) ON DELETE CASCADE,
    PRIMARY KEY (skill_id, claim_id)
) STRICT;
CREATE INDEX profile_skill_evidence_claim ON profile_skill_evidence (claim_id);

-- Preference statements in the user's own words. Never rewritten.
CREATE TABLE profile_preference_statements (
    id              TEXT PRIMARY KEY NOT NULL,        -- stmt_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    text            TEXT NOT NULL,
    parser          TEXT NOT NULL,
    reading         TEXT NOT NULL CHECK (reading IN ('understood', 'partial', 'not_understood')),
    unparsed        TEXT NOT NULL DEFAULT '[]',       -- JSON: parts not understood
    created_at      TEXT NOT NULL
) STRICT;
CREATE INDEX profile_statements_profile ON profile_preference_statements (profile_id);

-- Structured preferences. `value` is the typed value as JSON
-- ({"type": "compensation", "amount": 120000, ...}); `category` and `key`
-- are derived from it for querying.
CREATE TABLE profile_preferences (
    id              TEXT PRIMARY KEY NOT NULL,        -- pref_<hex>
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    category        TEXT NOT NULL CHECK (category IN (
                        'role', 'compensation', 'location', 'company', 'domain', 'work_style')),
    key             TEXT NOT NULL,
    value           TEXT NOT NULL,
    stance          TEXT NOT NULL CHECK (stance IN ('required', 'wanted', 'acceptable', 'unwanted')),
    origin          TEXT NOT NULL CHECK (origin IN ('user_entered', 'statement')),
    statement_id    TEXT REFERENCES profile_preference_statements (id) ON DELETE SET NULL,
    snippet         TEXT,
    certainty       TEXT NOT NULL CHECK (certainty IN ('certain', 'uncertain')),
    active          INTEGER NOT NULL CHECK (active IN (0, 1)),
    superseded_by   TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
) STRICT;
CREATE INDEX profile_preferences_category ON profile_preferences (profile_id, category, active);
CREATE INDEX profile_preferences_key ON profile_preferences (profile_id, key);

-- History of changes to a profile (imports, decisions, edits).
CREATE TABLE profile_events (
    id              INTEGER PRIMARY KEY,
    profile_id      TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    at              TEXT NOT NULL,
    kind            TEXT NOT NULL,
    record          TEXT,
    detail          TEXT NOT NULL
) STRICT;
CREATE INDEX profile_events_profile ON profile_events (profile_id, id);
