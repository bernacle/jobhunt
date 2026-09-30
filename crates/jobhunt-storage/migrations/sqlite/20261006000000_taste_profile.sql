-- The candidate taste profile (BRU-321). Additive: no existing table or
-- row changes, and structured preferences stay exactly as they were
-- (the taste profile reads them live).

-- One statement of the taste profile ("prefers small technical teams"),
-- tombstones of removed ones included. The record, with its provenance
-- and the person's corrections, is `body` (JSON); the columns beside it
-- are for inspection.
CREATE TABLE profile_taste (
    id          TEXT PRIMARY KEY NOT NULL,        -- taste_<hex>
    profile_id  TEXT NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    dimension   TEXT NOT NULL,
    value       TEXT NOT NULL,
    polarity    TEXT NOT NULL CHECK (polarity IN ('prefer', 'open', 'avoid', 'neutral')),
    review      TEXT NOT NULL CHECK (review IN ('unreviewed', 'confirmed', 'corrected', 'removed')),
    body        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
) STRICT;

CREATE INDEX profile_taste_profile ON profile_taste (profile_id, created_at, id);

-- What the person is looking for, in their words, and how it was last
-- interpreted (JSON `body`). One per profile.
CREATE TABLE profile_taste_briefs (
    id          TEXT PRIMARY KEY NOT NULL,        -- tbrief_<hex>
    profile_id  TEXT NOT NULL UNIQUE REFERENCES profiles (id) ON DELETE CASCADE,
    body        TEXT NOT NULL,
    updated_at  TEXT NOT NULL
) STRICT;
