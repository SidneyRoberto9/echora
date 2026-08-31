# Discover + Statistics — Design

Status: Approved. First of the six Fase 7 features (Smart Search, Scenes,
Mood Mixing, Intensity, Discover, Statistics) to get a full spec → plan →
implementation cycle, per `docs/REQUIREMENTS_FREEZE.md`'s V1 scope.

## Purpose

Both are personal-insight panels — they reflect the listener's own history
back at them. Neither influences mood-engine scoring or pushes the listener
toward anything new; that's explicitly out of scope for v1.

- **Discover** (internally: the "Library" tab) — a browsable view over data
  that already exists but has no UI today: session history, favorited
  moods, favorited tracks, and a most-played-moods ranking. Every row is
  actionable (starts a session or plays a track), not read-only.
- **Statistics** — aggregate numbers about listening habits: total time
  listened, total sessions/tracks played, top mood, and a per-category
  breakdown. Cards only, no charts — matches the minimalist design
  direction and avoids pulling in a charting dependency for v1.

They share one destination (see Navigation) because they're two views over
the same underlying history, not two independent product surfaces.

## Non-goals (v1)

- No influence on mood-engine candidate scoring or recommendations.
- No charts/graphs — number cards only.
- No like/dislike ratio card (data exists via `track_feedback` but wasn't
  selected for v1).
- No new database migration — every field needed already exists in the
  `tracks`, `sessions`, `session_tracks`, `track_favorites`, and
  `mood_favorites` tables (see `src-tauri/migrations/0001_init.sql`).

## Backend (Rust)

