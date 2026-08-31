# Discover + Statistics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a "Discover" screen with two tabs — a browsable Library (session
history, favorite moods, favorite tracks, most-played moods, all clickable)
and Statistics (listening-time/session/mood number cards) — as personal
insight panels that never influence mood-engine scoring.

**Architecture:** Backend: three new read-only `Db` query methods (no new
tables/migration) plus three new Tauri commands, plus one new command
(`play_single_track`) that fixes a real gap — there is currently no way to
play an arbitrary favorited track while keeping the queue/mini-player/
player-view consistent. Frontend: one new top-level `View`, one new hook
(`useDiscover`), three new components (`DiscoverView`, `LibraryTab`,
`StatsTab`), reusing existing design tokens and patterns (`.queue-row`,
`.segmented`, `EmptyState`) rather than inventing new visual language.

**Tech Stack:** Rust/Tauri 2 (`rusqlite`), React 18 + TypeScript, existing
`src/lib/api.ts` IPC wrapper pattern.

**Spec:** `docs/superpowers/specs/2026-08-31-discover-statistics-design.md`

## Global Constraints

- No new database migration — every field needed already exists in
  `tracks`, `sessions`, `session_tracks`, `track_favorites`,
  `mood_favorites` (`src-tauri/migrations/0001_init.sql`).
- No new dependencies (Rust crate or npm package) for this feature.
- `Db` methods never reference `MoodCatalog` (moods are bundled static
  data per ADR 0008, not a DB concern) — category grouping happens in the
  command layer, not the DB layer.
- Tauri command wrappers stay one-line; testable logic lives in a plain
  `pub(crate) fn`/`pub(crate) async fn` alongside them (the codebase's
  existing `start_session_impl`/`end_session_impl` pattern in
  `commands/session.rs`) so it's unit-testable without a real `tauri::App`.
- No automated frontend tests — vitest isn't installed in this project
  (tracked as a known gap, out of scope here). Frontend tasks are verified
  with `npx tsc --noEmit` and, once fully wired, `npm run lint` +
  `npm run build`.
- Before the feature is considered done: `cargo fmt --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` (from
  `src-tauri/`), `npm run lint`, `npm run build` (from repo root) — all
  must pass, per `CLAUDE.md`.
- UI copy is English (matches every existing screen).

---

## Task 1: `Db::list_favorite_tracks`

**Files:**
- Modify: `src-tauri/src/db/library.rs`

**Interfaces:**
- Produces: `pub fn list_favorite_tracks(&self) -> Result<Vec<Track>>` on
  `Db`, ordered most-recently-favorited first.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in
