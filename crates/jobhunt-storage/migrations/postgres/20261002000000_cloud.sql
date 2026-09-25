-- JobHunt Cloud schema (BRU-294).
--
-- Two kinds of data live here, kept apart on purpose:
--
-- * The shared corpus: jobs discovered from public job boards, their
--   history, identity evidence, discovery runs and verification attempts.
--   A Greenhouse job is stored once, however many people it concerns, and
--   discovered and verified once. These tables have no user column.
--
-- * Private data: everything that belongs to one person (profile, claims,
--   preferences, feedback, eligibility decisions, rankings, search state),
--   keyed by user_id and deleted with the account. Values that reveal the
--   person (every profile record, feedback reasons, profile history
--   details, eligibility decisions and rankings, which quote their
--   preferences) are encrypted by the application (AES-256-GCM, see
--   jobhunt_storage::postgres::crypto) and stored as bytea; only ids,
--   states and timestamps needed for querying are plain.
--
-- Identifiers keep the application's formats (job_<hex>, opp_<hex>,
-- clm_<hex>, usr_<hex>, ...) as text with the "C" collation, so ordering by
-- id matches the local SQLite store byte for byte.

-- ---------------------------------------------------------------------
-- Shared corpus
-- ---------------------------------------------------------------------

CREATE TABLE jobs (
    id                  text COLLATE "C" PRIMARY KEY,
    source_kind         text NOT NULL,
    source_instance     text NOT NULL,
    source_job_id       text,
    fetched_from        text,
    url                 text NOT NULL,
    apply_url           text,
    company             text NOT NULL,
    title               text NOT NULL,
    department          text,
    team                text,
    location            text,
    locations           jsonb NOT NULL DEFAULT '[]',
    employment_type     text,
    workplace_type      text,
    is_remote           boolean,
    compensation        jsonb,
    work_authorization  text,
    description_text    text,
    description_html    text,
    posted_at           timestamptz,
    source_updated_at   timestamptz,
    -- Normalized words for search (see JobPosting::search_document).
    search_text         text NOT NULL,
    fingerprint         text NOT NULL,
    content_fingerprint text,
    first_seen_at       timestamptz NOT NULL,
    last_seen_at        timestamptz NOT NULL,
    content_updated_at  timestamptz NOT NULL,
    status              text NOT NULL CHECK (status IN ('open', 'closed')),
    closed_at           timestamptz,
    opportunity_id      text COLLATE "C" NOT NULL,
    -- 'sync': first stored because a person's synced feedback referred to
    -- it, not yet seen by cloud discovery. The verification worker checks
    -- these first.
    origin              text NOT NULL DEFAULT 'discovery'
                        CHECK (origin IN ('discovery', 'sync'))
);

CREATE UNIQUE INDEX jobs_source_identity
    ON jobs (source_kind, source_instance, source_job_id)
    WHERE source_job_id IS NOT NULL;
CREATE INDEX jobs_source_status ON jobs (source_kind, source_instance, status);
CREATE INDEX jobs_opportunity ON jobs (opportunity_id);
CREATE INDEX jobs_listing_order
    ON jobs (status, posted_at DESC NULLS LAST, first_seen_at DESC, id);
CREATE INDEX jobs_first_seen ON jobs (first_seen_at);

CREATE TABLE discovery_runs (
    id                          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    started_at                  timestamptz NOT NULL,
    finished_at                 timestamptz,
    sources                     integer NOT NULL DEFAULT 0,
    failed                      integer NOT NULL DEFAULT 0,
    received                    integer NOT NULL DEFAULT 0,
    normalized                  integer NOT NULL DEFAULT 0,
    rejected                    integer NOT NULL DEFAULT 0,
    new                         integer NOT NULL DEFAULT 0,
    updated                     integer NOT NULL DEFAULT 0,
    unchanged                   integer NOT NULL DEFAULT 0,
    reopened                    integer NOT NULL DEFAULT 0,
    closed                      integer NOT NULL DEFAULT 0,
    multi_source_opportunities  integer NOT NULL DEFAULT 0
);

