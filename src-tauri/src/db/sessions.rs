use rusqlite::OptionalExtension;

use super::{Db, now};
use crate::error::Result;
use crate::models::{SessionInfo, SessionSummary, Track};

impl Db {
    /// Ends any currently-open session, then starts a new one for `mood_id`.
    pub fn start_session(&self, mood_id: &str) -> Result<SessionInfo> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE ended_at IS NULL",
            [now()],
        )?;

        let started_at = now();
        self.conn.execute(
            "INSERT INTO sessions (mood_id, started_at) VALUES (?1, ?2)",
            rusqlite::params![mood_id, started_at],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(SessionInfo {
            id,
            mood_id: mood_id.to_string(),
            started_at,
            ended_at: None,
        })
    }

    pub fn end_session(&self, session_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE id = ?2 AND ended_at IS NULL",
            rusqlite::params![now(), session_id],
        )?;
        Ok(())
    }

    /// The most recent session that hasn't been ended, if any.
    pub fn current_session(&self) -> Result<Option<SessionInfo>> {
        self.conn
            .query_row(
                "SELECT id, mood_id, started_at, ended_at FROM sessions
                 WHERE ended_at IS NULL ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok(SessionInfo {
                        id: r.get(0)?,
                        mood_id: r.get(1)?,
                        started_at: r.get(2)?,
                        ended_at: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_sessions(&self, limit: i64, offset: i64) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.mood_id, s.started_at, s.ended_at, COUNT(st.track_id)
             FROM sessions s
             LEFT JOIN session_tracks st ON st.session_id = s.id
             GROUP BY s.id
             ORDER BY s.id DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit, offset], |r| {
            Ok(SessionSummary {
                id: r.get(0)?,
                mood_id: r.get(1)?,
                started_at: r.get(2)?,
                ended_at: r.get(3)?,
                track_count: r.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Upserts the track and records it as played at `position` within
    /// `session_id`.
    ///
    /// No caller yet — the playback controller (Fase 3) records real plays
    /// as they happen; exercised directly by this module's tests until then.
    #[allow(dead_code)]
    pub fn record_play(
        &self,
        session_id: i64,
        track: &Track,
        position: u32,
        completion_pct: Option<f64>,
    ) -> Result<()> {
        self.upsert_track(track)?;
        self.conn.execute(
            "INSERT INTO session_tracks (session_id, position, track_id, played_at, completion_pct)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id, position) DO UPDATE SET
                track_id = excluded.track_id,
                played_at = excluded.played_at,
                completion_pct = excluded.completion_pct",
            rusqlite::params![session_id, position, track.id, now(), completion_pct],
        )?;
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
    fn starting_a_session_creates_it_open() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("villain").unwrap();
        assert_eq!(session.mood_id, "villain");
        assert!(session.ended_at.is_none());
    }

    #[test]
    fn starting_a_new_session_ends_the_previous_open_one() {
        let db = Db::open_in_memory().unwrap();
        let first = db.start_session("villain").unwrap();
        db.start_session("focus").unwrap();

        let sessions = db.list_sessions(10, 0).unwrap();
        let first_row = sessions.iter().find(|s| s.id == first.id).unwrap();
        assert!(first_row.ended_at.is_some());
    }

    #[test]
    fn current_session_is_none_when_nothing_open() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.current_session().unwrap().is_none());

        let session = db.start_session("villain").unwrap();
        db.end_session(session.id).unwrap();

        assert!(db.current_session().unwrap().is_none());
    }

    #[test]
    fn current_session_returns_the_open_one() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("villain").unwrap();
        let current = db.current_session().unwrap().unwrap();
        assert_eq!(current.id, session.id);
    }

    #[test]
    fn ending_an_already_ended_session_is_a_harmless_no_op() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("villain").unwrap();
        db.end_session(session.id).unwrap();
        db.end_session(session.id).unwrap(); // must not error
    }

    #[test]
    fn list_sessions_reports_track_count() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session("villain").unwrap();
        db.record_play(session.id, &track("a"), 0, Some(1.0))
            .unwrap();
        db.record_play(session.id, &track("b"), 1, Some(0.5))
            .unwrap();

        let sessions = db.list_sessions(10, 0).unwrap();
        let row = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert_eq!(row.track_count, 2);
    }
}