`src-tauri/src/db/library.rs` (it already has a private `track(id: &str)
-> Track` helper — reuse it, don't redefine it):

```rust
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
```

(Ordering among ties isn't asserted — `favorited_at` is second-precision,
so two favorites in the same test run can tie; the existing
`favoriting_a_mood_lists_it` test already avoids this the same way, via
unfavorite rather than checking order of two simultaneous entries.)

- [ ] **Step 2: Run test to verify it fails**

Run (from `src-tauri/`): `cargo test list_favorite_tracks_returns_favorited_tracks_with_full_fields`
Expected: FAIL with "no method named `list_favorite_tracks` found"

- [ ] **Step 3: Write minimal implementation**

Add to `impl Db` in `src-tauri/src/db/library.rs` (after
`list_favorite_moods`):

```rust
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
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test list_favorite_tracks_returns_favorited_tracks_with_full_fields`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/db/library.rs
git commit -m "feat(db): add list_favorite_tracks query"
```

---

## Task 2: `MoodPlayCount` model + `Db::mood_play_counts`

**Files:**
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/db/sessions.rs`

**Interfaces:**
- Produces: `pub struct MoodPlayCount { pub mood_id: String, pub
  play_count: i64 }` in `models.rs`.
- Produces: `pub fn mood_play_counts(&self) -> Result<Vec<MoodPlayCount>>`
  on `Db`, descending by count.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in
`src-tauri/src/db/sessions.rs` (it already has a private `track(id: &str)
-> Track` helper):

```rust
#[test]
fn mood_play_counts_ranks_moods_by_session_count_descending() {
    let db = Db::open_in_memory().unwrap();
    assert!(db.mood_play_counts().unwrap().is_empty());

    db.start_session("villain").unwrap();
    db.start_session("villain").unwrap();
    db.start_session("focus").unwrap();

    let counts = db.mood_play_counts().unwrap();
    assert_eq!(counts.len(), 2);
    assert_eq!(counts[0].mood_id, "villain");
    assert_eq!(counts[0].play_count, 2);
    assert_eq!(counts[1].mood_id, "focus");
    assert_eq!(counts[1].play_count, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `src-tauri/`): `cargo test mood_play_counts_ranks_moods_by_session_count_descending`
Expected: FAIL with "cannot find type `MoodPlayCount`" / "no method named `mood_play_counts`"

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/models.rs`, after `SessionSummary`:

```rust
/// All-time play count for one mood — the "most played moods" ranking.
#[derive(Debug, Clone, Serialize)]
pub struct MoodPlayCount {
    pub mood_id: String,
    pub play_count: i64,
}
```

Add to `impl Db` in `src-tauri/src/db/sessions.rs`, after `recent_mood_ids`:

```rust
/// All-time play count per mood, descending — the "most played moods"
/// ranking shown in Discover. Unlike `recent_mood_ids` (a bounded recent
/// window, existence-only, used for Surprise Me diversity), this is an
/// unbounded count used for display.
pub fn mood_play_counts(&self) -> Result<Vec<MoodPlayCount>> {
    let mut stmt = self.conn.prepare(
        "SELECT mood_id, COUNT(*) as play_count FROM sessions
         GROUP BY mood_id ORDER BY play_count DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(MoodPlayCount {
            mood_id: r.get(0)?,
            play_count: r.get(1)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
}
```

Update the `use` line at the top of `sessions.rs` from
`use crate::models::{SessionInfo, SessionSummary, Track};` to
`use crate::models::{MoodPlayCount, SessionInfo, SessionSummary, Track};`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test mood_play_counts_ranks_moods_by_session_count_descending`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/db/sessions.rs
git commit -m "feat(db): add mood_play_counts ranking query"
```

---

## Task 3: `ListeningStats`/`CategoryBreakdown` models + `Db::listening_stats`

**Files:**
- Modify: `src-tauri/src/models.rs`
- Modify: `src-tauri/src/db/sessions.rs`

**Interfaces:**
- Consumes: `MoodPlayCount` (Task 2), `self.mood_play_counts()` (Task 2).
- Produces: `pub struct CategoryBreakdown { pub category: String, pub
  session_count: i64 }` and `pub struct ListeningStats { pub
  total_seconds_listened: i64, pub total_sessions: i64, pub
  total_tracks_played: i64, pub top_mood_id: Option<String>, pub
  category_breakdown: Vec<CategoryBreakdown> }` in `models.rs`.
- Produces: `pub fn listening_stats(&self) -> Result<ListeningStats>` on
  `Db` — always returns `category_breakdown: vec![]` (see Task 4 for why).

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/db/sessions.rs`'s test module:

```rust
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
    let session = db.start_session("villain").unwrap();

    let mut half_played = track("a");
    half_played.duration_seconds = Some(200);
    db.record_play(session.id, &half_played, 0, Some(0.5)).unwrap(); // 100s

    let mut unrecorded = track("b");
    unrecorded.duration_seconds = Some(100);
    db.record_play(session.id, &unrecorded, 1, None).unwrap(); // 0s — no completion recorded

    let stats = db.listening_stats().unwrap();
    assert_eq!(stats.total_seconds_listened, 100);
    assert_eq!(stats.total_sessions, 1);
    assert_eq!(stats.total_tracks_played, 2);
    assert_eq!(stats.top_mood_id, Some("villain".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test listening_stats`
Expected: FAIL with "cannot find type `ListeningStats`" / "no method named `listening_stats`"

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/models.rs`, after `MoodPlayCount`:

```rust
/// One category's share of all-time sessions (e.g. "dark", "power").
#[derive(Debug, Clone, Serialize)]
pub struct CategoryBreakdown {
    pub category: String,
    pub session_count: i64,
}

/// Aggregate listening stats shown in Discover's Statistics tab.
/// `category_breakdown` is always empty coming out of `Db` — grouping by
/// category needs `MoodCatalog`, which lives outside the DB layer (see
/// ADR 0008); `commands::session::build_listening_stats` fills it in.
#[derive(Debug, Clone, Serialize)]
pub struct ListeningStats {
    pub total_seconds_listened: i64,
    pub total_sessions: i64,
    pub total_tracks_played: i64,
    pub top_mood_id: Option<String>,
    pub category_breakdown: Vec<CategoryBreakdown>,
}
```

Add to `impl Db` in `src-tauri/src/db/sessions.rs`, after
`mood_play_counts`:

```rust
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
        self.conn.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
    let total_tracks_played: i64 =
        self.conn.query_row("SELECT COUNT(*) FROM session_tracks", [], |r| r.get(0))?;
    let top_mood_id = self.mood_play_counts()?.into_iter().next().map(|m| m.mood_id);

    Ok(ListeningStats {
        total_seconds_listened,
        total_sessions,
        total_tracks_played,
        top_mood_id,
        category_breakdown: Vec::new(),
    })
}
```

Update the `use` line in `sessions.rs` from
`use crate::models::{MoodPlayCount, SessionInfo, SessionSummary, Track};`
(from Task 2) to:
`use crate::models::{ListeningStats, MoodPlayCount, SessionInfo, SessionSummary, Track};`
(`CategoryBreakdown` isn't named in this file — `category_breakdown:
Vec::new()` needs no explicit type import; it's only named directly in
Task 4's `commands/session.rs`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test listening_stats`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/models.rs src-tauri/src/db/sessions.rs
git commit -m "feat(db): add listening_stats aggregate query"
```

---

## Task 4: Tauri commands for library browsing and stats

**Files:**
- Modify: `src-tauri/src/commands/library.rs`
- Modify: `src-tauri/src/commands/session.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `Db::list_favorite_tracks` (Task 1), `Db::mood_play_counts`
  (Task 2), `Db::listening_stats` (Task 3), `MoodCatalog::get(&self, id:
  &str) -> Result<&Mood>` (existing, `src-tauri/src/moods.rs`), `Mood.category:
  String` (existing, `src-tauri/src/models.rs`).
- Produces: `#[tauri::command] list_favorite_tracks`,
  `#[tauri::command] list_most_played_moods`,
  `#[tauri::command] get_listening_stats`, and
  `pub(crate) fn build_listening_stats(state: &AppState) ->
  Result<ListeningStats>` (testable, in `commands/session.rs`).

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block in
`src-tauri/src/commands/session.rs` (it already has a `test_state()`
helper — reuse it, don't redefine it):

```rust
#[test]
fn build_listening_stats_groups_play_counts_by_category_skipping_unknown_moods() {
    let state = test_state();
    let mood = state.moods.list()[0].clone();

    state.db.lock().unwrap().start_session(&mood.id).unwrap();
    state.db.lock().unwrap().start_session("not-a-real-mood").unwrap();

    let stats = build_listening_stats(&state).unwrap();

    assert_eq!(stats.total_sessions, 2);
    assert_eq!(stats.category_breakdown.len(), 1);
    assert_eq!(stats.category_breakdown[0].category, mood.category);
    assert_eq!(stats.category_breakdown[0].session_count, 1);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `src-tauri/`): `cargo test build_listening_stats_groups_play_counts_by_category_skipping_unknown_moods`
Expected: FAIL with "cannot find function `build_listening_stats`"

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/commands/library.rs`, after `list_favorite_moods`:

```rust
#[tauri::command]
pub fn list_favorite_tracks(state: State<AppState>) -> Result<Vec<Track>> {
    state.db.lock().unwrap().list_favorite_tracks()
}
```

Add to `src-tauri/src/commands/session.rs`. First update its imports from
`use crate::models::{SessionInfo, SessionSummary};` to:

```rust
use crate::models::{CategoryBreakdown, ListeningStats, MoodPlayCount, SessionInfo, SessionSummary};
```

Then add, after `clear_history`:

```rust
#[tauri::command]
pub fn list_most_played_moods(state: State<AppState>) -> Result<Vec<MoodPlayCount>> {
    state.db.lock().unwrap().mood_play_counts()
}

/// Plain function (not `#[tauri::command]`) so the category-grouping logic
/// is testable without a real Tauri `App` — mirrors `start_session_impl`.
/// `Db::listening_stats` can't do this grouping itself: it never
/// references `MoodCatalog` (moods are bundled static data, not a DB
/// concern — see ADR 0008), so this command layer maps each `mood_id` in
/// the ranking to its category, silently skipping any id no longer in the
/// loaded catalog (a mood renamed/removed across an app update) rather
/// than failing the whole stats view over one stale id.
pub(crate) fn build_listening_stats(state: &AppState) -> Result<ListeningStats> {
    let mut stats = state.db.lock().unwrap().listening_stats()?;
    let ranking = state.db.lock().unwrap().mood_play_counts()?;

    let mut by_category: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    for entry in ranking {
        if let Ok(mood) = state.moods.get(&entry.mood_id) {
            *by_category.entry(mood.category.clone()).or_insert(0) += entry.play_count;
        }
    }
    let mut breakdown: Vec<CategoryBreakdown> = by_category
        .into_iter()
        .map(|(category, session_count)| CategoryBreakdown { category, session_count })
        .collect();
    breakdown.sort_by(|a, b| b.session_count.cmp(&a.session_count));

    stats.category_breakdown = breakdown;
    Ok(stats)
}

#[tauri::command]
pub fn get_listening_stats(state: State<AppState>) -> Result<ListeningStats> {
    build_listening_stats(&state)
}
```

Register all three new commands in `src-tauri/src/lib.rs`'s
`invoke_handler(tauri::generate_handler![...])` list — add these three
lines anywhere alongside the existing `commands::library::*` /
`commands::session::*` entries:

```rust
commands::library::list_favorite_tracks,
commands::session::list_most_played_moods,
commands::session::get_listening_stats,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test build_listening_stats_groups_play_counts_by_category_skipping_unknown_moods`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/library.rs src-tauri/src/commands/session.rs src-tauri/src/lib.rs
git commit -m "feat(commands): expose favorite tracks and listening stats"
```

---

## Task 5: `play_single_track` command

**Files:**
- Modify: `src-tauri/src/commands/queue.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `commands::session::end_session_impl` (existing,
  `pub(crate)`), `commands::resolve_and_load` (existing, `pub(crate)
  async fn(state: &AppState, track: &Track) -> Result<()>`),
  `Queue::clear`/`Queue::add_candidates` (existing).
- Produces: `pub(crate) fn make_room_for_single_track(state: &AppState)
  -> Result<()>` (testable) and `#[tauri::command] pub async fn
  play_single_track(state: State<'_, AppState>, track: Track) ->
  Result<()>`.

**Why this exists:** `commands::playback::play_track` only resolves a
stream and hands it to mpv — it never touches `state.queue`. Calling it
directly from a favorited-track click would play real audio while
`queue.current` stays whatever it was before (or stays `None`), so
`MiniPlayerBar` (which only renders when `playback.queue.current` is set
— see `src/App.tsx`) and `PlayerView` would show stale or no now-playing
UI at all.

- [ ] **Step 1: Write the failing test**

Add a new `#[cfg(test)] mod tests` block at the end of
`src-tauri/src/commands/queue.rs` (this file has no test module yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::media::player::Player;
    use crate::media::resolver::{Resolver, ResolverConfig};
    use crate::moods::MoodCatalog;
    use crate::queue::Queue;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    fn test_state() -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                yt_dlp_path: PathBuf::from("yt-dlp"),
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-queue-unused.sock"),
            )),
            mpris: None,
        }
    }

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
    fn make_room_for_single_track_ends_an_active_session_and_clears_its_queue() {
        let state = test_state();
        let mood_id = state.moods.list()[0].id.clone();
        state.db.lock().unwrap().start_session(&mood_id).unwrap();
        state.queue.lock().unwrap().add_candidates([track("a"), track("b")]);

        make_room_for_single_track(&state).unwrap();

        assert!(state.db.lock().unwrap().current_session().unwrap().is_none());
        assert!(state.queue.lock().unwrap().current().is_none());
    }

    #[test]
    fn make_room_for_single_track_clears_a_leftover_ad_hoc_track_with_no_session() {
        let state = test_state();
        state.queue.lock().unwrap().add_candidates([track("leftover")]);
        assert!(state.db.lock().unwrap().current_session().unwrap().is_none());

        make_room_for_single_track(&state).unwrap();

        assert!(state.queue.lock().unwrap().current().is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `src-tauri/`): `cargo test make_room_for_single_track`
Expected: FAIL with "cannot find function `make_room_for_single_track`"

- [ ] **Step 3: Write minimal implementation**

Add to `src-tauri/src/commands/queue.rs`, after `ensure_queue_topped_up`:

```rust
/// Clears the way for an ad-hoc single-track play: ends the active
/// session if there is one (a favorited-track replay isn't tied to a
/// mood, so it doesn't start a new one), or clears the queue directly if
/// there wasn't one — either way leaving the queue empty before the
/// caller adds the new track as `current`.
///
/// The "no session" branch matters on its own: without it, a second
/// favorited-track click while one ad-hoc track is already playing (no
/// session was ever started for it) would append behind that track
/// instead of replacing it, since `Queue::add_candidates` only makes a
/// track current when nothing is current yet.
pub(crate) fn make_room_for_single_track(state: &AppState) -> Result<()> {
    if state.db.lock().unwrap().current_session()?.is_some() {
        super::session::end_session_impl(state)?;
    } else {
        state.queue.lock().unwrap().clear();
    }
    Ok(())
}

/// Plays a single track outside of any mood session — used for replaying
/// a favorited track from Discover. See `make_room_for_single_track` for
/// why this can't just call `commands::playback::play_track` directly.
#[tauri::command]
pub async fn play_single_track(state: State<'_, AppState>, track: Track) -> Result<()> {
    make_room_for_single_track(&state)?;
    state.queue.lock().unwrap().add_candidates([track.clone()]);
    super::resolve_and_load(&state, &track).await
}
```

Register the new command in `src-tauri/src/lib.rs`'s
`invoke_handler(tauri::generate_handler![...])` list:

```rust
commands::queue::play_single_track,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test make_room_for_single_track`
Expected: PASS (2 tests)

- [ ] **Step 5: Run the full Rust verification gate**

Run (from `src-tauri/`):
```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
Expected: all clean, all tests pass (existing suite + the 6 new tests
from Tasks 1–5).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/queue.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add play_single_track for favorited tracks"
```

---

## Task 6: `api.ts` — new types and IPC wrappers

**Files:**
- Modify: `src/lib/api.ts`

**Interfaces:**
- Consumes: Rust commands from Tasks 4–5 (`list_favorite_tracks`,
  `list_most_played_moods`, `get_listening_stats`, `play_single_track`).
- Produces: TS interfaces `MoodPlayCount`, `CategoryBreakdown`,
  `ListeningStats`; `api.listFavoriteTracks`, `api.listMostPlayedMoods`,
  `api.getListeningStats`, `api.playSingleTrack`.

- [ ] **Step 1: Add the new interfaces**

Add to `src/lib/api.ts`, after `SessionSummary`:

```typescript
export interface MoodPlayCount {
  mood_id: string;
  play_count: number;
}

export interface CategoryBreakdown {
  category: string;
  session_count: number;
}

export interface ListeningStats {
  total_seconds_listened: number;
  total_sessions: number;
  total_tracks_played: number;
  top_mood_id: string | null;
  category_breakdown: CategoryBreakdown[];
}
```

- [ ] **Step 2: Add the new wrappers**

Add to the `api` object in `src/lib/api.ts`, after `getTrackFeedback`:

```typescript
  listFavoriteTracks: () => call<Track[]>("list_favorite_tracks"),
  listMostPlayedMoods: () => call<MoodPlayCount[]>("list_most_played_moods"),
  getListeningStats: () => call<ListeningStats>("get_listening_stats"),
  playSingleTrack: (track: Track) => call<void>("play_single_track", { track }),
```

- [ ] **Step 3: Typecheck**

Run (from repo root): `npx tsc --noEmit`
Expected: no errors (pure additions, nothing consumes them yet).

- [ ] **Step 4: Commit**

```bash
git add src/lib/api.ts
git commit -m "feat(api): add discover and listening-stats ipc wrappers"
```

---

## Task 7: `useDiscover` hook

**Files:**
- Create: `src/hooks/useDiscover.ts`

**Interfaces:**
- Consumes: `api.listHistory`, `api.listFavoriteMoods`,
  `api.listFavoriteTracks`, `api.listMostPlayedMoods`,
  `api.getListeningStats` (Task 6 + existing).
- Produces: `useDiscover(): { history: SessionSummary[]; favoriteMoodIds:
  string[]; favoriteTracks: Track[]; mostPlayedMoods: MoodPlayCount[];
  stats: ListeningStats | null; loading: boolean; error: string | null }`.

- [ ] **Step 1: Write the hook**

Create `src/hooks/useDiscover.ts`:

```typescript
import { useEffect, useState } from "react";
import {
  api,
  type ListeningStats,
  type MoodPlayCount,
  type SessionSummary,
  type Track,
} from "../lib/api";

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/**
 * Fetches everything Discover needs, once on mount — matches
 * `useSettings`/`useMoods`'s existing fetch-on-mount pattern. Rust stays
 * the source of truth; this hook never invents data it doesn't have.
 */
export function useDiscover() {
  const [history, setHistory] = useState<SessionSummary[]>([]);
  const [favoriteMoodIds, setFavoriteMoodIds] = useState<string[]>([]);
  const [favoriteTracks, setFavoriteTracks] = useState<Track[]>([]);
  const [mostPlayedMoods, setMostPlayedMoods] = useState<MoodPlayCount[]>([]);
  const [stats, setStats] = useState<ListeningStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [historyList, favoriteMoods, favTracks, mostPlayed, listeningStats] = await Promise.all([
          api.listHistory(20, 0),
          api.listFavoriteMoods(),
          api.listFavoriteTracks(),
          api.listMostPlayedMoods(),
          api.getListeningStats(),
        ]);
        if (!cancelled) {
          setHistory(historyList);
          setFavoriteMoodIds(favoriteMoods);
          setFavoriteTracks(favTracks);
          setMostPlayedMoods(mostPlayed);
          setStats(listeningStats);
        }
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return { history, favoriteMoodIds, favoriteTracks, mostPlayedMoods, stats, loading, error };
}
```

- [ ] **Step 2: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/hooks/useDiscover.ts
git commit -m "feat(discover): add useDiscover data-fetching hook"
```

---

## Task 8: `LibraryTab` and `StatsTab` components

**Files:**
- Create: `src/components/LibraryTab.tsx`
- Create: `src/components/StatsTab.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `useDiscover`'s return type (Task 7), `MoodSummary`,
  `Track`, `ListeningStats` (existing/Task 6), `EmptyState`
  (`src/components/EmptyState.tsx`, existing), `EmptyQueueIcon`
  (`src/components/icons.tsx`, existing).
- Produces: `LibraryTab` and `StatsTab` React components (props below).

- [ ] **Step 1: Write `LibraryTab.tsx`**

Create `src/components/LibraryTab.tsx`:

```typescript
import { EmptyState } from "./EmptyState";
import { EmptyQueueIcon } from "./icons";
import type { useDiscover } from "../hooks/useDiscover";
import type { MoodSummary, Track } from "../lib/api";

interface LibraryTabProps {
  discover: ReturnType<typeof useDiscover>;
  moods: MoodSummary[];
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onPlayTrack: (track: Track) => void;
}

function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

export function LibraryTab({
  discover,
  moods,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onPlayTrack,
}: LibraryTabProps) {
  const { history, favoriteMoodIds, favoriteTracks, mostPlayedMoods, loading } = discover;
  const moodsById = new Map(moods.map((m) => [m.id, m]));
  const moodName = (moodId: string) => moodsById.get(moodId)?.name ?? "Unknown mood";
  const busy = startingMoodId !== null || startingTrackId !== null;

  if (loading) {
    return (
      <div className="library-tab">
        <div className="skeleton" style={{ height: 44, borderRadius: 14, marginBottom: 8 }} />
        <div className="skeleton" style={{ height: 44, borderRadius: 14 }} />
      </div>
    );
  }

  return (
    <div className="library-tab">
      <section className="library-section">
        <h2 className="mood-row__title">Most Played Moods</h2>
        {mostPlayedMoods.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No moods played yet" />
        ) : (
          mostPlayedMoods.map((entry) => (
            <button
              key={entry.mood_id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(entry.mood_id)}
            >
              <span className="library-row__title">{moodName(entry.mood_id)}</span>
              <span className="library-row__meta">{entry.play_count} sessions</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Favorite Moods</h2>
        {favoriteMoodIds.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No favorited moods yet" />
        ) : (
          favoriteMoodIds.map((moodId) => (
            <button
              key={moodId}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(moodId)}
            >
              <span className="library-row__title">{moodName(moodId)}</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Favorite Tracks</h2>
        {favoriteTracks.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No favorited tracks yet" />
        ) : (
          favoriteTracks.map((track) => (
            <button
              key={track.id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onPlayTrack(track)}
            >
              {startingTrackId === track.id ? (
                <span className="mood-card__spinner" aria-hidden="true" />
              ) : null}
              <span className="library-row__title">{track.title}</span>
              <span className="library-row__meta">{track.artist ?? ""}</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Session History</h2>
        {history.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No sessions yet" />
        ) : (
          history.map((session) => (
            <button
              key={session.id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(session.mood_id)}
            >
              <span className="library-row__title">{moodName(session.mood_id)}</span>
              <span className="library-row__meta">
                {formatDate(session.started_at)} · {session.track_count} tracks
              </span>
            </button>
          ))
        )}
      </section>
    </div>
  );
}
```

- [ ] **Step 2: Write `StatsTab.tsx`**

Create `src/components/StatsTab.tsx`:

```typescript
import type { ListeningStats, MoodSummary } from "../lib/api";

interface StatsTabProps {
  stats: ListeningStats | null;
  moods: MoodSummary[];
  loading: boolean;
}

function formatDuration(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}

const CATEGORY_LABEL: Record<string, string> = {
  power: "Power",
  dark: "Dark",
  love: "Love",
  sad: "Sad",
  "energy-lifestyle": "Energy & Lifestyle",
  cinematic: "Cinematic",
};

export function StatsTab({ stats, moods, loading }: StatsTabProps) {
  if (loading || !stats) {
    return (
      <div className="stats-tab">
        <div className="skeleton" style={{ height: 88, borderRadius: 16 }} />
      </div>
    );
  }

  const topMoodName = stats.top_mood_id
    ? (moods.find((m) => m.id === stats.top_mood_id)?.name ?? "Unknown mood")
    : "—";

  return (
    <div className="stats-tab">
      <div className="stats-grid">
        <div className="stats-card">
          <div className="stats-card__value">{formatDuration(stats.total_seconds_listened)}</div>
          <div className="stats-card__label">Time listened</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{stats.total_sessions}</div>
          <div className="stats-card__label">Sessions</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{stats.total_tracks_played}</div>
          <div className="stats-card__label">Tracks played</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{topMoodName}</div>
          <div className="stats-card__label">Top mood</div>
        </div>
      </div>

      {stats.category_breakdown.length > 0 ? (
        <section className="library-section">
          <h2 className="mood-row__title">By Category</h2>
          {stats.category_breakdown.map((entry) => (
            <div className="library-row library-row--static" key={entry.category}>
              <span className="library-row__title">{CATEGORY_LABEL[entry.category] ?? entry.category}</span>
              <span className="library-row__meta">{entry.session_count} sessions</span>
            </div>
          ))}
        </section>
      ) : null}
    </div>
  );
}
```

- [ ] **Step 3: Add CSS**

Add to `src/styles.css`, after the `/* ---------- Shared: empty / error /
loading ---------- */` block's contents (before or after `.skeleton` — end
of file is fine, it's the last section today):

```css
/* ---------- Discover ---------- */

.discover-view {
  padding: 20px 28px 28px 28px;
  display: flex;
  flex-direction: column;
  gap: 20px;
}

.library-tab,
.stats-tab {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.library-section {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.library-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 14px;
  padding: 12px 14px;
  border-radius: var(--radius-md);
  width: 100%;
  text-align: left;
}
.library-row:hover {
  background: var(--bg-elevated);
}
.library-row:disabled {
  opacity: 0.5;
  cursor: default;
}
.library-row--static {
  cursor: default;
}
.library-row__title {
  font-size: 14px;
  font-weight: 500;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.library-row__meta {
  font-size: 12px;
  color: var(--text-tertiary);
  flex: 0 0 auto;
}

.stats-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
  gap: 12px;
}
.stats-card {
  background: var(--bg-elevated);
  border-radius: var(--radius-lg);
  padding: 16px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.stats-card__value {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.01em;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.stats-card__label {
  font-size: 11px;
  color: var(--text-tertiary);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}
```

- [ ] **Step 4: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors (both files are valid standalone components, not yet
imported anywhere — that's Task 9).

- [ ] **Step 5: Commit**

```bash
git add src/components/LibraryTab.tsx src/components/StatsTab.tsx src/styles.css
git commit -m "feat(discover): add library and stats tab components"
```

---

## Task 9: `DiscoverView` container

**Files:**
- Create: `src/components/DiscoverView.tsx`

**Interfaces:**
- Consumes: `useDiscover` (Task 7), `useMoods` (existing,
  `src/hooks/useMoods.ts`), `LibraryTab`/`StatsTab` (Task 8).
- Produces: `DiscoverView` component with props `{ onError: (message:
  string) => void; startingMoodId: string | null; startingTrackId:
  string | null; onStartMood: (moodId: string) => void; onPlayTrack:
  (track: Track) => void }`.

- [ ] **Step 1: Write `DiscoverView.tsx`**

Create `src/components/DiscoverView.tsx`:

```typescript
import { useEffect, useState } from "react";
import { useDiscover } from "../hooks/useDiscover";
import { useMoods } from "../hooks/useMoods";
import { LibraryTab } from "./LibraryTab";
import { StatsTab } from "./StatsTab";
import type { Track } from "../lib/api";

type DiscoverTab = "library" | "stats";

interface DiscoverViewProps {
  onError: (message: string) => void;
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onPlayTrack: (track: Track) => void;
}

export function DiscoverView({
  onError,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onPlayTrack,
}: DiscoverViewProps) {
  const [tab, setTab] = useState<DiscoverTab>("library");
  const discover = useDiscover();
  const moodsData = useMoods();

  useEffect(() => {
    if (discover.error) onError(discover.error);
  }, [discover.error, onError]);

  useEffect(() => {
    if (moodsData.error) onError(moodsData.error);
  }, [moodsData.error, onError]);

  return (
    <div className="discover-view">
      <div className="segmented" role="tablist" aria-label="Discover">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "library"}
          className={`segment${tab === "library" ? " is-active" : ""}`}
          onClick={() => setTab("library")}
        >
          Library
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "stats"}
          className={`segment${tab === "stats" ? " is-active" : ""}`}
          onClick={() => setTab("stats")}
        >
          Statistics
        </button>
      </div>

      {tab === "library" ? (
        <LibraryTab
          discover={discover}
          moods={moodsData.moods}
          startingMoodId={startingMoodId}
          startingTrackId={startingTrackId}
          onStartMood={onStartMood}
          onPlayTrack={onPlayTrack}
        />
      ) : (
        <StatsTab stats={discover.stats} moods={moodsData.moods} loading={discover.loading} />
      )}
    </div>
  );
}
```

- [ ] **Step 2: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src/components/DiscoverView.tsx
git commit -m "feat(discover): add DiscoverView tab container"
```

---

## Task 10: Wire Discover into navigation

**Files:**
- Modify: `src/components/icons.tsx`
- Modify: `src/components/TopBar.tsx`
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `DiscoverView` (Task 9), `api.playSingleTrack` (Task 6).
- Produces: `"discover"` added to the `View` union
  (`export type View = "home" | "queue" | "discover" | "settings";`),
  reachable from the top bar.

- [ ] **Step 1: Add the icon**

Add to `src/components/icons.tsx`, after `SettingsIcon`:

```typescript
export const DiscoverIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <circle cx="12" cy="12" r="9" />
    <path d="M15.5 8.5 13 13l-4.5 2.5L11 11l4.5-2.5Z" />
  </svg>
);
```

- [ ] **Step 2: Add the nav button**

In `src/components/TopBar.tsx`, update the import line:

```typescript
import { CloseIcon, DiscoverIcon, HomeIcon, QueueIcon, SettingsIcon } from "./icons";
```

Insert a new button between the Queue button and the Settings button:

```typescript
        <button
          type="button"
          className={`nav-icon-btn${view === "discover" ? " is-active" : ""}`}
          aria-current={view === "discover" || undefined}
          aria-label="Discover"
          onClick={() => onChangeView("discover")}
        >
          <DiscoverIcon />
        </button>
```

- [ ] **Step 3: Wire `App.tsx`**

Update the `View` type:

```typescript
export type View = "home" | "queue" | "discover" | "settings";
```

Add the `Track` type import (new line, alongside the existing
`DiscoverView` import below):

```typescript
import { DiscoverView } from "./components/DiscoverView";
import type { Track } from "./lib/api";
```

Add a `startingTrackId` state next to the existing `startingMoodId`:

```typescript
  const [startingTrackId, setStartingTrackId] = useState<string | null>(null);
```

Add a `handlePlayTrack` callback, alongside `handleStartMood`:

```typescript
  const handlePlayTrack = useCallback(
    async (track: Track) => {
      setStartingTrackId(track.id);
      try {
        await api.playSingleTrack(track);
        setCurrentMoodId(null);
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
```

Add the render branch, alongside the existing `settings` branch:

```typescript
        {view === "discover" ? (
          <DiscoverView
            onError={reportError}
            startingMoodId={startingMoodId}
            startingTrackId={startingTrackId}
            onStartMood={handleStartMood}
            onPlayTrack={handlePlayTrack}
          />
        ) : null}
```

- [ ] **Step 4: Run the full frontend verification gate**

Run (from repo root):
```bash
npm run lint
npm run build
```
Expected: both clean.

- [ ] **Step 5: Commit**

```bash
git add src/components/icons.tsx src/components/TopBar.tsx src/App.tsx
git commit -m "feat(discover): wire discover screen into navigation"
```

- [ ] **Step 6: Manual verification (this sandbox can't render a Tauri window)**

Ask the user to run `npm run tauri dev` in their own terminal and check:
- The Discover icon appears in the top bar and switches views.
- Library tab shows real history/favorites/most-played data (or the empty
  states, on a fresh profile) and clicking a mood/track row actually
  starts playback with the mini player bar appearing.
- Statistics tab shows the four number cards and, once at least one
  session exists, the category breakdown.
- Clicking a favorited track while a mood session is already playing
  correctly interrupts it and switches to that track (the
  `play_single_track` behavior from Task 5).

---

## Self-Review Notes

- **Spec coverage:** every section of the spec (Purpose, Non-goals,
  Backend, Frontend, Error handling, Testing) maps to a task above; the
  `play_single_track` amendment (added to the spec after the original
  approval) is Task 5.
- **Type consistency checked:** `MoodPlayCount`, `CategoryBreakdown`,
  `ListeningStats` field names match exactly between `models.rs` (Tasks
  2–3), the command layer (Task 4), and the TS interfaces (Task 6).
  `make_room_for_single_track`/`play_single_track` names match between
  Task 5's implementation and its own test.
- **No placeholders:** every step has real, complete code — no "similar
  to Task N" references, no TBD sections.
