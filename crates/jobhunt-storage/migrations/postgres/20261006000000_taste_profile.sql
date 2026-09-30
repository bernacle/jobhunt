-- The candidate taste profile (BRU-321): two new kinds of profile entity,
-- stored like every other (encrypted, versioned, synced). Additive: the
-- check only widens.
ALTER TABLE profile_entities DROP CONSTRAINT profile_entities_kind_check;
ALTER TABLE profile_entities ADD CONSTRAINT profile_entities_kind_check CHECK (kind IN (
    'profile', 'document', 'experience', 'project', 'education', 'skill', 'claim',
    'preference', 'statement', 'taste', 'taste_brief'));
