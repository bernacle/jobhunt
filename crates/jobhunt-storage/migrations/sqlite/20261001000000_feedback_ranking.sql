-- Feedback on opportunities and stored rankings. Additive: no existing
-- table changes. Both are per-person data, kept out of the shared `jobs`
-- table and keyed by profile.

-- One row per action a person took on an opportunity (save, reject,
-- applied, interview, offer, like, dislike, unsave, seen), never updated
-- or deleted. `reason` is the person's words, verbatim; how it is read is
-- recomputed from it and never stored in its place. `job_id` is the source
-- record the person was looking at, so the feedback follows that record if
-- identity grouping later merges opportunities; `opportunity_id` is the
-- opportunity at the time. `title` and `company` are what was shown then.
CREATE TABLE opportunity_feedback (
    id              TEXT PRIMARY KEY,
    profile_id      TEXT NOT NULL,
    opportunity_id  TEXT NOT NULL,
    job_id          TEXT NOT NULL REFERENCES jobs (id),
    action          TEXT NOT NULL CHECK (action IN (
        'seen', 'save', 'unsave', 'reject', 'applied', 'interview', 'offer',
        'like', 'dislike')),
    reason          TEXT,
    title           TEXT NOT NULL,
    company         TEXT NOT NULL,
    recorded_at     TEXT NOT NULL
) STRICT;

CREATE INDEX opportunity_feedback_by_profile
    ON opportunity_feedback (profile_id, recorded_at);
CREATE INDEX opportunity_feedback_by_job
    ON opportunity_feedback (profile_id, job_id);

-- Rankings that were shown, keyed by a digest of everything they were
-- computed from (see jobhunt_ranking::cache): the profile, every feedback
-- event, the records, verification and eligibility answers, the ranking
-- rules and the day. A changed input is a new key; older rows remain as a
-- record of what was shown and why. `ranking` is the whole ranking
-- (gate, tier, signals with evidence, brief) as JSON.
CREATE TABLE opportunity_rankings (
    opportunity_id   TEXT NOT NULL,
    profile_id       TEXT NOT NULL,
    rank_key         TEXT NOT NULL,
    ranking_version  TEXT NOT NULL,
    tier             TEXT NOT NULL
        CHECK (tier IN ('strong_fit', 'worth_reviewing', 'maybe', 'low_priority')),
    ranked_at        TEXT NOT NULL,
    ranking          TEXT NOT NULL,
    PRIMARY KEY (opportunity_id, profile_id, rank_key)
) STRICT, WITHOUT ROWID;
