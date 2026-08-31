CREATE TABLE scenes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE scene_tracks (
    scene_id  INTEGER NOT NULL REFERENCES scenes (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    track_id  TEXT NOT NULL REFERENCES tracks (id),
    PRIMARY KEY (scene_id, position)
);
