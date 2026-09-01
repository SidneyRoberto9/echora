CREATE TABLE session_moods (
    session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    mood_id    TEXT NOT NULL,
    weight     INTEGER NOT NULL CHECK (weight > 0 AND weight <= 100),
    PRIMARY KEY (session_id, mood_id)
);

INSERT INTO session_moods (session_id, mood_id, weight)
SELECT id, mood_id, 100 FROM sessions;

ALTER TABLE sessions DROP COLUMN mood_id;