All new code lives in the existing `db::library` and `db::sessions` modules
(no new module — this is small enough to extend what's there) plus one new
model in `models.rs`.

### `src-tauri/src/models.rs`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct MoodPlayCount {
    pub mood_id: String,
    pub play_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryBreakdown {
    pub category: String,
    pub session_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListeningStats {
    pub total_seconds_listened: i64,
    pub total_sessions: i64,
    pub total_tracks_played: i64,
    pub top_mood_id: Option<String>,
    pub category_breakdown: Vec<CategoryBreakdown>,
}
```

`category_breakdown` is computed by the command layer, not the DB layer:
`Db` only knows mood IDs (it never references `MoodCatalog` — moods are
bundled static data per ADR 0008, not a DB concern). The command groups
session counts by `mood_id`, then maps each `mood_id` to its `category` via
`state.moods`, skipping any `mood_id` no longer present in the loaded
catalog (a mood renamed/removed between app versions) rather than erroring.

### `src-tauri/src/db/library.rs` — new methods

```rust
/// Favorited tracks, most recently favorited first. Mirrors
/// `list_favorite_moods`'s shape but returns full `Track` rows (joined
/// from `tracks`) since the frontend needs title/artist/thumbnail to
/// render and play them, not just an id.
pub fn list_favorite_tracks(&self) -> Result<Vec<Track>>
```

SQL: `SELECT t.id, t.title, t.artist, t.duration_seconds, t.thumbnail_url
FROM track_favorites f JOIN tracks t ON t.id = f.track_id
ORDER BY f.favorited_at DESC`.

### `src-tauri/src/db/sessions.rs` — new methods

```rust
/// All-time play count per mood, descending — "most played moods" ranking.
/// Unlike `recent_mood_ids` (bounded window, existence-only, used for
/// Surprise Me diversity), this is an unbounded count used for display.
pub fn mood_play_counts(&self) -> Result<Vec<MoodPlayCount>>

/// Aggregate listening stats. `total_seconds_listened` treats a NULL
/// `completion_pct` (a play that ended without recording completion — the
/// app closing mid-track, a crash) as 0 contribution rather than the full
/// track length, since we don't actually know how much was heard.
pub fn listening_stats(&self) -> Result<ListeningStats>
```

`mood_play_counts`: `SELECT mood_id, COUNT(*) FROM sessions GROUP BY
mood_id ORDER BY COUNT(*) DESC`.

`listening_stats`: one query for `total_seconds_listened` (`SUM(t.duration_seconds
* COALESCE(st.completion_pct, 0))` joining `session_tracks` to `tracks`,
`COALESCE`d to `0` overall for an empty table), one `COUNT(*)` on `sessions`
for `total_sessions`, one `COUNT(*)` on `session_tracks` for
`total_tracks_played`, and `top_mood_id` is the first row of
`mood_play_counts()` (reused, not re-queried). `category_breakdown` is left
empty here — filled in by the command layer as described above.

### `src-tauri/src/commands/library.rs` and `commands/session.rs`

Three new `#[tauri::command]`s, thin wrappers like the existing ones:

- `list_favorite_tracks(state) -> Result<Vec<Track>>` — commands/library.rs.
- `list_most_played_moods(state) -> Result<Vec<MoodPlayCount>>` —
  commands/session.rs (co-located with the other history/session reads).
- `get_listening_stats(state) -> Result<ListeningStats>` —
  commands/session.rs. Builds the DB's `ListeningStats` then fills
  `category_breakdown` by grouping `mood_play_counts()` through
  `state.moods.get(mood_id)`, skipping unknown IDs.

### Amendment: playing a favorited track needs a new command, not raw `play_track`

`commands::playback::play_track` only resolves a stream and hands it to
mpv — it never touches `state.queue`. Calling it directly from a favorited
track click would play real audio while `queue.current` stays whatever it
was before (or `None`), so `MiniPlayerBar` (which only renders when
`playback.queue.current` is set — see `App.tsx`) and `PlayerView` would
show stale or no now-playing UI at all. A history/favorite/most-played
**mood** click doesn't have this problem — it calls `start_mood_session`,
which already goes through the real queue.

Fix: one new command, `commands::queue::play_single_track`:

```rust
/// Ends the current session (if any — a favorited-track replay isn't tied
/// to a mood, so it doesn't start a new one), replaces the queue with just
/// this one track, and plays it. `queue_next` afterward finds nothing
/// upcoming (matching a deliberately ad-hoc, non-session play) rather than
/// pulling in mood-engine candidates.
#[tauri::command]
pub async fn play_single_track(state: State<'_, AppState>, track: Track) -> Result<()> {
    if state.db.lock().unwrap().current_session()?.is_some() {
        super::session::end_session_impl(&state)?;
    }
    state.queue.lock().unwrap().add_candidates([track.clone()]);
    super::resolve_and_load(&state, &track).await
}
```

This plays but does not record history for the track (no active session to
attribute it to) — acceptable, since it's a replay of something already
favorited/known, not new listening the stats should double-count. Frontend
calls this instead of `api.playTrack` for favorited-track rows only; mood
rows keep using `api.startMoodSession`.

No other changes to `resolve_and_load`, `start_session_and_play`, or
`play_track` — everything else in Discover calls the same
`start_mood_session` command the rest of the UI already uses.

## Frontend (React)

- `src/App.tsx`: `View` union gains `"discover"`; one more conditional
  render branch, same pattern as `home`/`queue`/`settings`.
- `src/components/icons.tsx`: one new icon (compass or similar, matching
  the existing 24×24 stroke-based style, `stroke-width: 1.7–1.8`).
- `src/components/TopBar.tsx`: one more nav button, same pattern as the
  existing three.
- `src/components/DiscoverView.tsx`: owns local tab state
  (`"library" | "stats"`), renders a small tab switcher plus one of:
  - `src/components/LibraryTab.tsx` — four sections (session history,
    favorite moods, favorite tracks, most-played moods), each a list of
    rows. Reuses `EmptyState` when a section has no data. Row click
    handlers call `api.startMoodSession(moodId)` (history entries and
    favorited/most-played moods, same call `HomeView` already makes) or
    `api.playSingleTrack(track)` (favorited tracks — see the
    `play_single_track` amendment above; NOT the raw `play_track` command,
    which doesn't update the queue that `MiniPlayerBar`/`PlayerView` read).
  - `src/components/StatsTab.tsx` — number cards for
    `total_seconds_listened` (formatted as `Xh Ym`), `total_sessions`,
    `total_tracks_played`, `top_mood_id` (resolved to the mood's display
    name via the already-loaded `useMoods` catalog), and one row per
    `category_breakdown` entry.
- `src/lib/api.ts`: four new thin wrappers (`listFavoriteTracks`,
  `listMostPlayedMoods`, `getListeningStats`, `playSingleTrack`), matching
  the existing `call<T>(...)` pattern.

No new hook needed — `DiscoverView` fetches its own data on mount with
plain `useEffect`/`useState`, matching `SettingsView`'s existing pattern
(this project doesn't have a shared data-fetching abstraction yet, and
adding one for a single view would be premature).

## Error handling / edge cases

- Empty database (no sessions/favorites ever) → every list is empty, every
  stat is `0`/`None`; `EmptyState` renders per section. No query above can
  error on an empty table (all use `COUNT`/`SUM`/`GROUP BY` over
  zero rows, which SQLite returns as `0`/`NULL`, not an error).
- A `mood_id` recorded in history that's no longer in the loaded
  `MoodCatalog` (renamed/removed mood across an app update) → skipped in
  `category_breakdown` and rendered with a generic fallback label
  ("Unknown mood") in `LibraryTab`/`StatsTab` rather than erroring, since
  `Mood` lookups are `Result`-returning and already handled this way
  elsewhere in the codebase (see `commands::session::start_session_impl`).
- Clicking a history/favorite/most-played row while a session is already
  active behaves exactly like clicking a mood card on `HomeView` today
  (starts a new session, replacing the current one) — no special-casing.

## Testing

- Rust: unit tests for `list_favorite_tracks`, `mood_play_counts`, and
  `listening_stats` in their respective `#[cfg(test)] mod tests` blocks,
  each covering an empty database and a database with a few rows — same
  style already used throughout `db/library.rs` and `db/sessions.rs`.
- Frontend: no automated tests (vitest isn't installed — tracked as a
  known gap, out of scope here). Verified manually: `npm run lint`,
  `npm run build`, then a live check in the user's own terminal via
  `npm run tauri dev` (this sandbox can't visually verify a Tauri window).
- Full verification gate before claiming done, per `CLAUDE.md`: `cargo fmt
  --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
  `npm run lint`, `npm run build`.
