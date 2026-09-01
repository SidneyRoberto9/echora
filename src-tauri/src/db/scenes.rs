use super::Db;
use crate::error::{EchoraError, Result};
use crate::models::{SceneSummary, Track};

impl Db {
    /// Upserts every track, then inserts the scene and its ordered track
    /// list, all inside one transaction — this codebase's first
    /// multi-statement write that must be atomic. Every other `Db` method
    /// takes `&self`; `Connection::transaction` needs `&mut self`, which
    /// would make this one method's signature inconsistent with the rest,
    /// so this uses `unchecked_transaction` (`&self`) instead. "Unchecked"
    /// only means it skips a compile-time guarantee that no other query
    /// runs on the connection while the transaction is open — safe here
    /// because `Db` only ever exists inside `Mutex<Db>` in `AppState`, so
    /// the lock already gives exclusive access for the whole call.
    pub fn save_scene(&self, name: &str, tracks: &[Track]) -> Result<SceneSummary> {
        if tracks.is_empty() {
            return Err(EchoraError::QueueEmpty);
        }
        let tx = self.conn.unchecked_transaction()?;
        let created_at = super::now();
        tx.execute(
            "INSERT INTO scenes (name, created_at) VALUES (?1, ?2)",
            rusqlite::params![name, created_at],
        )?;
        let scene_id = tx.last_insert_rowid();
        for (position, track) in tracks.iter().enumerate() {
            // Same upsert SQL as `Db::upsert_track` (`db/mod.rs`), run
            // against `tx` directly (it derefs to `Connection`) instead
            // of `self.conn`, so every write for this scene lands inside
            // the one transaction. Do NOT call `self.upsert_track(track)`
            // here — that runs on `self.conn` outside `tx` and would
            // commit each upsert immediately, defeating the point.
            tx.execute(
                "INSERT INTO tracks (id, title, artist, duration_seconds, thumbnail_url, first_seen_at, last_seen_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    title = excluded.title,
                    artist = excluded.artist,
                    duration_seconds = excluded.duration_seconds,
                    thumbnail_url = excluded.thumbnail_url,
                    last_seen_at = excluded.last_seen_at",
                rusqlite::params![track.id, track.title, track.artist, track.duration_seconds, track.thumbnail_url, created_at],
            )?;
            tx.execute(
                "INSERT INTO scene_tracks (scene_id, position, track_id) VALUES (?1, ?2, ?3)",
                rusqlite::params![scene_id, position as i64, track.id],
            )?;
        }
        tx.commit()?;
        Ok(SceneSummary {
            id: scene_id,
            name: name.to_string(),
            created_at,
            track_count: tracks.len() as u32,
        })
    }

    pub fn list_scenes(&self) -> Result<Vec<SceneSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.name, s.created_at, COUNT(st.position)
             FROM scenes s JOIN scene_tracks st ON st.scene_id = s.id
             GROUP BY s.id
             ORDER BY s.created_at DESC, s.id DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(SceneSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                created_at: r.get(2)?,
                track_count: r.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// The scene's tracks in saved order — used internally by
    /// `commands::queue::play_scene_impl`, never exposed to the frontend
    /// directly.
    pub fn scene_tracks(&self, scene_id: i64) -> Result<Vec<Track>> {
        let mut stmt = self.conn.prepare(
            "SELECT t.id, t.title, t.artist, t.duration_seconds, t.thumbnail_url
             FROM scene_tracks st JOIN tracks t ON t.id = st.track_id
             WHERE st.scene_id = ?1
             ORDER BY st.position ASC",
        )?;
        let rows = stmt.query_map([scene_id], |r| {
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

    pub fn rename_scene(&self, scene_id: i64, name: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE scenes SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, scene_id],
        )?;
        Ok(())
    }

    pub fn delete_scene(&self, scene_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM scenes WHERE id = ?1", [scene_id])?;
        Ok(())
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
    fn save_scene_persists_tracks_never_seen_before() {
        // These tracks were never `upsert_track`ed by any other path
        // (no `record_play`/`favorite_track` call) — this is the
        // "unplayed upcoming track" case the foreign key would reject
        // without an upsert-before-insert.
        let db = Db::open_in_memory().unwrap();
        let tracks = vec![track("a"), track("b")];

        let summary = db.save_scene("Chuva de domingo", &tracks).unwrap();

        assert_eq!(summary.name, "Chuva de domingo");
        assert_eq!(summary.track_count, 2);
    }

    #[test]
    fn list_scenes_returns_saved_scenes_most_recent_first() {
        let db = Db::open_in_memory().unwrap();
        db.save_scene("First", &[track("a")]).unwrap();
        db.save_scene("Second", &[track("b")]).unwrap();

        let scenes = db.list_scenes().unwrap();
        assert_eq!(scenes.len(), 2);
        assert_eq!(scenes[0].name, "Second");
        assert_eq!(scenes[0].track_count, 1);
        assert_eq!(scenes[1].name, "First");
        assert_eq!(scenes[1].track_count, 1);
    }

    #[test]
    fn save_scene_errors_on_an_empty_track_list() {
        let db = Db::open_in_memory().unwrap();
        let err = db.save_scene("Empty", &[]).unwrap_err();
        assert!(matches!(err, EchoraError::QueueEmpty));
    }

    #[test]
    fn scene_tracks_returns_tracks_in_saved_order() {
        let db = Db::open_in_memory().unwrap();
        let summary = db
            .save_scene("Order test", &[track("a"), track("b"), track("c")])
            .unwrap();

        let tracks = db.scene_tracks(summary.id).unwrap();
        let ids: Vec<&str> = tracks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[test]
    fn rename_scene_updates_the_name() {
        let db = Db::open_in_memory().unwrap();
        let summary = db.save_scene("Old name", &[track("a")]).unwrap();

        db.rename_scene(summary.id, "New name").unwrap();

        let scenes = db.list_scenes().unwrap();
        assert_eq!(scenes[0].name, "New name");
    }

    #[test]
    fn delete_scene_removes_it_and_cascades_to_scene_tracks() {
        let db = Db::open_in_memory().unwrap();
        let summary = db.save_scene("Doomed", &[track("a")]).unwrap();

        db.delete_scene(summary.id).unwrap();

        assert!(db.list_scenes().unwrap().is_empty());
        assert!(db.scene_tracks(summary.id).unwrap().is_empty());
    }
}
