-- Cloud sync bookkeeping (BRU-294). Additive: no existing table changes.
-- None of this is needed to use JobHunt offline; it only remembers what
-- the cloud last said, so the next sync can tell local changes from
-- cloud changes.

-- The account and server this database syncs with (at most one row).
CREATE TABLE sync_account (
    singleton     INTEGER PRIMARY KEY CHECK (singleton = 1),
    server        TEXT NOT NULL,
    user_id       TEXT NOT NULL,
    cursor        INTEGER NOT NULL CHECK (cursor >= 0),
    last_sync_at  TEXT
) STRICT;

-- The last cloud version of every profile entity this database agreed
-- with: the common ancestor of the next three-way merge.
CREATE TABLE sync_entities (
    kind      TEXT NOT NULL,
    id        TEXT NOT NULL,
    version   INTEGER NOT NULL CHECK (version >= 0),
    deleted   INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    digest    TEXT,
    body      TEXT,                              -- canonical JSON
    PRIMARY KEY (kind, id)
) STRICT, WITHOUT ROWID;

-- Changes that could not be merged safely, until the person decides.
CREATE TABLE sync_conflicts (
    kind           TEXT NOT NULL,
    id             TEXT NOT NULL,
    reason         TEXT NOT NULL,
    local_body     TEXT,
    cloud_body     TEXT,
    cloud_version  INTEGER NOT NULL,
    detected_at    TEXT NOT NULL,
    PRIMARY KEY (kind, id)
) STRICT, WITHOUT ROWID;

-- Feedback events known to be in the cloud (pushed or pulled).
CREATE TABLE sync_feedback (
    id  TEXT PRIMARY KEY
) STRICT, WITHOUT ROWID;
