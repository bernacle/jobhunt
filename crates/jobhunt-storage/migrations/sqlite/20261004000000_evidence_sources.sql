-- LinkedIn exports and GitHub accounts as evidence sources (BRU-309).
--
-- Additive only: new nullable columns, no table is rebuilt. The `origin`
-- and `kind` columns keep their CHECK constraints (widening them would
-- mean rebuilding tables that other tables reference), so they keep a
-- value they accept and the new columns say which source it really is:
--
--   profile_documents.source_kind   'linkedin' | 'github' for those
--                                   sources (kind is then 'text'); NULL
--                                   for resumes (kind says pdf/text/markdown)
--   <records>.import_origin         'linkedin' | 'github' for records those
--                                   sources created (origin is then
--                                   'resume', i.e. "imported"); NULL for
--                                   resume and user records
--   <records>.corroborations,
--   profile_claims.corroborations   JSON [{document, snippet, section}]:
--                                   other sources that contain the same
--                                   record or claim
--
-- Existing rows read exactly as before (NULL, '[]').

ALTER TABLE profile_documents ADD COLUMN source_kind TEXT
    CHECK (source_kind IN ('linkedin', 'github'));

ALTER TABLE profile_experiences ADD COLUMN import_origin TEXT
    CHECK (import_origin IN ('linkedin', 'github'));
ALTER TABLE profile_experiences ADD COLUMN corroborations TEXT NOT NULL DEFAULT '[]';

ALTER TABLE profile_projects ADD COLUMN import_origin TEXT
    CHECK (import_origin IN ('linkedin', 'github'));
ALTER TABLE profile_projects ADD COLUMN corroborations TEXT NOT NULL DEFAULT '[]';

ALTER TABLE profile_education ADD COLUMN import_origin TEXT
    CHECK (import_origin IN ('linkedin', 'github'));
ALTER TABLE profile_education ADD COLUMN corroborations TEXT NOT NULL DEFAULT '[]';

ALTER TABLE profile_skills ADD COLUMN import_origin TEXT
    CHECK (import_origin IN ('linkedin', 'github'));
ALTER TABLE profile_skills ADD COLUMN corroborations TEXT NOT NULL DEFAULT '[]';

ALTER TABLE profile_claims ADD COLUMN corroborations TEXT NOT NULL DEFAULT '[]';
