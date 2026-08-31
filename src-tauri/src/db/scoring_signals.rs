use std::collections::{HashMap, HashSet};

use super::Db;
use crate::error::Result;

/// Read-only aggregate lookups the mood engine's scorer needs, each
/// fetched once per candidate round rather than per-track — see
/// `mood_engine::build_scoring_context`, which assembles these into a
/// `ScoringContext`.
impl Db {
    pub fn all_track_feedback(&self) -> Result<HashMap<String, bool>> {
        let mut stmt = self
            .conn
            .prepare("SELECT track_id, liked FROM track_feedback")?;
        let rows = stmt.query_map([], |row| {
            let liked: i64 = row.get(1)?;
            Ok((row.get::<_, String>(0)?, liked != 0))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(Into::into)
    }

    pub fn all_favorited_track_ids(&self) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT track_id FROM track_favorites")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<HashSet<_>>>()
            .map_err(Into::into)
    }

    /// Distinct tracks played across the most recent `session_limit`
    /// sessions (any mood) — used to penalize repetition.
    pub fn recently_played_track_ids(&self, session_limit: i64) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT DISTINCT st.track_id
             FROM session_tracks st
             WHERE st.session_id IN (SELECT id FROM sessions ORDER BY id DESC LIMIT ?1)",
        )?;
        let rows = stmt.query_map([session_limit], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<HashSet<_>>>()
            .map_err(Into::into)
    }

    pub fn avg_completion_by_track(&self) -> Result<HashMap<String, f64>> {
        let mut stmt = self.conn.prepare(
            "SELECT track_id, AVG(completion_pct)
             FROM session_tracks
             WHERE completion_pct IS NOT NULL
             GROUP BY track_id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(Into::into)
    }

    pub fn liked_artist_counts(&self) -> Result<HashMap<String, u32>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.artist, COUNT(*)
             FROM track_feedback f
             JOIN tracks t ON t.id = f.track_id
             WHERE f.liked = 1 AND t.artist IS NOT NULL
             GROUP BY t.artist",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u32))
        })?;
        rows.collect::<rusqlite::Result<HashMap<_, _>>>()
            .map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Track;

    fn track(id: &str, artist: Option<&str>) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: artist.map(String::from),
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn all_track_feedback_reflects_likes_and_dislikes() {
        let db = Db::open_in_memory().unwrap();
        db.set_track_feedback(&track("liked", None), true).unwrap();
        db.set_track_feedback(&track("disliked", None), false)
            .unwrap();

        let feedback = db.all_track_feedback().unwrap();
        assert_eq!(feedback.get("liked"), Some(&true));
        assert_eq!(feedback.get("disliked"), Some(&false));
    }

    #[test]
    fn all_favorited_track_ids_lists_only_favorited() {
        let db = Db::open_in_memory().unwrap();
        db.favorite_track(&track("fav", None)).unwrap();
        db.upsert_track(&track("not-fav", None)).unwrap();

        let favorited = db.all_favorited_track_ids().unwrap();
        assert!(favorited.contains("fav"));
        assert!(!favorited.contains("not-fav"));
    }

    #[test]
    fn recently_played_track_ids_only_looks_at_the_last_n_sessions() {
        let db = Db::open_in_memory().unwrap();
        let old_session = db.start_session("mood-a").unwrap();
        db.record_play(old_session.id, &track("old", None), 0, None)
            .unwrap();
        let new_session = db.start_session("mood-b").unwrap();
        db.record_play(new_session.id, &track("new", None), 0, None)
            .unwrap();

        let recent = db.recently_played_track_ids(1).unwrap();
        assert!(recent.contains("new"));
        assert!(!recent.contains("old"));
    }

    #[test]
    fn avg_completion_by_track_averages_across_plays() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("mood-a").unwrap();
        db.record_play(session.id, &track("a", None), 0, Some(1.0))
            .unwrap();
        // Same track played again in a later position within the fixture session.
        let session2 = db.start_session("mood-a").unwrap();
        db.record_play(session2.id, &track("a", None), 0, Some(0.5))
            .unwrap();

        let avg = db.avg_completion_by_track().unwrap();
        assert!((avg.get("a").unwrap() - 0.75).abs() < 0.001);
    }

    #[test]
    fn avg_completion_ignores_plays_with_no_recorded_completion() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("mood-a").unwrap();
        db.record_play(session.id, &track("no-completion", None), 0, None)
            .unwrap();

        let avg = db.avg_completion_by_track().unwrap();
        assert_eq!(avg.get("no-completion"), None);
    }

    #[test]
    fn liked_artist_counts_only_counts_liked_tracks() {
        let db = Db::open_in_memory().unwrap();
        db.set_track_feedback(&track("a", Some("Artist X")), true)
            .unwrap();
        db.set_track_feedback(&track("b", Some("Artist X")), true)
            .unwrap();
        db.set_track_feedback(&track("c", Some("Artist X")), false)
            .unwrap();
        db.set_track_feedback(&track("d", Some("Artist Y")), true)
            .unwrap();

        let counts = db.liked_artist_counts().unwrap();
        assert_eq!(counts.get("Artist X"), Some(&2));
        assert_eq!(counts.get("Artist Y"), Some(&1));
    }
}
