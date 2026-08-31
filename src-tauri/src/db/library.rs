use rusqlite::OptionalExtension;

use super::{Db, now};
use crate::error::Result;
use crate::models::Track;

impl Db {
    pub fn favorite_track(&self, track: &Track) -> Result<()> {
        self.upsert_track(track)?;
        self.conn.execute(
            "INSERT INTO track_favorites (track_id, favorited_at) VALUES (?1, ?2)
             ON CONFLICT(track_id) DO UPDATE SET favorited_at = excluded.favorited_at",
            rusqlite::params![track.id, now()],
        )?;
        Ok(())
    }

    pub fn unfavorite_track(&self, track_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM track_favorites WHERE track_id = ?1",
            [track_id],
        )?;
        Ok(())
    }

    pub fn is_track_favorited(&self, track_id: &str) -> Result<bool> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM track_favorites WHERE track_id = ?1",
                [track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }

    pub fn favorite_mood(&self, mood_id: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO mood_favorites (mood_id, favorited_at) VALUES (?1, ?2)
             ON CONFLICT(mood_id) DO UPDATE SET favorited_at = excluded.favorited_at",
            rusqlite::params![mood_id, now()],
        )?;
        Ok(())
    }

    pub fn unfavorite_mood(&self, mood_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM mood_favorites WHERE mood_id = ?1", [mood_id])?;
        Ok(())
    }

    pub fn list_favorite_moods(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT mood_id FROM mood_favorites ORDER BY favorited_at DESC")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn set_track_feedback(&self, track: &Track, liked: bool) -> Result<()> {
        self.upsert_track(track)?;
        self.conn.execute(
            "INSERT INTO track_feedback (track_id, liked, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_id) DO UPDATE SET liked = excluded.liked, updated_at = excluded.updated_at",
            rusqlite::params![track.id, liked as i64, now()],
        )?;
        Ok(())
    }

    /// `None` means no feedback recorded; `Some(true)` liked, `Some(false)` disliked.
    pub fn get_track_feedback(&self, track_id: &str) -> Result<Option<bool>> {
        let liked: Option<i64> = self
            .conn
            .query_row(
                "SELECT liked FROM track_feedback WHERE track_id = ?1",
                [track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(liked.map(|v| v != 0))
    }

    /// No caller yet — wired up when media resolution (Fase 3) can actually
    /// detect an unavailable track; exercised directly by tests until then.
    #[allow(dead_code)]
    pub fn mark_track_unavailable(&self, track_id: &str, reason: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO track_unavailable (track_id, reason, marked_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(track_id) DO UPDATE SET reason = excluded.reason, marked_at = excluded.marked_at",
            rusqlite::params![track_id, reason, now()],
        )?;
        Ok(())
    }

    /// No caller yet — the mood engine (Fase 4) will use this to skip
    /// known-bad tracks when picking candidates; exercised directly by
    /// tests until then.
    #[allow(dead_code)]
    pub fn is_track_unavailable(&self, track_id: &str) -> Result<bool> {
        let exists: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM track_unavailable WHERE track_id = ?1",
                [track_id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(exists.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn favoriting_a_track_persists_and_can_be_undone() {
        let db = Db::open_in_memory().unwrap();
        let t = track("a");
        assert!(!db.is_track_favorited(&t.id).unwrap());

        db.favorite_track(&t).unwrap();
        assert!(db.is_track_favorited(&t.id).unwrap());

        db.unfavorite_track(&t.id).unwrap();
        assert!(!db.is_track_favorited(&t.id).unwrap());
    }

    #[test]
    fn favoriting_a_mood_lists_it() {
        let db = Db::open_in_memory().unwrap();
        db.favorite_mood("villain").unwrap();
        db.favorite_mood("focus").unwrap();
        db.unfavorite_mood("focus").unwrap();

        assert_eq!(
            db.list_favorite_moods().unwrap(),
            vec!["villain".to_string()]
        );
    }

    #[test]
    fn track_feedback_defaults_to_none_then_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let t = track("a");
        assert_eq!(db.get_track_feedback(&t.id).unwrap(), None);

        db.set_track_feedback(&t, true).unwrap();
        assert_eq!(db.get_track_feedback(&t.id).unwrap(), Some(true));

        db.set_track_feedback(&t, false).unwrap();
        assert_eq!(db.get_track_feedback(&t.id).unwrap(), Some(false));
    }

    #[test]
    fn marking_a_track_unavailable_is_queryable() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.is_track_unavailable("a").unwrap());

        db.mark_track_unavailable("a", "region_blocked").unwrap();
        assert!(db.is_track_unavailable("a").unwrap());
    }
}
