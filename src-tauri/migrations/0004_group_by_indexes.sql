-- Both of these columns are grouped by, but neither is the leading column
-- of its table's primary key, so SQLite had to build a temporary b-tree
-- for every such query.
--
-- session_tracks(track_id): `SELECT track_id, AVG(completion_pct) ...
-- GROUP BY track_id` in db::scoring_signals::avg_completion_by_track, on
-- the hot path — build_scoring_context runs it on every queue top-up, and
-- the table grows by one row per track played.
--
-- session_moods(mood_id): `SELECT mood_id, SUM(weight) ... GROUP BY
-- mood_id` in db::sessions::mood_play_counts, behind Discover and Stats.
--
-- Not added for scene_tracks(track_id): nothing groups or filters by it,
-- only a join that scans either way.
CREATE INDEX idx_session_tracks_track_id ON session_tracks (track_id);
CREATE INDEX idx_session_moods_mood_id ON session_moods (mood_id);
