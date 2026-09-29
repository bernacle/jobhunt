-- Exact field values of sources sharing a record, and ownership of
-- LinkedIn-populated profile basics. Legacy rows stay empty/defaulted.
ALTER TABLE profiles ADD COLUMN basic_sources TEXT NOT NULL DEFAULT '[]';

ALTER TABLE profile_experiences ADD COLUMN source_snapshots TEXT NOT NULL DEFAULT '[]';
ALTER TABLE profile_projects ADD COLUMN source_snapshots TEXT NOT NULL DEFAULT '[]';
ALTER TABLE profile_education ADD COLUMN source_snapshots TEXT NOT NULL DEFAULT '[]';
ALTER TABLE profile_skills ADD COLUMN source_snapshots TEXT NOT NULL DEFAULT '[]';
