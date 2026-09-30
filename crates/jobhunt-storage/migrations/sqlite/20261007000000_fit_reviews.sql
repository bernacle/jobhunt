-- Semantic fit reviews (BRU-322): a model's validated review of one job for
-- one person, stored under a key of everything it was read from (the
-- reviewer, the prompt and rules revision, what was sent about the person's
-- taste and career evidence, and the posting version), so the same review
-- is never paid for twice. Additive: nothing else changes.
CREATE TABLE fit_reviews (
    profile_id   TEXT NOT NULL,
    review_key   TEXT NOT NULL,                  -- rev_<hex>
    reviewer     TEXT NOT NULL,                  -- model/anthropic:claude-opus-5-5
    fit          TEXT NOT NULL
        CHECK (fit IN ('strong', 'plausible', 'insufficient', 'poor')),
    reviewed_at  TEXT NOT NULL,
    review       TEXT NOT NULL,                  -- JSON
    PRIMARY KEY (profile_id, review_key)
) STRICT, WITHOUT ROWID;
