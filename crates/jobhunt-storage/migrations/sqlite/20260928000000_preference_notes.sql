-- How an ambiguous part of a preference was read, for the user to check
-- (for example: "“$” can mean USD, CAD, AUD and other dollars, so the
-- currency is unknown"). Additive; existing rows have no note.
ALTER TABLE profile_preferences ADD COLUMN note TEXT;
