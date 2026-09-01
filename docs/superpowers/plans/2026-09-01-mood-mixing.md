# Mood Mixing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a session be started from 2–3 moods at once, each with a weight
(sum 100), instead of exactly one mood.

**Architecture:** `sessions.mood_id` (one string) is replaced by a
`session_moods` join table (`session_id, mood_id, weight`) so every session —
single or mixed — stores 1–3 weighted moods uniformly. The mood engine's
candidate generation splits its per-round query budget across the mix
proportional to weight; everything downstream (dedup, scoring, shuffle,
playback, history, ranking) is untouched. Backend lands first as two tasks
(Rust requires the whole crate to compile, so a breaking signature change and
its call-site fixes can't be split across tasks without an unbuildable
intermediate state) — Task 1 is a behavior-preserving refactor, Task 2 adds
the actual mixing capability. Frontend lands as three tasks: data layer,
then the two new UI pieces, then wiring them into Home.

**Tech Stack:** Tauri 2, Rust (rusqlite, rusqlite_migration), React + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-01-mood-mixing-design.md`

## Global Constraints

- Package manager: npm only (repo-wide, see CLAUDE.md).
- Rust is the source of truth for session/mood state — the frontend only
  displays what the backend returns, no client-side session logic.
- A session has 1–3 moods; weights are `u8` and must sum to exactly 100; no
  duplicate mood id within one session. Enforced once, in `Db::start_session`.
- No mid-session re-weighting — a mix is fixed at session start; topping the
  queue back up mid-session reuses the session's already-stored mix.
- No trait blending — scoring stays the existing global `ScoringContext`
  (favorites/feedback/recency), never a synthetic combined `MoodTraits`.
- Every command run before claiming a task done: `cargo fmt --check && cargo
  clippy --all-targets -- -D warnings && cargo test` (from `src-tauri/`) and
  `npm run lint && npm run build` (frontend tasks).

---

### Task 1: Session data model — weighted moods (behavior-preserving)

Replaces `sessions.mood_id: String` with a `session_moods` table and
`SessionInfo.moods: Vec<SessionMood>`. No new user-facing capability yet —
every existing caller still starts a session with exactly one mood at weight
100; this task only makes the storage layer able to hold up to three.

**Files:**
- Create: `src-tauri/migrations/0003_mixed_sessions.sql`
- Modify: `src-tauri/src/db/mod.rs:16-20` (register migration)
- Modify: `src-tauri/src/models.rs` (new `SessionMood`, rewritten `SessionInfo`/`SessionSummary`/`MoodPlayCount`/`CategoryBreakdown`)
- Modify: `src-tauri/src/error.rs` (new `InvalidMoodMix` variant)
- Modify: `src-tauri/src/db/sessions.rs` (full rewrite: write path, read paths, tests)
- Modify: `src-tauri/src/db/scoring_signals.rs` (test call-site fixes only)
- Modify: `src-tauri/src/commands/session.rs` (minimal compile fix: `start_session_impl` internally adapts to the new `Db::start_session`, its own signature unchanged; `build_listening_stats` f64 accumulator; test fixes)
- Modify: `src-tauri/src/commands/queue.rs` (minimal compile fix: `ensure_queue_topped_up` reads `session.moods[0]`, temporary — Task 2 generalizes it; test fix)

**Interfaces:**
- Produces: `models::SessionMood { mood_id: String, weight: u8 }` (`Debug, Clone, PartialEq, Serialize, Deserialize`); `Db::start_session(&self, moods: &[(String, u8)]) -> Result<SessionInfo>`; `EchoraError::InvalidMoodMix(String)`.
- Consumes: nothing new from outside this task.

- [ ] **Step 1: Write the migration**

`src-tauri/migrations/0003_mixed_sessions.sql`:

```sql
CREATE TABLE session_moods (
    session_id INTEGER NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    mood_id    TEXT NOT NULL,
    weight     INTEGER NOT NULL CHECK (weight > 0 AND weight <= 100),
    PRIMARY KEY (session_id, mood_id)
);

INSERT INTO session_moods (session_id, mood_id, weight)
SELECT id, mood_id, 100 FROM sessions;

ALTER TABLE sessions DROP COLUMN mood_id;
```

Register it in `src-tauri/src/db/mod.rs`:

```rust
fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../migrations/0001_init.sql")),
        M::up(include_str!("../../migrations/0002_scenes.sql")),
        M::up(include_str!("../../migrations/0003_mixed_sessions.sql")),
    ])
}
```

- [ ] **Step 2: Run the existing migration test to verify it applies cleanly**

Run: `cd src-tauri && cargo test db::tests::opening_an_in_memory_db_applies_migrations_cleanly`
Expected: PASS (this test already exists and exercises every registered migration against a fresh in-memory db)

- [ ] **Step 3: Add `SessionMood` and update the session/ranking models**

In `src-tauri/src/models.rs`, replace the `SessionInfo`/`SessionSummary` block (lines 60-75) and the `MoodPlayCount` struct (lines 77-81, if present — this repo's current layout has it right after `SessionSummary`):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionMood {
    pub mood_id: String,
    pub weight: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: i64,
    pub moods: Vec<SessionMood>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub moods: Vec<SessionMood>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub track_count: u32,
}

/// Weighted all-time play count for one mood — the "most played moods"
/// ranking. A mixed session credits each of its moods only its weighted
/// fraction (a 70/30 mix credits 0.7 to one mood, 0.3 to the other), not a
/// full play to both.
#[derive(Debug, Clone, Serialize)]
pub struct MoodPlayCount {
    pub mood_id: String,
    pub play_count: f64,
}
```

And in the `CategoryBreakdown` struct further down:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct CategoryBreakdown {
    pub category: String,
    pub session_count: f64,
}
```

- [ ] **Step 4: Add the validation error variant**

In `src-tauri/src/error.rs`, add to the `EchoraError` enum (after `UnknownMood`):

```rust
    #[error("invalid mood mix: {0}")]
    InvalidMoodMix(String),
