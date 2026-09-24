-- Link-radio sessions: seeded by one track instead of moods. A link
-- session has this set and no session_moods rows.
ALTER TABLE sessions ADD COLUMN seed_track_id TEXT REFERENCES tracks (id);
