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

    /// Favorited tracks, most recently favorited first. Mirrors
    /// `list_favorite_moods`'s shape but returns full `Track` rows (joined
    /// from `tracks`) since the frontend needs title/artist/thumbnail to
    /// render and play them, not just an id.
    pub fn list_favorite_tracks(&self) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.title, t.artist, t.duration_seconds, t.thumbnail_url
             FROM track_favorites f JOIN tracks t ON t.id = f.track_id
             ORDER BY f.favorited_at DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Track {
                id: r.get(0)?,
                title: r.get(1)?,
                artist: r.get(2)?,
                duration_seconds: r.get(3)?,
                thumbnail_url: r.get(4)?,
            })
        })?;
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
    fn list_favorite_tracks_returns_favorited_tracks_with_full_fields() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.list_favorite_tracks().unwrap().is_empty());

        let mut a = track("a");
        a.title = "Song A".into();
        a.artist = Some("Artist A".into());
        db.favorite_track(&a).unwrap();
        db.favorite_track(&track("b")).unwrap();
        db.unfavorite_track("b").unwrap();

        let favorites = db.list_favorite_tracks().unwrap();
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0].id, "a");
        assert_eq!(favorites[0].title, "Song A");
        assert_eq!(favorites[0].artist.as_deref(), Some("Artist A"));
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