```

And to `code()`:

```rust
            EchoraError::InvalidMoodMix(_) => "invalid_mood_mix",
```

- [ ] **Step 5: Rewrite `src-tauri/src/db/sessions.rs`**

Full file:

```rust
use std::collections::HashSet;

use rusqlite::OptionalExtension;

use super::{Db, now};
use crate::error::{EchoraError, Result};
use crate::models::{ListeningStats, MoodPlayCount, SessionInfo, SessionMood, SessionSummary, Track};

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
```

- [ ] **Step 6: Run the db::sessions tests**

Run: `cd src-tauri && cargo test db::sessions::`
Expected: PASS — all tests above green

- [ ] **Step 7: Fix `db::scoring_signals` test call sites**

In `src-tauri/src/db/scoring_signals.rs`, every `db.start_session("mood-a")` /
`db.start_session("mood-b")` in the test module becomes
`db.start_session(&[("mood-a".to_string(), 100)])` /
`db.start_session(&[("mood-b".to_string(), 100)])` (5 call sites, lines
114, 117, 129, 133, 144 as of this plan — grep to confirm exact lines before
editing since line numbers drift):

```rust
let old_session = db.start_session(&[("mood-a".to_string(), 100)]).unwrap();
...
let new_session = db.start_session(&[("mood-b".to_string(), 100)]).unwrap();
...
let session = db.start_session(&[("mood-a".to_string(), 100)]).unwrap();
...
let session2 = db.start_session(&[("mood-a".to_string(), 100)]).unwrap();
...
let session = db.start_session(&[("mood-a".to_string(), 100)]).unwrap();
```

- [ ] **Step 8: Fix `commands/session.rs` to compile against the new `Db::start_session`**

`start_session_impl` keeps its own signature unchanged for this task — it
just adapts its one call into `Db::start_session`:

```rust
pub(crate) fn start_session_impl(state: &AppState, mood_id: &str) -> Result<SessionInfo> {
    state.moods.get(mood_id)?;
    let session = state
        .db
        .lock()
        .unwrap()
        .start_session(&[(mood_id.to_string(), 100)])?;
    state.queue.lock().unwrap().clear();
    Ok(session)
}
```

`build_listening_stats` moves to an `f64` accumulator (its `CategoryBreakdown`
field is now `f64`) and sorts with `partial_cmp` instead of `sort_by_key`
(`f64` has no `Ord`):

```rust
pub(crate) fn build_listening_stats(state: &AppState) -> Result<ListeningStats> {
    let mut stats = state.db.lock().unwrap().listening_stats()?;
    let ranking = state.db.lock().unwrap().mood_play_counts()?;

    let mut by_category: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for entry in ranking {
        if let Ok(mood) = state.moods.get(&entry.mood_id) {
            *by_category.entry(mood.category.clone()).or_insert(0.0) += entry.play_count;
        }
    }
    let mut breakdown: Vec<CategoryBreakdown> = by_category
        .into_iter()
        .map(|(category, session_count)| CategoryBreakdown {
            category,
            session_count,
        })
        .collect();
    breakdown.sort_by(|a, b| b.session_count.partial_cmp(&a.session_count).unwrap());

    stats.category_breakdown = breakdown;
    Ok(stats)
}
```

Fix the two test call/assertion sites in the same file's `mod tests`:

```rust
#[test]
fn starting_a_session_with_a_known_mood_clears_the_queue() {
    let state = test_state();
    let mood_id = state.moods.list()[0].id.clone();
    state.queue.lock().unwrap().add_candidates([Track {
        id: "leftover".into(),
        title: "leftover".into(),
        artist: None,
        duration_seconds: None,
        thumbnail_url: None,
    }]);

    let session = start_session_impl(&state, &mood_id).unwrap();

    assert_eq!(session.moods.len(), 1);
    assert_eq!(session.moods[0].mood_id, mood_id);
    assert!(state.queue.lock().unwrap().current().is_none());
}
```

```rust
#[test]
fn build_listening_stats_groups_play_counts_by_category_skipping_unknown_moods() {
    let state = test_state();
    let mood = state.moods.list()[0].clone();

    state
        .db
        .lock()
        .unwrap()
        .start_session(&[(mood.id.clone(), 100)])
        .unwrap();
    state
        .db
        .lock()
        .unwrap()
        .start_session(&[("not-a-real-mood".to_string(), 100)])
        .unwrap();

    let stats = build_listening_stats(&state).unwrap();

    assert_eq!(stats.total_sessions, 2);
    assert_eq!(stats.category_breakdown.len(), 1);
    assert_eq!(stats.category_breakdown[0].category, mood.category);
    assert_eq!(stats.category_breakdown[0].session_count, 1.0);
}
```

Add `use crate::models::CategoryBreakdown;` if not already imported (it's
already in the existing `use crate::models::{...}` line — just confirm
`CategoryBreakdown` is present).

- [ ] **Step 9: Fix `commands/queue.rs` to compile (temporary single-mood read)**

`ensure_queue_topped_up` reads the first mood of the current session — this
is provisional, Task 2 generalizes it to the whole mix:

```rust
pub async fn ensure_queue_topped_up(state: State<'_, AppState>) -> Result<()> {
    let needs_more = state.queue.lock().unwrap().upcoming().len() < LOW_WATERMARK;
    if !needs_more {
        return Ok(());
    }

    let mood_id = {
        let db = state.db.lock().unwrap();
        match db.current_session()? {
            Some(session) => session.moods[0].mood_id.clone(),
            None => return Ok(()),
        }
    };

    super::top_up_queue(&state, &mood_id).await
}
```

Fix its test call site:

```rust
#[test]
fn make_room_for_single_track_ends_an_active_session_and_clears_its_queue() {
    let state = test_state();
    let mood_id = state.moods.list()[0].id.clone();
    state
        .db
        .lock()
        .unwrap()
        .start_session(&[(mood_id, 100)])
        .unwrap();
    state
        .queue
        .lock()
        .unwrap()
        .add_candidates([track("a"), track("b")]);

    make_room_for_single_track(&state).unwrap();

    assert!(
        state
            .db
            .lock()
            .unwrap()
            .current_session()
```
(rest of that test body is unchanged — only the `start_session` call line changes)

- [ ] **Step 10: Run the full backend test suite**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, zero warnings. Behavior is unchanged end-to-end (every
session is still single-mood in practice) — this step only proves the
refactor didn't break anything.

- [ ] **Step 11: Commit**

```bash
git add src-tauri/migrations/0003_mixed_sessions.sql src-tauri/src/db/mod.rs \
  src-tauri/src/models.rs src-tauri/src/error.rs src-tauri/src/db/sessions.rs \
  src-tauri/src/db/scoring_signals.rs src-tauri/src/commands/session.rs \
  src-tauri/src/commands/queue.rs
git commit -m "refactor(sessions): store session moods as a weighted list"
```

---

### Task 2: Mood mixing — engine and commands

Adds the actual mixing capability on top of Task 1's storage: the mood
engine splits its query budget across a mix, and a new `start_mixed_session`
command lets the frontend start one.

**Files:**
- Modify: `src-tauri/src/mood_engine/candidates.rs` (new `query_counts_for_weights`)
- Modify: `src-tauri/src/mood_engine/mod.rs` (`generate_candidates` → `generate_mixed_candidates`)
- Modify: `src-tauri/src/commands/mod.rs` (`top_up_queue`, `start_session_and_play` generalized to slices)
- Modify: `src-tauri/src/commands/session.rs` (`start_session_impl` generalized; new `start_mixed_session` command)
- Modify: `src-tauri/src/commands/mood.rs` (`surprise_me` call site)
- Modify: `src-tauri/src/commands/queue.rs` (`ensure_queue_topped_up` passes the full mix, not just the first mood)
- Modify: `src-tauri/src/lib.rs` (register `start_mixed_session`)

**Interfaces:**
- Consumes: `models::SessionMood`, `Db::start_session(&[(String,u8)])`, `EchoraError::InvalidMoodMix` (Task 1).
- Produces: `mood_engine::generate_mixed_candidates(moods: &[(&Mood, u8)], ...) -> Result<Vec<Track>>`; Tauri command `start_mixed_session(moods: Vec<SessionMood>) -> Result<SessionInfo>`.

- [ ] **Step 1: Write the failing test for query-budget distribution**

In `src-tauri/src/mood_engine/candidates.rs`, add to `mod tests`:

```rust
#[test]
fn query_counts_for_weights_never_leaves_a_mood_at_zero() {
    let counts = query_counts_for_weights(6, &[70, 20, 10]);
    assert_eq!(counts.len(), 3);
    assert!(counts.iter().all(|&c| c >= 1));
}

#[test]
fn query_counts_for_weights_is_proportional_for_a_dominant_mood() {
    let counts = query_counts_for_weights(10, &[80, 20]);
    assert!(counts[0] > counts[1]);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd src-tauri && cargo test mood_engine::candidates::tests::query_counts_for_weights`
Expected: FAIL with "cannot find function `query_counts_for_weights`"

- [ ] **Step 3: Implement `query_counts_for_weights`**

In `src-tauri/src/mood_engine/candidates.rs`, add above `dedup`:

```rust
/// Splits `total_budget` search-query slots across moods proportional to
/// their weight, rounding each share and guaranteeing every mood gets at
/// least 1 — a small budget or a low-weight mood in a big mix must still get
/// a chance to contribute.
pub fn query_counts_for_weights(total_budget: usize, weights: &[u8]) -> Vec<usize> {
    weights
        .iter()
        .map(|&weight| {
            let share = ((total_budget as f32) * (weight as f32) / 100.0).round() as usize;
            share.max(1)
        })
        .collect()
}
```

- [ ] **Step 4: Run it to verify it passes**

Run: `cd src-tauri && cargo test mood_engine::candidates::tests::query_counts_for_weights`
Expected: PASS

- [ ] **Step 5: Replace `generate_candidates` with `generate_mixed_candidates`**

In `src-tauri/src/mood_engine/mod.rs`, delete `generate_candidates` and add:

```rust
/// The core mood-engine flow, mix-aware: splits the round's query budget
/// across 1-3 moods proportional to weight (see
/// `candidates::query_counts_for_weights`), searches, dedups, scores,
/// shuffles. Same partial-failure rule as before: the last error is
/// propagated only if every query across every mood in the mix failed.
pub async fn generate_mixed_candidates(
    moods: &[(&Mood, u8)],
    resolver: &Resolver,
    ctx: &ScoringContext,
    config: &GenerationConfig,
    rng: &mut impl Rng,
) -> Result<Vec<Track>> {
    let total_budget = config.queries_per_round * moods.len().max(1);
    let weights: Vec<u8> = moods.iter().map(|(_, weight)| *weight).collect();
    let counts = candidates::query_counts_for_weights(total_budget, &weights);

    let mut raw = Vec::new();
    let mut last_err = None;
    for ((mood, _), count) in moods.iter().copied().zip(counts) {
        for query in candidates::select_queries(mood, count, rng) {
            match resolver.search(&query, config.results_per_query).await {
                Ok(tracks) => raw.extend(tracks),
                Err(err) => last_err = Some(err),
            }
        }
    }

    if raw.is_empty()
        && let Some(err) = last_err
    {
        return Err(err);
    }

    let deduped = candidates::dedup(raw);
    Ok(scoring::shuffle_by_score(deduped, ctx, rng))
}
```

Update the (ignored, real-network) smoke test in the same file:

```rust
    #[tokio::test]
    #[ignore]
    async fn generating_candidates_for_a_real_mood_returns_deduped_scored_tracks() {
        let dev_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        let resolver = Resolver::new(ResolverConfig {
            yt_dlp_path: dev_dir.join("yt-dlp_linux"),
            deno_path: dev_dir.join("deno"),
            timeout: Duration::from_secs(30),
        });
        let catalog = MoodCatalog::load().unwrap();
        let mood = catalog
            .get("villain")
            .expect("the bundled catalog should have a 'villain' mood");

        let ctx = ScoringContext::default();
        let config = GenerationConfig::default();
        let mut rng = StdRng::seed_from_u64(1);

        let candidates =
            generate_mixed_candidates(&[(mood, 100)], &resolver, &ctx, &config, &mut rng)
                .await
                .unwrap();

        assert!(!candidates.is_empty());
        let mut ids: Vec<_> = candidates.iter().map(|t| t.id.clone()).collect();
        let before_dedup_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            before_dedup_len,
            "candidates should already be deduped"
        );
    }
```

- [ ] **Step 6: Generalize `commands/mod.rs` to mixes**

```rust
use crate::error::Result;
use crate::models::{Mood, SessionInfo, Track};
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// Generates a fresh batch of candidates for `moods` (1-3 weighted moods)
/// and appends them to the queue. Shared by starting a mixed session and
/// topping the queue back up mid-session — both are "get more candidates
/// for this mix," just triggered at different times.
pub(crate) async fn top_up_queue(state: &AppState, moods: &[(String, u8)]) -> Result<()> {
    let resolved: Vec<(&Mood, u8)> = moods
        .iter()
        .map(|(mood_id, weight)| state.moods.get(mood_id).map(|m| (m, *weight)))
        .collect::<Result<_>>()?;
    let config = GenerationConfig::default();
    let ctx = {
        let db = state.db.lock().unwrap();
        mood_engine::build_scoring_context(&db, config.recent_session_window)?
    };

    // A plain `StdRng`, not the thread-local `rand::rng()` — Tauri's async
    // commands require `Send` futures, and `ThreadRng` (`Rc`-based) isn't.
    let mut rng = StdRng::from_rng(&mut rand::rng());
    let candidates =
        mood_engine::generate_mixed_candidates(&resolved, &state.resolver, &ctx, &config, &mut rng)
            .await?;
    state.queue.lock().unwrap().add_candidates(candidates);
    Ok(())
}
```

(`resolve_and_load`, `toggle_play_pause`, `record_current_completion` stay
unchanged.)

```rust
/// Starts a session for `moods`, fetches its first batch of candidates, and
/// immediately starts playing the first one — shared by `start_mood_session`,
/// `start_mixed_session`, and `surprise_me`, which only differ in how they
/// pick `moods`.
pub(crate) async fn start_session_and_play(
    state: &AppState,
    moods: &[(String, u8)],
) -> Result<SessionInfo> {
    let session = crate::commands::session::start_session_impl(state, moods)?;
    top_up_queue(state, moods).await?;

    let current = state.queue.lock().unwrap().current().cloned();
    if let Some(track) = current {
        resolve_and_load(state, &track).await?;
    }
    Ok(session)
}
```

- [ ] **Step 7: Generalize `commands/session.rs`**

`start_session_impl` now takes the slice directly, validating every mood id
exists before touching the db:

```rust
pub(crate) fn start_session_impl(state: &AppState, moods: &[(String, u8)]) -> Result<SessionInfo> {
    for (mood_id, _) in moods {
        state.moods.get(mood_id)?;
    }
    let session = state.db.lock().unwrap().start_session(moods)?;
    state.queue.lock().unwrap().clear();
    Ok(session)
}
```

Its two existing commands adapt to pass one-element slices:

```rust
#[tauri::command]
pub fn start_session(state: State<AppState>, mood_id: String) -> Result<SessionInfo> {
    start_session_impl(&state, &[(mood_id, 100)])
}

#[tauri::command]
pub async fn start_mood_session(
    state: State<'_, AppState>,
    mood_id: String,
) -> Result<SessionInfo> {
    super::start_session_and_play(&state, &[(mood_id, 100)]).await
}
```

New command, reusing `SessionMood` as the IPC input shape (it already
derives `Deserialize` from Task 1):

```rust
#[tauri::command]
pub async fn start_mixed_session(
    state: State<'_, AppState>,
    moods: Vec<SessionMood>,
) -> Result<SessionInfo> {
    let pairs: Vec<(String, u8)> = moods.into_iter().map(|m| (m.mood_id, m.weight)).collect();
    super::start_session_and_play(&state, &pairs).await
}
```

Add `SessionMood` to this file's `use crate::models::{...}` import line.

Fix the remaining test call sites in the same file:

```rust
#[test]
fn starting_a_session_with_an_unknown_mood_errors() {
    let state = test_state();
    let err = start_session_impl(&state, &[("not-a-real-mood".to_string(), 100)]).unwrap_err();
    assert!(matches!(err, EchoraError::UnknownMood(_)));
}
```

```rust
#[test]
fn starting_a_session_with_a_known_mood_clears_the_queue() {
    let state = test_state();
    let mood_id = state.moods.list()[0].id.clone();
    state.queue.lock().unwrap().add_candidates([Track {
        id: "leftover".into(),
        title: "leftover".into(),
        artist: None,
        duration_seconds: None,
        thumbnail_url: None,
    }]);

    let session = start_session_impl(&state, &[(mood_id.clone(), 100)]).unwrap();

    assert_eq!(session.moods.len(), 1);
    assert_eq!(session.moods[0].mood_id, mood_id);
    assert!(state.queue.lock().unwrap().current().is_none());
}
```

```rust
#[test]
fn ending_the_active_session_clears_the_queue() {
    let state = test_state();
    let mood_id = state.moods.list()[0].id.clone();
    start_session_impl(&state, &[(mood_id, 100)]).unwrap();
    state.queue.lock().unwrap().add_candidates([Track {
        id: "a".into(),
        title: "a".into(),
        artist: None,
        duration_seconds: None,
        thumbnail_url: None,
    }]);

    end_session_impl(&state).unwrap();

    assert!(state.queue.lock().unwrap().current().is_none());
    assert!(
        state
            .db
            .lock()
            .unwrap()
            .current_session()
            .unwrap()
            .is_none()
    );
}
```

- [ ] **Step 8: Fix `commands/mood.rs`**

```rust
#[tauri::command]
pub async fn surprise_me(state: State<'_, AppState>) -> Result<SessionInfo> {
    let moods = state.moods.list();
    let mood_id = {
        let db = state.db.lock().unwrap();
        let favorited = db.list_favorite_moods()?.into_iter().collect();
        let recently_played = db.recent_mood_ids(5)?;
        let mut rng = rand::rng();
        let picked = surprise::pick_surprise_mood(&moods, &favorited, &recently_played, &mut rng)?;
        picked.id.clone()
    };

    super::start_session_and_play(&state, &[(mood_id, 100)]).await
}
```

- [ ] **Step 9: Generalize `ensure_queue_topped_up` to the full mix**

```rust
pub async fn ensure_queue_topped_up(state: State<'_, AppState>) -> Result<()> {
    let needs_more = state.queue.lock().unwrap().upcoming().len() < LOW_WATERMARK;
    if !needs_more {
        return Ok(());
    }

    let moods = {
        let db = state.db.lock().unwrap();
        match db.current_session()? {
            Some(session) => session
                .moods
                .into_iter()
                .map(|m| (m.mood_id, m.weight))
                .collect::<Vec<_>>(),
            None => return Ok(()),
        }
    };

    super::top_up_queue(&state, &moods).await
}
```

- [ ] **Step 10: Register the new command**

In `src-tauri/src/lib.rs`, add to `invoke_handler(tauri::generate_handler![...])`
right after `commands::session::start_mood_session,`:

```rust
            commands::session::start_mixed_session,
```

- [ ] **Step 11: Run the full backend test suite**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, zero warnings. Backend now fully supports mixed sessions
end-to-end via `start_mixed_session`, even though nothing calls it yet.

- [ ] **Step 12: Commit**

```bash
git add src-tauri/src/mood_engine/candidates.rs src-tauri/src/mood_engine/mod.rs \
  src-tauri/src/commands/mod.rs src-tauri/src/commands/session.rs \
  src-tauri/src/commands/mood.rs src-tauri/src/commands/queue.rs src-tauri/src/lib.rs
git commit -m "feat(mood-mixing): generate candidates from a weighted mood mix"
```

---

### Task 3: Frontend — session types and mixed-session display

Makes every existing frontend consumer of session data handle the new
`moods` array correctly, and adds the API wrapper + action handler for
starting a mix. No new UI to trigger it yet (Task 5) — this task is the data
layer only.

**Files:**
- Modify: `src/lib/api.ts` (`SessionMood`, `SessionInfo`/`SessionSummary`, `startMixedSession`)
- Modify: `src/App.tsx` (`currentMoods` state, `handleStartMix`, `currentMoodName` derivation)
- Modify: `src/components/DiscoverView.tsx` (`onStartMix` prop passthrough)
- Modify: `src/components/LibraryTab.tsx` (`onStartMix` prop, mixed session-history rows, `play_count` rounding)

**Interfaces:**
- Consumes: Tauri command `start_mixed_session` (Task 2).
- Produces: `api.startMixedSession(moods: SessionMood[]) => Promise<SessionInfo>`; `App`'s `handleStartMix(moods: SessionMood[]) => Promise<void>`, passed down as `onStartMix`.

- [ ] **Step 1: Update `src/lib/api.ts` types and add the wrapper**

Replace the `SessionInfo`/`SessionSummary` block:

```ts
export interface SessionMood {
  mood_id: string;
  weight: number;
}

export interface SessionInfo {
  id: number;
  moods: SessionMood[];
  started_at: number;
  ended_at: number | null;
}

export interface SessionSummary extends SessionInfo {
  track_count: number;
}
```

Add the wrapper next to `startMoodSession`:

```ts
  startMoodSession: (moodId: string) => call<SessionInfo>("start_mood_session", { moodId }),
  startMixedSession: (moods: SessionMood[]) => call<SessionInfo>("start_mixed_session", { moods }),
```

- [ ] **Step 2: Run the frontend typecheck to confirm the intentional breakage**

Run: `npm run build`
Expected: FAIL — `App.tsx`, `LibraryTab.tsx` still reference `.mood_id` on
`SessionInfo`/`SessionSummary`, which no longer exists

- [ ] **Step 3: Update `src/App.tsx`**

Replace the `currentMoodId` state and its four call sites, and add
`handleStartMix`:

```tsx
import type { SessionMood, Track } from "./lib/api";

function App() {
  const [view, setView] = useState<View>("home");
  const [playerExpanded, setPlayerExpanded] = useState(false);
  const [startingMoodId, setStartingMoodId] = useState<string | null>(null);
  const [startingTrackId, setStartingTrackId] = useState<string | null>(null);
  const [currentMoods, setCurrentMoods] = useState<SessionMood[] | null>(null);
  const [globalError, setGlobalError] = useState<string | null>(null);
  const [sceneSaveTick, setSceneSaveTick] = useState(0);

  const playback = usePlayback();
  const moodsData = useMoods();

  const reportError = useCallback((message: string) => setGlobalError(message), []);
  const onSceneSaved = useCallback(() => setSceneSaveTick((t) => t + 1), []);

  const handleStartMood = useCallback(
    async (moodId: string) => {
      setStartingMoodId(moodId);
      try {
        const session = await api.startMoodSession(moodId);
        setCurrentMoods(session.moods);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingMoodId(null);
      }
    },
    [playback, reportError],
  );

  const handleStartMix = useCallback(
    async (moods: SessionMood[]) => {
      setStartingMoodId("mix");
      try {
        const session = await api.startMixedSession(moods);
        setCurrentMoods(session.moods);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingMoodId(null);
      }
    },
    [playback, reportError],
  );

  const handlePlayTrack = useCallback(
    async (track: Track) => {
      setStartingTrackId(track.id);
      try {
        await api.playSingleTrack(track);
        setCurrentMoods(null);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingTrackId(null);
      }
    },
    [playback, reportError],
  );

  const handlePlayScene = useCallback(
    async (sceneId: number) => {
      setStartingTrackId(`scene-${sceneId}`);
      try {
        await api.playScene(sceneId);
        setCurrentMoods(null);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingTrackId(null);
      }
    },
    [playback, reportError],
  );

  const handleSurpriseMe = useCallback(async () => {
    setStartingMoodId("surprise");
    try {
      const session = await api.surpriseMe();
      setCurrentMoods(session.moods);
      await playback.refreshQueue();
      setPlayerExpanded(true);
    } catch (err) {
      reportError(messageOf(err));
    } finally {
      setStartingMoodId(null);
    }
  }, [playback, reportError]);

  const currentMoodName = currentMoods
    ? currentMoods
        .map((m) => moodsData.moods.find((mood) => mood.id === m.mood_id)?.name ?? "Unknown mood")
        .join(" + ")
    : null;
```

And thread `onStartMix` through to `DiscoverView` (the `HomeView` prop is
added in Task 5, once the mix UI exists):

```tsx
        {view === "discover" ? (
          <DiscoverView
            moodsData={moodsData}
            onError={reportError}
            startingMoodId={startingMoodId}
            startingTrackId={startingTrackId}
            onStartMood={handleStartMood}
            onStartMix={handleStartMix}
            onPlayTrack={handlePlayTrack}
            onPlayScene={handlePlayScene}
            sceneSaveTick={sceneSaveTick}
          />
        ) : null}
```

- [ ] **Step 4: Thread `onStartMix` through `DiscoverView`**

```tsx
interface DiscoverViewProps {
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onStartMix: (moods: SessionMood[]) => void;
  onPlayTrack: (track: Track) => void;
  onPlayScene: (sceneId: number) => void;
  sceneSaveTick: number;
}

export function DiscoverView({
  moodsData,
  onError,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onStartMix,
  onPlayTrack,
  onPlayScene,
  sceneSaveTick,
}: DiscoverViewProps) {
```

(add `import type { SessionMood, Track } from "../lib/api";`) and pass it
into `LibraryTab`:

```tsx
        <LibraryTab
          discover={discover}
          moods={moodsData.moods}
          startingMoodId={startingMoodId}
          startingTrackId={startingTrackId}
          onStartMood={onStartMood}
          onStartMix={onStartMix}
          onPlayTrack={onPlayTrack}
          onPlayScene={onPlayScene}
          onError={onError}
        />
```

- [ ] **Step 5: Update `LibraryTab.tsx`**

```tsx
import type { MoodSummary, SceneSummary, SessionMood, Track } from "../lib/api";

interface LibraryTabProps {
  discover: ReturnType<typeof useDiscover>;
  moods: MoodSummary[];
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onStartMix: (moods: SessionMood[]) => void;
  onPlayTrack: (track: Track) => void;
  onPlayScene: (sceneId: number) => void;
  onError: (message: string) => void;
}
```

Add `onStartMix` to the destructured props, then update the Most Played
Moods row (rounds the now-fractional `play_count` for display) and the
Session History row (mixed sessions restart via `onStartMix`, single-mood
ones keep using `onStartMood`):

```tsx
            <span className="library-row__meta">{Math.round(entry.play_count)} sessions</span>
```

```tsx
      <section className="library-section">
        <h2 className="mood-row__title">Session History</h2>
        {history.length === 0 ? (
          <EmptySection message="No sessions yet" />
        ) : (
          history.map((session) => (
            <button
              key={session.id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() =>
                session.moods.length > 1
                  ? onStartMix(session.moods)
                  : onStartMood(session.moods[0].mood_id)
              }
            >
              <span className="library-row__title">
                {session.moods.map((m) => moodName(m.mood_id)).join(" + ")}
              </span>
              <span className="library-row__meta">
                {formatDate(session.started_at)} · {session.track_count} tracks
              </span>
            </button>
          ))
        )}
      </section>
```

- [ ] **Step 6: Verify frontend build**

Run: `npm run lint && npm run build`
Expected: PASS, zero errors

- [ ] **Step 7: Commit**

```bash
git add src/lib/api.ts src/App.tsx src/components/DiscoverView.tsx src/components/LibraryTab.tsx
git commit -m "feat(mood-mixing): wire mixed-session types through the frontend"
```

---

### Task 4: `MoodCard` multi-select and `MoodMixBar`

Two additive UI pieces, buildable and reviewable independently of Home's
actual mix-mode flow (Task 5 wires them in).

**Files:**
- Modify: `src/components/MoodCard.tsx` (new `selected` prop)
- Create: `src/components/MoodMixBar.tsx`
- Modify: `src/styles.css` (`.mood-card.is-selected`, `.mood-mix-bar*`)

**Interfaces:**
- Consumes: `MoodSummary` (existing), `SessionMood`-shaped weight tuples.
- Produces: `<MoodCard selected? />`; `<MoodMixBar moods weights onChangeWeights onStart busy />`, calling `onChangeWeights(next: number[])` and `onStart()`.

- [ ] **Step 1: Add the `selected` prop to `MoodCard`**

```tsx
import { CATEGORY_COLOR } from "../lib/categories";
import type { MoodSummary } from "../lib/api";

interface MoodCardProps {
  mood: MoodSummary;
  favorited?: boolean;
  loading?: boolean;
  disabled?: boolean;
  selected?: boolean;
  onSelect: (moodId: string) => void;
}

export function MoodCard({ mood, favorited, loading, disabled, selected, onSelect }: MoodCardProps) {
  return (
    <button
      type="button"
      className={`mood-card${favorited ? " is-favorited" : ""}${loading ? " is-loading" : ""}${selected ? " is-selected" : ""}`}
      disabled={disabled}
      onClick={() => onSelect(mood.id)}
    >
      {loading ? (
        <span className="mood-card__spinner" aria-hidden="true" />
      ) : (
        <span
          className="mood-card__dot"
          style={{ background: CATEGORY_COLOR[mood.category] ?? "oklch(70% 0.05 290)" }}
          aria-hidden="true"
        />
      )}
      <span className="mood-card__name">{mood.name}</span>
    </button>
  );
}
```

- [ ] **Step 2: Write `MoodMixBar`**

`src/components/MoodMixBar.tsx` — two native range sliders instead of a
custom multi-handle drag bar (`ponytail:` simplification; a fancier
draggable 3-segment bar would need custom pointer-event math for the same
result, add it if 2 sliders prove hard to use in practice):

```tsx
import { useCallback } from "react";
import type { MoodSummary } from "../lib/api";

interface MoodMixBarProps {
  moods: MoodSummary[];
  weights: number[];
  onChangeWeights: (weights: number[]) => void;
  onStart: () => void;
  busy: boolean;
}

export function MoodMixBar({ moods, weights, onChangeWeights, onStart, busy }: MoodMixBarProps) {
  const handleFirstChange = useCallback(
    (value: number) => {
      if (moods.length === 2) {
        onChangeWeights([value, 100 - value]);
        return;
      }
      const remainder = 100 - value;
      const previousRemainder = weights[1] + weights[2];
      const secondShare = previousRemainder > 0 ? weights[1] / previousRemainder : 0.5;
      const second = Math.round(remainder * secondShare);
      onChangeWeights([value, second, remainder - second]);
    },
    [moods.length, weights, onChangeWeights],
  );

  const handleSecondChange = useCallback(
    (value: number) => {
      const remainder = 100 - weights[0];
      onChangeWeights([weights[0], value, remainder - value]);
    },
    [weights, onChangeWeights],
  );

  return (
    <div className="mood-mix-bar">
      <div className="mood-mix-bar__chips">
        {moods.map((mood, i) => (
          <span className="mood-mix-bar__chip" key={mood.id}>
            {mood.name} · {weights[i]}%
          </span>
        ))}
      </div>

      <input
        type="range"
        min={1}
        max={moods.length === 2 ? 99 : 98}
        value={weights[0]}
        disabled={busy}
        onChange={(e) => handleFirstChange(Number(e.target.value))}
        aria-label={`${moods[0]?.name ?? "first mood"} weight`}
        className="mood-mix-bar__slider"
      />
      {moods.length === 3 ? (
        <input
          type="range"
          min={0}
          max={100 - weights[0]}
          value={weights[1]}
          disabled={busy}
          onChange={(e) => handleSecondChange(Number(e.target.value))}
          aria-label={`${moods[1]?.name ?? "second mood"} vs ${moods[2]?.name ?? "third mood"} split`}
          className="mood-mix-bar__slider"
        />
      ) : null}

      <button type="button" className="mood-mix-bar__start" disabled={busy} onClick={onStart}>
        {busy ? "Starting…" : "Start Mix"}
      </button>
    </div>
  );
}
```

- [ ] **Step 3: Add CSS**

In `src/styles.css`, after the `.mood-card__name` block:

```css
.mood-card.is-selected {
  border: 1px solid var(--accent);
  background: var(--bg-elevated-hover);
}

.mood-mix-bar {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px;
  border-radius: var(--radius-md);
  background: var(--bg-elevated);
  margin-bottom: 12px;
}
.mood-mix-bar__chips {
  display: flex;
  gap: 8px;
  flex-wrap: wrap;
}
.mood-mix-bar__chip {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
  background: var(--bg-elevated-hover);
  border-radius: var(--radius-sm);
  padding: 4px 10px;
}
.mood-mix-bar__slider {
  width: 100%;
  accent-color: var(--accent);
}
.mood-mix-bar__start {
  align-self: flex-end;
  min-height: 44px;
  padding: 0 20px;
  border-radius: var(--radius-sm);
  background: var(--accent);
  color: var(--bg-base);
  font-weight: 600;
}
.mood-mix-bar__start:disabled {
  opacity: 0.6;
}
```

- [ ] **Step 4: Verify frontend build**

Run: `npm run lint && npm run build`
Expected: PASS, zero errors (nothing renders `MoodMixBar` yet — Task 5 does — so this only proves it compiles cleanly in isolation)

- [ ] **Step 5: Commit**

```bash
git add src/components/MoodCard.tsx src/components/MoodMixBar.tsx src/styles.css
git commit -m "feat(mood-mixing): add MoodCard selection state and MoodMixBar"
```

---

### Task 5: `HomeView` — mix mode

Wires Tasks 3 and 4 into the actual "pick 2-3 moods, set weights, start"
flow. This is the last task — after it, Mood Mixing is fully usable end to
end.

**Files:**
- Modify: `src/components/HomeView.tsx` (mix-mode toggle, selection, `MoodMixBar` integration)
- Modify: `src/App.tsx` (pass `onStartMix` into `HomeView`)
- Modify: `src/styles.css` (`.mix-toggle`)

**Interfaces:**
- Consumes: `MoodCard`'s `selected` prop, `MoodMixBar` (Task 4); `handleStartMix` (Task 3).
- Produces: none — this is the leaf of the feature.

- [ ] **Step 1: Rewrite `HomeView.tsx`**

```tsx
import { useEffect, useState } from "react";
import type { useMoods } from "../hooks/useMoods";
import { MoodCard } from "./MoodCard";
import { MoodMixBar } from "./MoodMixBar";
import { SparkleIcon, ChevronRightIcon } from "./icons";
import { CATEGORY_LABEL } from "../lib/categories";
import type { MoodSummary, SessionMood } from "../lib/api";

interface HomeViewProps {
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  onStartMood: (moodId: string) => void;
  onStartMix: (moods: SessionMood[]) => void;
  onSurpriseMe: () => void;
}

function evenWeights(count: number): number[] {
  const base = Math.floor(100 / count);
  const weights = new Array(count).fill(base);
  weights[count - 1] = 100 - base * (count - 1);
  return weights;
}

export function HomeView({
  moodsData,
  onError,
  startingMoodId,
  onStartMood,
  onStartMix,
  onSurpriseMe,
}: HomeViewProps) {
  const { moods, favoriteMoodIds, recentMoodIds, loading, error } = moodsData;
  const [mixMode, setMixMode] = useState(false);
  const [selectedMoodIds, setSelectedMoodIds] = useState<string[]>([]);
  const [weights, setWeights] = useState<number[]>([]);

  useEffect(() => {
    if (error) onError(error);
  }, [error, onError]);

  const busy = startingMoodId !== null;

  const forYouIds = [...favoriteMoodIds, ...recentMoodIds];
  const moodsById = new Map(moods.map((m) => [m.id, m]));
  const forYouMoods = forYouIds.map((id) => moodsById.get(id)).filter((m): m is MoodSummary => !!m);

  const categories: { key: string; moods: MoodSummary[] }[] = [];
  for (const mood of moods) {
    let bucket = categories.find((c) => c.key === mood.category);
    if (!bucket) {
      bucket = { key: mood.category, moods: [] };
      categories.push(bucket);
    }
    bucket.moods.push(mood);
  }

  const toggleMoodSelection = (moodId: string) => {
    setSelectedMoodIds((current) => {
      if (current.includes(moodId)) {
        const next = current.filter((id) => id !== moodId);
        setWeights(evenWeights(Math.max(next.length, 1)));
        return next;
      }
      if (current.length >= 3) return current;
      const next = [...current, moodId];
      setWeights(evenWeights(next.length));
      return next;
    });
  };

  const exitMixMode = () => {
    setMixMode(false);
    setSelectedMoodIds([]);
    setWeights([]);
  };

  const handleStartMix = () => {
    onStartMix(selectedMoodIds.map((id, i) => ({ mood_id: id, weight: weights[i] })));
    exitMixMode();
  };

  const selectedMoods = selectedMoodIds
    .map((id) => moodsById.get(id))
    .filter((m): m is MoodSummary => !!m);

  const renderMoodCard = (mood: MoodSummary) => (
    <MoodCard
      key={mood.id}
      mood={mood}
      favorited={favoriteMoodIds.has(mood.id)}
      loading={startingMoodId === mood.id}
      disabled={busy}
      selected={mixMode && selectedMoodIds.includes(mood.id)}
      onSelect={mixMode ? toggleMoodSelection : onStartMood}
    />
  );

  return (
    <div className="home-view">
      <button
        type="button"
        className="surprise-banner"
        onClick={onSurpriseMe}
        disabled={busy || mixMode}
        aria-label="Surprise me — let Echora pick your mood"
      >
        <span className="surprise-banner__label">
          <SparkleIcon />
          <span>
            <div className="surprise-banner__title">
              {busy && startingMoodId === "surprise" ? "Picking…" : "Surprise Me"}
            </div>
            <div className="surprise-banner__subtitle">Let Echora pick your mood</div>
          </span>
        </span>
        <ChevronRightIcon />
      </button>

      <button
        type="button"
        className={`mix-toggle${mixMode ? " is-active" : ""}`}
        onClick={() => (mixMode ? exitMixMode() : setMixMode(true))}
        disabled={busy}
      >
        {mixMode ? "Cancel mix" : "Mix moods"}
      </button>

      {mixMode && selectedMoods.length >= 2 ? (
        <MoodMixBar
          moods={selectedMoods}
          weights={weights}
          onChangeWeights={setWeights}
          onStart={handleStartMix}
          busy={busy}
        />
      ) : null}

      {loading ? (
        <div className="mood-row">
          <div className="mood-row__scroll">
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
            <div className="skeleton" style={{ width: 148, height: 88, borderRadius: 16 }} />
          </div>
        </div>
      ) : (
        <>
          {forYouMoods.length > 0 ? (
            <div className="mood-row">
              <h2 className="mood-row__title">For You</h2>
              <div className="mood-row__scroll">{forYouMoods.map(renderMoodCard)}</div>
            </div>
          ) : null}

          {categories.map((category) => (
            <div className="mood-row" key={category.key}>
              <h2 className="mood-row__title">{CATEGORY_LABEL[category.key] ?? category.key}</h2>
              <div className="mood-row__scroll">{category.moods.map(renderMoodCard)}</div>
            </div>
          ))}
        </>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Wire `onStartMix` into `HomeView` from `App.tsx`**

```tsx
        {view === "home" ? (
          <HomeView
            moodsData={moodsData}
            onError={reportError}
            startingMoodId={startingMoodId}
            onStartMood={handleStartMood}
            onStartMix={handleStartMix}
            onSurpriseMe={handleSurpriseMe}
          />
        ) : null}
```

- [ ] **Step 3: Add `.mix-toggle` CSS**

In `src/styles.css`, after the `.surprise-banner__subtitle` block:

```css
.mix-toggle {
  min-height: 44px;
  padding: 0 16px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 600;
  margin-bottom: 12px;
  align-self: flex-start;
}
.mix-toggle.is-active {
  border-color: var(--accent);
  color: var(--text-primary);
}
.mix-toggle:disabled {
  opacity: 0.5;
}
```

- [ ] **Step 4: Verify frontend build**

Run: `npm run lint && npm run build`
Expected: PASS, zero errors

- [ ] **Step 5: Manual check (documented, not automated — no Tauri window in this environment)**

The plan's author cannot launch `npm run tauri dev` in this sandbox. Whoever
runs this task last should manually verify in a real window: tap "Mix
moods", select 2 then 3 moods, drag both sliders, confirm the weights always
sum to 100 and each stays ≥1%, tap "Start Mix", confirm the player shows
"MoodA + MoodB(+ MoodC)" and the queue is non-empty. Then check a mixed
session shows correctly in Discover's Library tab (Session History row,
joined name) and tapping it restarts the same mix.

- [ ] **Step 6: Commit**

```bash
git add src/components/HomeView.tsx src/App.tsx src/styles.css
git commit -m "feat(mood-mixing): add mix mode to Home"
```