CREATE TABLE source_scans (
    id                bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    run_id            bigint NOT NULL REFERENCES discovery_runs (id),
    source_kind       text NOT NULL,
    source_instance   text NOT NULL,
    status            text NOT NULL CHECK (status IN ('listing', 'not_modified', 'failed')),
    complete          boolean NOT NULL,
    closing_applied   boolean NOT NULL,
    closing_withheld  text,
    started_at        timestamptz NOT NULL,
    finished_at       timestamptz NOT NULL,
    received          integer NOT NULL DEFAULT 0,
    normalized        integer NOT NULL DEFAULT 0,
    rejected          integer NOT NULL DEFAULT 0,
    skipped           integer NOT NULL DEFAULT 0,
    duplicates        integer NOT NULL DEFAULT 0,
    new               integer NOT NULL DEFAULT 0,
    updated           integer NOT NULL DEFAULT 0,
    unchanged         integer NOT NULL DEFAULT 0,
    reopened          integer NOT NULL DEFAULT 0,
    closed            integer NOT NULL DEFAULT 0,
    validator         text,
    revision          text NOT NULL,
    error             text
);

CREATE INDEX source_scans_by_source
    ON source_scans (source_kind, source_instance, status, id);

-- History: one row per change (NEW / UPDATED / CLOSED / REOPENED), never
-- per unchanged observation. `previous` is the JobSnapshot a content change
-- replaced.
CREATE TABLE job_events (
    id              bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    job_id          text COLLATE "C" NOT NULL REFERENCES jobs (id),
    run_id          bigint REFERENCES discovery_runs (id),
    kind            text NOT NULL CHECK (kind IN ('new', 'updated', 'closed', 'reopened')),
    at              timestamptz NOT NULL,
    changed_fields  jsonb NOT NULL DEFAULT '[]',
    previous        jsonb
);

CREATE INDEX job_events_by_job ON job_events (job_id, id);

-- Identity evidence for cross-source grouping.
CREATE TABLE job_evidence (
    job_id  text COLLATE "C" NOT NULL REFERENCES jobs (id),
    key     text NOT NULL,
    PRIMARY KEY (job_id, key)
);

CREATE INDEX job_evidence_by_key ON job_evidence (key);

-- One row per verification attempt, never updated or deleted. The facts
-- (is the listing open, who publishes it, what it pays) do not depend on
-- who asked, so everyone shares them.
CREATE TABLE job_verifications (
    id                  text COLLATE "C" PRIMARY KEY,
    job_id              text COLLATE "C" NOT NULL REFERENCES jobs (id),
    opportunity_id      text COLLATE "C" NOT NULL,
    source_kind         text NOT NULL,
    source_instance     text NOT NULL,
    attempted_at        timestamptz NOT NULL,
    succeeded           boolean NOT NULL,
    method              text NOT NULL,
    listing_status      text NOT NULL
        CHECK (listing_status IN ('active', 'closed', 'unreachable', 'ambiguous', 'unknown')),
    application_status  text NOT NULL
        CHECK (application_status IN ('active', 'closed', 'unavailable', 'unknown')),
    authority           text NOT NULL CHECK (authority IN (
        'employer_first_party', 'employer_configured_ats', 'trusted_source',
        'secondary_source', 'unknown')),
    checked_url         text,
    content_fingerprint text,
    failure_kind        text,
    revision            text NOT NULL,
    record              jsonb NOT NULL
);

CREATE INDEX job_verifications_latest
    ON job_verifications (job_id, attempted_at DESC, id DESC);
CREATE INDEX job_verifications_latest_success
    ON job_verifications (job_id, attempted_at DESC, id DESC) WHERE succeeded;

-- ---------------------------------------------------------------------
-- Scheduled work
-- ---------------------------------------------------------------------

-- Every source the cloud discovers, with when it is due and who holds it.
-- A worker claims due sources by setting a lease (UPDATE … FOR UPDATE SKIP
-- LOCKED), so two workers never read the same board at once, and a lease
-- left by a crashed worker simply expires.
CREATE TABLE source_schedule (
    source_kind           text NOT NULL,
    source_instance       text NOT NULL,
    -- The configured display name, if any.
    company               text,
    -- 'active': people recently acted on its jobs (read more often).
    tier                  text NOT NULL DEFAULT 'normal' CHECK (tier IN ('active', 'normal')),
    -- Still configured. Sources removed from the configuration keep their
    -- jobs and history but are no longer read.
    enabled               boolean NOT NULL DEFAULT true,
    next_due_at           timestamptz NOT NULL,
    consecutive_failures  integer NOT NULL DEFAULT 0,
    last_started_at       timestamptz,
    last_finished_at      timestamptz,
    last_status           text CHECK (last_status IN ('succeeded', 'failed')),
    last_error            text,
    lease_owner           text,
    lease_expires_at      timestamptz,
    PRIMARY KEY (source_kind, source_instance)
);

CREATE INDEX source_schedule_due ON source_schedule (next_due_at) WHERE enabled;

