-- Semantic fit reviews (BRU-322): a model's validated review of one job for
-- one person, stored under a key of everything it was read from, so the same
-- review is never paid for twice. The review quotes what the person wants,
-- so it is sealed like rankings (see crypto.rs); only ids, the reviewer, the
-- fit level and the time are plain. Deleted with the account.
CREATE TABLE fit_reviews (
    user_id      text COLLATE "C" NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    profile_id   text COLLATE "C" NOT NULL,
    review_key   text COLLATE "C" NOT NULL,
    reviewer     text NOT NULL,
    fit          text NOT NULL
        CHECK (fit IN ('strong', 'plausible', 'insufficient', 'poor')),
    reviewed_at  timestamptz NOT NULL,
    review       bytea NOT NULL,
    PRIMARY KEY (user_id, profile_id, review_key)
);
