use std::collections::HashSet;

use rusqlite::OptionalExtension;

use super::{Db, now};
use crate::error::{EchoraError, Result};
use crate::models::{
    ListeningStats, MoodPlayCount, SessionInfo, SessionMood, SessionSummary, Track,
};

impl Db {
    /// Deletes every session (and, via `ON DELETE CASCADE`, every
    /// `session_tracks`/`session_moods` row) — the Settings "clear all
    /// history" action. Favorites and feedback are untouched; only playback
    /// history goes.
    pub fn clear_history(&self) -> Result<()> {
        self.conn.execute("DELETE FROM sessions", [])?;
        Ok(())
    }

    /// Distinct moods used in the most recent `session_limit` sessions —
    /// used by "Surprise Me" to deprioritize moods played very recently.
    pub fn recent_mood_ids(&self, session_limit: i64) -> Result<HashSet<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT mood_id FROM session_moods
             WHERE session_id IN (SELECT id FROM sessions ORDER BY id DESC LIMIT ?1)",
        )?;
        let rows = stmt.query_map([session_limit], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<HashSet<_>>>()
            .map_err(Into::into)
    }

    /// All-time weighted play count per mood, descending — the "most played
    /// moods" ranking shown in Discover. A mixed session credits each mood
    /// only its weighted fraction (see `MoodPlayCount`).
    pub fn mood_play_counts(&self) -> Result<Vec<MoodPlayCount>> {
        let mut stmt = self.conn.prepare(
            "SELECT mood_id, SUM(weight) / 100.0 as play_count FROM session_moods
             GROUP BY mood_id ORDER BY play_count DESC",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(MoodPlayCount {
                mood_id: r.get(0)?,
                play_count: r.get(1)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Aggregate listening stats. `total_seconds_listened` treats a NULL
    /// `completion_pct` (a play that ended without recording completion — the
    /// app closing mid-track, a crash) as 0 contribution rather than the full
    /// track length, since we don't actually know how much was heard.
    pub fn listening_stats(&self) -> Result<ListeningStats> {
        let total_seconds_listened: i64 = self.conn.query_row(
            "SELECT CAST(COALESCE(SUM(t.duration_seconds * COALESCE(st.completion_pct, 0)), 0) AS INTEGER)
             FROM session_tracks st JOIN tracks t ON t.id = st.track_id",
            [],
            |r| r.get(0),
        )?;
        let total_sessions: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
        let total_tracks_played: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM session_tracks", [], |r| r.get(0))?;
        let top_mood_id = self
            .mood_play_counts()?
            .into_iter()
            .next()
            .map(|m| m.mood_id);

        Ok(ListeningStats {
            total_seconds_listened,
            total_sessions,
            total_tracks_played,
            top_mood_id,
            category_breakdown: Vec::new(),
        })
    }

    /// Ends any currently-open session, then starts a new one for `moods`
    /// (1-3 entries, weights summing to 100, no duplicate mood id).
    pub fn start_session(&self, moods: &[(String, u8)]) -> Result<SessionInfo> {
        if moods.is_empty() || moods.len() > 3 {
            return Err(EchoraError::InvalidMoodMix(format!(
                "expected 1-3 moods, got {}",
                moods.len()
            )));
        }
        let weight_sum: u32 = moods.iter().map(|(_, weight)| *weight as u32).sum();
        if weight_sum != 100 {
            return Err(EchoraError::InvalidMoodMix(format!(
                "mood weights must sum to 100, got {weight_sum}"
            )));
        }
        let mut seen = HashSet::with_capacity(moods.len());
        if !moods.iter().all(|(mood_id, _)| seen.insert(mood_id)) {
            return Err(EchoraError::InvalidMoodMix(
                "duplicate mood id in mix".into(),
            ));
        }

        self.conn.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE ended_at IS NULL",
            [now()],
        )?;

        let started_at = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO sessions (started_at) VALUES (?1)",
            [started_at],
        )?;
        let id = tx.last_insert_rowid();
        for (mood_id, weight) in moods {
            tx.execute(
                "INSERT INTO session_moods (session_id, mood_id, weight) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, mood_id, weight],
            )?;
        }
        tx.commit()?;

        Ok(SessionInfo {
            id,
            moods: moods
                .iter()
                .map(|(mood_id, weight)| SessionMood {
                    mood_id: mood_id.clone(),
                    weight: *weight,
                })
                .collect(),
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

    fn session_moods(&self, session_id: i64) -> Result<Vec<SessionMood>> {
        let mut stmt = self
            .conn
            .prepare("SELECT mood_id, weight FROM session_moods WHERE session_id = ?1")?;
        let rows = stmt.query_map([session_id], |r| {
            Ok(SessionMood {
                mood_id: r.get(0)?,
                weight: r.get(1)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// The most recent session that hasn't been ended, if any.
    pub fn current_session(&self) -> Result<Option<SessionInfo>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, started_at, ended_at FROM sessions
                 WHERE ended_at IS NULL ORDER BY id DESC LIMIT 1",
                [],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()?;

        let Some((id, started_at, ended_at)) = row else {
            return Ok(None);
        };
        Ok(Some(SessionInfo {
            id,
            moods: self.session_moods(id)?,
            started_at,
            ended_at,
        }))
    }

    pub fn list_sessions(&self, limit: i64, offset: i64) -> Result<Vec<SessionSummary>> {
        let mut stmt = self.conn.prepare(
            "SELECT s.id, s.started_at, s.ended_at, COUNT(st.track_id)
             FROM sessions s
             LEFT JOIN session_tracks st ON st.session_id = s.id
             GROUP BY s.id
             ORDER BY s.id DESC
             LIMIT ?1 OFFSET ?2",
        )?;
        let rows: Vec<(i64, i64, Option<i64>, u32)> = stmt
            .query_map(rusqlite::params![limit, offset], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        rows.into_iter()
            .map(|(id, started_at, ended_at, track_count)| {
                Ok(SessionSummary {
                    id,
                    moods: self.session_moods(id)?,
                    started_at,
                    ended_at,
                    track_count,
                })
            })
            .collect()
    }

    /// Upserts the track and records it as played at `position` within
    /// `session_id`.
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

    fn single(mood_id: &str) -> Vec<(String, u8)> {
        vec![(mood_id.to_string(), 100)]
    }

    #[test]
    fn clear_history_removes_sessions_and_cascades_to_session_tracks() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();
        db.record_play(session.id, &track("a"), 0, Some(1.0))
            .unwrap();

        db.clear_history().unwrap();

        assert!(db.list_sessions(10, 0).unwrap().is_empty());
        assert!(db.avg_completion_by_track().unwrap().is_empty());
    }

    #[test]
    fn recent_mood_ids_reflects_the_last_n_sessions_only() {
        let db = Db::open_in_memory().unwrap();
        db.start_session(&single("old-mood")).unwrap();
        db.start_session(&single("recent-mood")).unwrap();

        let recent = db.recent_mood_ids(1).unwrap();
        assert!(recent.contains("recent-mood"));
        assert!(!recent.contains("old-mood"));
    }

    #[test]
    fn starting_a_session_creates_it_open() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();
        assert_eq!(
            session.moods,
            vec![SessionMood {
                mood_id: "villain".into(),
                weight: 100
            }]
        );
        assert!(session.ended_at.is_none());
    }

    #[test]
    fn starting_a_new_session_ends_the_previous_open_one() {
        let db = Db::open_in_memory().unwrap();
        let first = db.start_session(&single("villain")).unwrap();
        db.start_session(&single("focus")).unwrap();

        let sessions = db.list_sessions(10, 0).unwrap();
        let first_row = sessions.iter().find(|s| s.id == first.id).unwrap();
        assert!(first_row.ended_at.is_some());
    }

    #[test]
    fn current_session_is_none_when_nothing_open() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.current_session().unwrap().is_none());

        let session = db.start_session(&single("villain")).unwrap();
        db.end_session(session.id).unwrap();

        assert!(db.current_session().unwrap().is_none());
    }

    #[test]
    fn current_session_returns_the_open_one() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();
        let current = db.current_session().unwrap().unwrap();
        assert_eq!(current.id, session.id);
    }

    #[test]
    fn ending_an_already_ended_session_is_a_harmless_no_op() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();
        db.end_session(session.id).unwrap();
        db.end_session(session.id).unwrap(); // must not error
    }

    #[test]
    fn list_sessions_reports_track_count() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();
        db.record_play(session.id, &track("a"), 0, Some(1.0))
            .unwrap();
        db.record_play(session.id, &track("b"), 1, Some(0.5))
            .unwrap();

        let sessions = db.list_sessions(10, 0).unwrap();
        let row = sessions.iter().find(|s| s.id == session.id).unwrap();
        assert_eq!(row.track_count, 2);
    }

    #[test]
    fn mood_play_counts_ranks_moods_by_session_count_descending() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.mood_play_counts().unwrap().is_empty());

        db.start_session(&single("villain")).unwrap();
        db.start_session(&single("villain")).unwrap();
        db.start_session(&single("focus")).unwrap();

        let counts = db.mood_play_counts().unwrap();
        assert_eq!(counts.len(), 2);
        assert_eq!(counts[0].mood_id, "villain");
        assert_eq!(counts[0].play_count, 2.0);
        assert_eq!(counts[1].mood_id, "focus");
        assert_eq!(counts[1].play_count, 1.0);
    }

    #[test]
    fn mood_play_counts_credits_fractional_weight_for_mixed_sessions() {
        let db = Db::open_in_memory().unwrap();
        db.start_session(&[("villain".to_string(), 70), ("chill".to_string(), 30)])
            .unwrap();

        let counts = db.mood_play_counts().unwrap();
        let villain = counts.iter().find(|c| c.mood_id == "villain").unwrap();
        let chill = counts.iter().find(|c| c.mood_id == "chill").unwrap();
        assert!((villain.play_count - 0.7).abs() < 0.001);
        assert!((chill.play_count - 0.3).abs() < 0.001);
    }

    #[test]
    fn listening_stats_on_empty_database_is_all_zero() {
        let db = Db::open_in_memory().unwrap();
        let stats = db.listening_stats().unwrap();
        assert_eq!(stats.total_seconds_listened, 0);
        assert_eq!(stats.total_sessions, 0);
        assert_eq!(stats.total_tracks_played, 0);
        assert_eq!(stats.top_mood_id, None);
        assert!(stats.category_breakdown.is_empty());
    }

    #[test]
    fn listening_stats_treats_unrecorded_completion_as_zero_seconds() {
        let db = Db::open_in_memory().unwrap();
        let session = db.start_session(&single("villain")).unwrap();

        let mut half_played = track("a");
        half_played.duration_seconds = Some(200);
        db.record_play(session.id, &half_played, 0, Some(0.5))
            .unwrap(); // 100s

        let mut unrecorded = track("b");
        unrecorded.duration_seconds = Some(100);
        db.record_play(session.id, &unrecorded, 1, None).unwrap(); // 0s — no completion recorded

        let stats = db.listening_stats().unwrap();
        assert_eq!(stats.total_seconds_listened, 100);
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.total_tracks_played, 2);
        assert_eq!(stats.top_mood_id, Some("villain".to_string()));
    }

    #[test]
    fn start_session_rejects_weights_that_dont_sum_to_100() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .start_session(&[("villain".to_string(), 70), ("chill".to_string(), 20)])
            .unwrap_err();
        assert!(matches!(err, EchoraError::InvalidMoodMix(_)));
    }

    #[test]
    fn start_session_rejects_more_than_three_moods() {
        let db = Db::open_in_memory().unwrap();
        let moods = vec![
            ("a".to_string(), 25),
            ("b".to_string(), 25),
            ("c".to_string(), 25),
            ("d".to_string(), 25),
        ];
        let err = db.start_session(&moods).unwrap_err();
        assert!(matches!(err, EchoraError::InvalidMoodMix(_)));
    }

    #[test]
    fn start_session_rejects_duplicate_mood_ids() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .start_session(&[("villain".to_string(), 50), ("villain".to_string(), 50)])
            .unwrap_err();
        assert!(matches!(err, EchoraError::InvalidMoodMix(_)));
    }

    #[test]
    fn start_session_persists_and_reads_back_a_mixed_session() {
        let db = Db::open_in_memory().unwrap();
        let session = db
            .start_session(&[("villain".to_string(), 70), ("chill".to_string(), 30)])
            .unwrap();

        let mut moods = session.moods.clone();
        moods.sort_by(|a, b| a.mood_id.cmp(&b.mood_id));
        assert_eq!(
            moods,
            vec![
                SessionMood {
                    mood_id: "chill".into(),
                    weight: 30
                },
                SessionMood {
                    mood_id: "villain".into(),
                    weight: 70
                },
            ]
        );

        let current = db.current_session().unwrap().unwrap();
        assert_eq!(current.moods.len(), 2);
    }
}