-- A verification worker's claim on a job, so concurrent workers do not
-- verify the same listing twice. Expired leases are free.
CREATE TABLE verification_leases (
    job_id      text COLLATE "C" PRIMARY KEY REFERENCES jobs (id),
    owner       text NOT NULL,
    expires_at  timestamptz NOT NULL
);

-- One row per worker execution, for operations (start, end, outcome).
CREATE TABLE worker_runs (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    kind         text NOT NULL CHECK (kind IN ('discovery', 'verification')),
    worker       text NOT NULL,
    started_at   timestamptz NOT NULL,
    finished_at  timestamptz,
    status       text NOT NULL CHECK (status IN ('running', 'succeeded', 'failed')),
    summary      jsonb NOT NULL DEFAULT '{}',
    error        text
);

CREATE INDEX worker_runs_by_kind ON worker_runs (kind, started_at DESC);

-- ---------------------------------------------------------------------
-- Accounts
-- ---------------------------------------------------------------------

CREATE TABLE users (
    id                  text COLLATE "C" PRIMARY KEY CHECK (id ~ '^usr_[0-9a-f]{32}$'),
    created_at          timestamptz NOT NULL,
    -- Per-person change counter: every private write takes the next value
    -- while holding this row's lock, so sync cursors never skip a change.
    change_seq          bigint NOT NULL DEFAULT 0,
    -- Tokens issued before this instant are refused ("log out everywhere").
    tokens_valid_after  timestamptz
);

-- Who a user is at an identity provider: (issuer, subject) of their
-- tokens. Nothing else about them (no email, no name) is kept.
CREATE TABLE user_identities (
    issuer         text NOT NULL,
    subject        text NOT NULL,
    user_id        text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at     timestamptz NOT NULL,
    last_login_at  timestamptz NOT NULL,
    PRIMARY KEY (issuer, subject)
);

CREATE INDEX user_identities_by_user ON user_identities (user_id);

-- Personal access tokens (for MCP clients and scripts without OAuth).
-- Only a SHA-256 digest of the token is stored.
CREATE TABLE api_tokens (
    id            text COLLATE "C" PRIMARY KEY,
    user_id       text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    name          text NOT NULL,
    token_hash    bytea NOT NULL UNIQUE,
    created_at    timestamptz NOT NULL,
    last_used_at  timestamptz,
    expires_at    timestamptz,
    revoked_at    timestamptz
);

CREATE INDEX api_tokens_by_user ON api_tokens (user_id);

-- ---------------------------------------------------------------------
-- Private data (one person each)
-- ---------------------------------------------------------------------

CREATE TABLE profiles (
    user_id     text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    profile_id  text COLLATE "C" NOT NULL,
    revision    bigint NOT NULL CHECK (revision >= 0),
    created_at  timestamptz NOT NULL,
    updated_at  timestamptz NOT NULL,
    PRIMARY KEY (user_id, profile_id)
);

-- Every record of a profile (basics, documents, experiences, projects,
-- education, skills, claims, preferences, statements) as one encrypted
-- entity with its own version: the unit of sync. Deleted records stay as
-- tombstones (body NULL) so other devices learn about the deletion.
CREATE TABLE profile_entities (
    user_id     text COLLATE "C" NOT NULL,
    profile_id  text COLLATE "C" NOT NULL,
    kind        text NOT NULL CHECK (kind IN (
        'profile', 'document', 'experience', 'project', 'education', 'skill', 'claim',
        'preference', 'statement')),
    entity_id   text COLLATE "C" NOT NULL,
    version     bigint NOT NULL CHECK (version > 0),
    seq         bigint NOT NULL,
    deleted     boolean NOT NULL,
    digest      text,
    body        bytea,
    updated_at  timestamptz NOT NULL,
    PRIMARY KEY (user_id, profile_id, kind, entity_id),
    FOREIGN KEY (user_id, profile_id) REFERENCES profiles (user_id, profile_id) ON DELETE CASCADE,
    CHECK (deleted = (body IS NULL)),
    CHECK (deleted = (digest IS NULL))
);

CREATE INDEX profile_entities_changes ON profile_entities (user_id, seq);

CREATE TABLE profile_events (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id     text COLLATE "C" NOT NULL,
    profile_id  text COLLATE "C" NOT NULL,
    at          timestamptz NOT NULL,
    kind        text NOT NULL,
    record      text,
    detail      bytea NOT NULL,
    FOREIGN KEY (user_id, profile_id) REFERENCES profiles (user_id, profile_id) ON DELETE CASCADE
);

