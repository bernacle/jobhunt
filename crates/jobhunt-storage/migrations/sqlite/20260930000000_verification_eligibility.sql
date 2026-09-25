-- Verification attempts and stored eligibility decisions. Additive: no
-- existing table changes.

-- One row per verification attempt of one source record, never updated or
-- deleted. The columns hold what is queried (latest attempt, latest
-- success, states); `record` is the whole VerificationRecord as JSON
-- (authority chain, compensation facts, published places, unknowns,
-- failure). `succeeded` is 1 when the source gave a definitive answer
-- (active or closed); a failed attempt never replaces a success.
CREATE TABLE job_verifications (
    id                  TEXT PRIMARY KEY,
    job_id              TEXT NOT NULL REFERENCES jobs (id),
    opportunity_id      TEXT NOT NULL,
    source_kind         TEXT NOT NULL,
    source_instance     TEXT NOT NULL,
    attempted_at        TEXT NOT NULL,
    succeeded           INTEGER NOT NULL CHECK (succeeded IN (0, 1)),
    method              TEXT NOT NULL,
    listing_status      TEXT NOT NULL
        CHECK (listing_status IN ('active', 'closed', 'unreachable', 'ambiguous', 'unknown')),
    application_status  TEXT NOT NULL
        CHECK (application_status IN ('active', 'closed', 'unavailable', 'unknown')),
    authority           TEXT NOT NULL CHECK (authority IN (
        'employer_first_party', 'employer_configured_ats', 'trusted_source',
        'secondary_source', 'unknown')),
    checked_url         TEXT,
    content_fingerprint TEXT,
    failure_kind        TEXT,
    revision            TEXT NOT NULL,
    record              TEXT NOT NULL
) STRICT;

CREATE INDEX job_verifications_by_job ON job_verifications (job_id, attempted_at);
CREATE INDEX job_verifications_successes ON job_verifications (job_id, succeeded, attempted_at);

-- Eligibility decisions, keyed by everything that produced them: the
-- opportunity, the profile, and a digest of the profile revision, every
-- source record's material content and status, the verification each rests
-- on, and the rules revision (see jobhunt_eligibility::cache). A changed
-- input is a new key, so a stored decision is never reused for inputs it
-- was not computed from; older rows remain as an audit trail.
CREATE TABLE eligibility_decisions (
    opportunity_id    TEXT NOT NULL,
    profile_id        TEXT NOT NULL,
    cache_key         TEXT NOT NULL,
    profile_revision  INTEGER NOT NULL,
    rules_version     TEXT NOT NULL,
    status            TEXT NOT NULL
        CHECK (status IN ('eligible', 'conditional', 'uncertain', 'ineligible')),
    decided_at        TEXT NOT NULL,
    decision          TEXT NOT NULL,
    PRIMARY KEY (opportunity_id, profile_id, cache_key)
) STRICT, WITHOUT ROWID;
