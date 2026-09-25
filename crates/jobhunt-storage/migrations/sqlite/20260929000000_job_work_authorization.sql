-- Who may be hired, when a source publishes it as a field of its own
-- (Work at a Startup's "US citizen/visa only"), verbatim. Additive; rows
-- written before it have none until their source is read again
-- (CANONICAL_REVISION 3 makes the next scan re-read every source).
ALTER TABLE jobs ADD COLUMN work_authorization TEXT;