CREATE INDEX profile_events_by_profile ON profile_events (user_id, profile_id, id);

-- Feedback, never updated. `reason` is the person's own words (encrypted).
CREATE TABLE feedback (
    user_id         text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    id              text COLLATE "C" NOT NULL,
    profile_id      text COLLATE "C" NOT NULL,
    opportunity_id  text COLLATE "C" NOT NULL,
    job_id          text COLLATE "C" NOT NULL REFERENCES jobs (id),
    action          text NOT NULL CHECK (action IN (
        'seen', 'save', 'unsave', 'reject', 'applied', 'interview', 'offer', 'like', 'dislike')),
    reason          bytea,
    title           text NOT NULL,
    company         text NOT NULL,
    recorded_at     timestamptz NOT NULL,
    seq             bigint NOT NULL,
    PRIMARY KEY (user_id, id)
);

CREATE INDEX feedback_by_time ON feedback (user_id, profile_id, recorded_at, id);
CREATE INDEX feedback_by_job ON feedback (user_id, profile_id, job_id);
CREATE INDEX feedback_changes ON feedback (user_id, seq);
CREATE INDEX feedback_jobs ON feedback (job_id);

-- Eligibility decisions depend on the person's profile: per user, keyed by
-- a digest of every input (see jobhunt_eligibility::cache).
CREATE TABLE eligibility_decisions (
    user_id           text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    opportunity_id    text COLLATE "C" NOT NULL,
    profile_id        text COLLATE "C" NOT NULL,
    cache_key         text COLLATE "C" NOT NULL,
    profile_revision  bigint NOT NULL,
    rules_version     text NOT NULL,
    status            text NOT NULL
        CHECK (status IN ('eligible', 'conditional', 'uncertain', 'ineligible')),
    decided_at        timestamptz NOT NULL,
    decision          bytea NOT NULL,
    PRIMARY KEY (user_id, opportunity_id, profile_id, cache_key)
);

CREATE INDEX eligibility_by_revision
    ON eligibility_decisions (user_id, profile_id, profile_revision);

-- Rankings that were shown, per user, keyed by a digest of their inputs.
CREATE TABLE rankings (
    user_id          text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    opportunity_id   text COLLATE "C" NOT NULL,
    profile_id       text COLLATE "C" NOT NULL,
    rank_key         text COLLATE "C" NOT NULL,
    ranking_version  text NOT NULL,
    tier             text NOT NULL
        CHECK (tier IN ('strong_fit', 'worth_reviewing', 'maybe', 'low_priority')),
    ranked_at        timestamptz NOT NULL,
    ranking          bytea NOT NULL,
    PRIMARY KEY (user_id, opportunity_id, profile_id, rank_key)
);

-- What the person's shortlists showed them, and when: "new since you last
-- looked" and the notification cursor (BRU-295) read it.
CREATE TABLE user_opportunities (
    user_id          text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    opportunity_id   text COLLATE "C" NOT NULL,
    first_shown_at   timestamptz NOT NULL,
    last_shown_at    timestamptz NOT NULL,
    last_tier        text NOT NULL
        CHECK (last_tier IN ('strong_fit', 'worth_reviewing', 'maybe', 'low_priority')),
    times_shown      integer NOT NULL DEFAULT 1,
    seq              bigint NOT NULL,
    PRIMARY KEY (user_id, opportunity_id)
);

CREATE INDEX user_opportunities_changes ON user_opportunities (user_id, seq);

-- Per-person search and sync state.
CREATE TABLE user_state (
    user_id                  text COLLATE "C" PRIMARY KEY REFERENCES users (id) ON DELETE CASCADE,
    last_sync_at             timestamptz,
    last_shortlist_at        timestamptz,
    -- The last user_opportunities.seq a notification covered (BRU-295).
    notification_cursor      bigint NOT NULL DEFAULT 0,
    -- The profile revision the last shortlist was ranked against.
    shortlist_profile_revision bigint
);

-- Product usage, kept apart from product data: event names and safe
-- metadata (counts, tool names, durations), never text the person wrote.
CREATE TABLE usage_events (
    id        bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id   text COLLATE "C" REFERENCES users (id) ON DELETE CASCADE,
    at        timestamptz NOT NULL,
    event     text NOT NULL,
    metadata  jsonb NOT NULL DEFAULT '{}'
);

CREATE INDEX usage_events_by_time ON usage_events (at);
CREATE INDEX usage_events_by_user ON usage_events (user_id, at);
