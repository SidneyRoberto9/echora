-- Settings: single-row JSON blob so adding a field never needs a migration.
CREATE TABLE settings (
    id   INTEGER PRIMARY KEY CHECK (id = 1),
    data TEXT NOT NULL
);

-- Normalized tracks Echora has actually encountered (played, favorited, or
-- given feedback on) — not a pre-populated catalog. Upserted lazily.
CREATE TABLE tracks (
    id                TEXT PRIMARY KEY,
    title             TEXT NOT NULL,
    artist            TEXT,
    duration_seconds  INTEGER,
    thumbnail_url     TEXT,
    first_seen_at     INTEGER NOT NULL,
    last_seen_at      INTEGER NOT NULL
);

-- One row per mood session (not a flat song list — history is organized by
-- session, per the product's "sessions, not a feed" design).
CREATE TABLE sessions (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    mood_id    TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER
);

CREATE INDEX idx_sessions_ended_at ON sessions (ended_at);

CREATE TABLE session_tracks (
    session_id      INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    position        INTEGER NOT NULL,
    track_id        TEXT NOT NULL REFERENCES tracks (id),
    played_at       INTEGER NOT NULL,
    completion_pct  REAL,
    PRIMARY KEY (session_id, position)
);

CREATE TABLE track_favorites (
    track_id      TEXT PRIMARY KEY REFERENCES tracks (id),
    favorited_at  INTEGER NOT NULL
);

CREATE TABLE mood_favorites (
    mood_id       TEXT PRIMARY KEY,
    favorited_at  INTEGER NOT NULL
);

-- liked = 1 (like) or 0 (dislike). Row absent means no feedback given.
CREATE TABLE track_feedback (
    track_id    TEXT PRIMARY KEY REFERENCES tracks (id),
    liked       INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE track_unavailable (
    track_id   TEXT PRIMARY KEY,
    reason     TEXT NOT NULL,
    marked_at  INTEGER NOT NULL
);
