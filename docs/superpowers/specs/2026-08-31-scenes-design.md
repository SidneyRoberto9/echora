# Scenes — Design

Status: Approved. Second of the six Fase 7 features (Smart Search deferred,
Scenes, Mood Mixing, Intensity, Discover✓, Statistics✓) per
`docs/REQUIREMENTS_FREEZE.md`'s V1 scope.

## Purpose

A Scene is a **named, frozen snapshot of an entire queue** — the exact
tracks, in the exact order, from a session the listener wants to relive
exactly rather than let the mood engine regenerate. This fills a real gap:
today "favoriting a mood" only saves *which mood to re-roll*, and re-rolling
always produces a different queue. Nothing currently lets a listener say
"save *this specific* run of tracks."

- **Save**: from the Queue view or the expanded Player, capture the
  **entire current queue** (from track 1, not just what's left to play) and
  a user-typed name.
- **Play**: picking a saved Scene loads its exact track list into the queue
  and starts playing the first track — an ad-hoc play, like replaying a
  favorited track (see Non-goals), not a new mood session.
- **Manage**: rename or delete a saved Scene. No v1 support for
  editing a Scene's track list (removing/reordering individual tracks) —
  frozen means frozen; re-save a new Scene if the lineup should change.
- **Where they live**: a fifth section in Discover's existing Library tab,
  alongside session history, favorite moods, favorite tracks, and
  most-played moods.

## Non-goals (v1)

- No editing a saved Scene's track list (add/remove/reorder tracks) —
  only rename and delete.
- Playing a Scene does **not** create a session row, does **not** appear in
  Session History, and does **not** count toward Statistics (total
  sessions/tracks-played/category-breakdown) — exactly like replaying a
  favorited track via `play_single_track` (see
  `docs/superpowers/specs/2026-08-31-discover-statistics-design.md`'s
  amendment). Scenes are a separate, parallel concept from mood sessions,
  not a special kind of session.
- No sharing/exporting Scenes, no scene artwork/cover image, no limit UI
  on Scene count or track count beyond what SQLite/the queue already
  naturally bound.
- No handling beyond what already exists for an unavailable/unresolvable
  track mid-Scene-playback — this is exactly the same failure mode a
  mood-engine-generated queue already has when advancing to a bad track
  (`queue_next`'s existing resolve-on-advance failure path), not a new
  problem Scenes introduce.

## Backend (Rust)

### New migration: `src-tauri/migrations/0002_scenes.sql`

```sql
CREATE TABLE scenes (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT NOT NULL,
    created_at  INTEGER NOT NULL
);

CREATE TABLE scene_tracks (
    scene_id  INTEGER NOT NULL REFERENCES scenes (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    track_id  TEXT NOT NULL REFERENCES tracks (id),
    PRIMARY KEY (scene_id, position)
);
```

Registered as a second entry in `db::migrations()`
(`src-tauri/src/db/mod.rs`): `M::up(include_str!("../../migrations/0002_scenes.sql"))`
appended to the existing `Migrations::new(vec![...])` call. `rusqlite_migration`
applies migrations in order and tracks the current version, so this is
additive and safe against existing installed databases (ADR 0004).

**Important correctness detail:** `scene_tracks.track_id` has a `REFERENCES
tracks (id)` foreign key, and `foreign_keys = ON` is set at connection open
(`db/mod.rs`). A track sitting in the live in-memory `Queue` — especially
one the mood engine generated but the listener never actually played yet —
is **not** guaranteed to already exist in the `tracks` table; `upsert_track`
today is only called from `record_play`, `favorite_track`,
`favorite_mood`(no-op for tracks), and `set_track_feedback`. `Db::save_scene`
must upsert every track in the snapshot *before* inserting its
`scene_tracks` row — see the exact code below (it upserts inline against
the open transaction, not via the existing `self.upsert_track`, for a
reason explained there) — otherwise saving a Scene with an unplayed
upcoming track fails the foreign key constraint.

### `src-tauri/src/models.rs`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SceneSummary {
    pub id: i64,
    pub name: String,
    pub created_at: i64,
    pub track_count: u32,
}
```

No public `Scene` (with full track list) IPC type is needed — the frontend
never needs a Scene's track contents directly; `play_scene(id)` loads them
straight into the queue on the Rust side, and the existing `get_queue`/
`refreshQueue()` round trip is how the frontend learns what's now playing
(same pattern `play_single_track` already established).

### `src-tauri/src/queue.rs` — one new method

```rust
/// Every track in the queue's lifetime, in order — current, past, and
/// upcoming. Unlike `upcoming()` (only what's left to play), this is what
/// "save the whole session as a Scene" needs to capture.
pub fn all_tracks(&self) -> &[Track] {
    &self.items
}
```

### New module: `src-tauri/src/db/scenes.rs`

Registered in `src-tauri/src/db/mod.rs`'s `mod library; mod scenes; mod
scoring_signals; mod sessions; mod settings;` list (alphabetical, matching
the existing ordering convention).

```rust
impl Db {
    /// Upserts every track (see the foreign-key note above), then inserts
    /// the scene and its ordered track list. Errors with
    /// `EchoraError::QueueEmpty` if `tracks` is empty — nothing to save.
    pub fn save_scene(&self, name: &str, tracks: &[Track]) -> Result<SceneSummary>

    pub fn list_scenes(&self) -> Result<Vec<SceneSummary>>

    /// The scene's tracks in saved order — used internally by
    /// `commands::queue::play_scene`, never exposed to the frontend directly.
    pub fn scene_tracks(&self, scene_id: i64) -> Result<Vec<Track>>

    pub fn rename_scene(&self, scene_id: i64, name: &str) -> Result<()>

    pub fn delete_scene(&self, scene_id: i64) -> Result<()>
}
```

`save_scene` is this codebase's first multi-statement write that must be
atomic (every prior write in `db/*.rs` was a single `INSERT`/`UPDATE`).
Every other `Db` method takes `&self` — `rusqlite::Connection::transaction`
needs `&mut self`, which would be an inconsistent signature for one method,
so use `self.conn.unchecked_transaction()` instead (`&self`, available in
rusqlite 0.32). "Unchecked" only means it skips a compile-time guarantee
that no other query runs on the connection while the transaction is
open — safe here because `Db` only exists inside `Mutex<Db>` in `AppState`,
so the lock already gives exclusive access for the transaction's whole
duration:

```rust
pub fn save_scene(&self, name: &str, tracks: &[Track]) -> Result<SceneSummary> {
    let tx = self.conn.unchecked_transaction()?;
    let created_at = now();
    tx.execute(
        "INSERT INTO scenes (name, created_at) VALUES (?1, ?2)",
        rusqlite::params![name, created_at],
    )?;
    let scene_id = tx.last_insert_rowid();
    for (position, track) in tracks.iter().enumerate() {
        // Same upsert SQL as `Db::upsert_track` (`db/mod.rs`), run against
        // `tx` directly instead of `self.conn` — `Transaction` derefs to
        // `Connection`, so `tx.execute(...)` works the same way, but doing
        // it this way keeps every write for this scene inside the one
        // transaction. Do not call `self.upsert_track(track)` here: that
        // method runs on `self.conn` outside `tx` and would commit each
        // upsert immediately, defeating the point of the transaction.
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
```

`list_scenes`: `SELECT s.id, s.name,
s.created_at, COUNT(st.position) FROM scenes s JOIN scene_tracks st ON
st.scene_id = s.id GROUP BY s.id ORDER BY s.created_at DESC`. `scene_tracks`:
join `scene_tracks` to `tracks` ordered by `position`, same shape as
`list_favorite_tracks`'s join. `delete_scene` relies on `ON DELETE CASCADE`
to remove its `scene_tracks` rows.

### `src-tauri/src/commands/queue.rs` — new commands

Reuses the existing `make_room_for_single_track` helper (added for
`play_single_track` in the Discover feature) unchanged — it already does
exactly what a Scene needs before loading a new track list: end the active
session if any, or clear the queue directly if there's an ad-hoc leftover
track.

Following this codebase's established pattern (`start_session_impl`,
`make_room_for_single_track`): testable branching logic lives in a plain
`pub(crate) fn`/`pub(crate) async fn`, and the `#[tauri::command]` wrapper
stays a one-line call into it — this is what makes the empty-queue and
empty-scene error paths unit-testable without a real Tauri `App`.

```rust
pub(crate) fn save_scene_impl(state: &AppState, name: &str) -> Result<SceneSummary> {
    let tracks = state.queue.lock().unwrap().all_tracks().to_vec();
    if tracks.is_empty() {
        return Err(EchoraError::QueueEmpty);
    }
    state.db.lock().unwrap().save_scene(name, &tracks)
}

#[tauri::command]
pub fn save_scene(state: State<AppState>, name: String) -> Result<SceneSummary> {
    save_scene_impl(&state, &name)
}

#[tauri::command]
pub fn list_scenes(state: State<AppState>) -> Result<Vec<SceneSummary>> {
    state.db.lock().unwrap().list_scenes()
}

pub(crate) async fn play_scene_impl(state: &AppState, scene_id: i64) -> Result<()> {
    let tracks = state.db.lock().unwrap().scene_tracks(scene_id)?;
    let Some(first) = tracks.first().cloned() else {
        return Err(EchoraError::QueueEmpty);
    };
    make_room_for_single_track(state)?;
    state.queue.lock().unwrap().add_candidates(tracks);
    super::resolve_and_load(state, &first).await
}

#[tauri::command]
pub async fn play_scene(state: State<'_, AppState>, scene_id: i64) -> Result<()> {
    play_scene_impl(&state, scene_id).await
}

#[tauri::command]
pub fn rename_scene(state: State<AppState>, scene_id: i64, name: String) -> Result<()> {
    state.db.lock().unwrap().rename_scene(scene_id, &name)
}

#[tauri::command]
pub fn delete_scene(state: State<AppState>, scene_id: i64) -> Result<()> {
    state.db.lock().unwrap().delete_scene(scene_id)
}
```

`EchoraError::QueueEmpty` already exists in `error.rs` (currently
`#[allow(dead_code)]`, unused) — this is its first real caller; the
`#[allow(dead_code)]` attribute should come off once it's used.

Register all five new commands in `lib.rs`'s
`invoke_handler(tauri::generate_handler![...])` list.

## Frontend (React)

### `src/lib/api.ts`

```typescript
export interface SceneSummary {
  id: number;
  name: string;
  created_at: number;
  track_count: number;
}
```

```typescript
saveScene: (name: string) => call<SceneSummary>("save_scene", { name }),
listScenes: () => call<SceneSummary[]>("list_scenes"),
playScene: (sceneId: number) => call<void>("play_scene", { sceneId }),
renameScene: (sceneId: number, name: string) => call<void>("rename_scene", { sceneId, name }),
deleteScene: (sceneId: number) => call<void>("delete_scene", { sceneId }),
```

### New component: `src/components/NameModal.tsx`

The project's first modal — a small centered overlay, reused for both
"save as Scene" (empty initial value) and "rename Scene" (pre-filled).
Props: `{ title: string; initialValue?: string; onConfirm: (name: string)
=> void; onCancel: () => void }`. Renders a scrim (`position: fixed; inset:
0; background: oklch(0% 0 0 / 0.5)`) behind a `bg-elevated` panel
(`radius-lg`, centered) containing the title, a text `<input>` (autofocus,
Enter submits, Escape cancels), and Cancel/Save buttons. New CSS block
`.modal-scrim` / `.modal-panel` / `.modal-input` in `styles.css`, following
existing token usage (no new colors).

### Save entry points

- `src/components/QueueView.tsx`: a small "Save as Scene" text-link/icon
  button in the `queue-view__label` header row (next to "Now Playing"),
  disabled when the queue is empty (mirrors the existing empty-state guard
  at the top of this component). Opens `NameModal` locally (component
  state: `showSaveModal: boolean`); on confirm, calls `api.saveScene(name)`
  and closes the modal — no toast/success banner for v1, the modal closing
  is the confirmation (matches this project's existing minimal-feedback
  style; errors still surface through the app's existing global error
  banner via a passed-down `onError`).
- `src/components/PlayerView.tsx`: a new icon button in `player-view__topbar`,
  grouped with the existing Queue button on the right, using a new
  `SaveIcon` (bookmark-style, matching the existing 24×24 stroke icon set
  in `icons.tsx`). Same `NameModal` flow, disabled when `!track` (nothing
  playing).

Both call sites need `onError` threading consistent with how `HomeView`/
`DiscoverView` already receive it from `App.tsx` — `QueueView`/`PlayerView`
currently don't take an `onError` prop; add one to each, passed from
`App.tsx`'s existing `reportError`.

### `LibraryTab.tsx` — fifth section

A new "Scenes" section, positioned last (after Session History), following
the exact same `EmptySection`/full-tab-`EmptyState` pattern the other four
sections already use (from the Discover+Statistics fix wave). Each row
shows the Scene's name and track count; clicking calls `api.playScene(id)`
through the same `onPlayTrack`-style callback wiring already established
(a new `onPlaySceneId: (id: number) => void` prop threaded the same way
`onPlayTrack` is). A trailing rename/delete icon pair appears on row
hover, mirroring `QueueView`'s existing `.queue-row__remove` reveal-on-hover
pattern (new `.library-row__actions` CSS, opacity 0 → 1 on `:hover`/
`:focus-visible`, matching `.queue-row__remove`'s existing transition).
Rename opens `NameModal` pre-filled with the current name; delete calls
`api.deleteScene(id)` directly (no confirmation dialog for v1 — consistent
with `QueueView`'s existing track-removal, which also has none).

### `useDiscover.ts` — one more fetch

Add `api.listScenes()` as a sixth parallel fetch alongside the existing
five, exposing `scenes: SceneSummary[]` in the hook's return shape. A
`refreshScenes` function should also be exposed so `LibraryTab` can
re-fetch after a save/rename/delete without a full page-level remount
(none of the existing five collections need this today, since nothing
currently mutates them from within Discover itself — Scenes are the first
data in this hook that Discover itself can create/change).

## Error handling / edge cases

- Saving with an empty queue → `EchoraError::QueueEmpty` (code
  `queue_empty`), surfaced through the existing global error banner; the
  Save buttons are additionally disabled in this state so it should be
  rare to actually hit the error path.
- Saving with a blank/whitespace-only name → treat as invalid client-side
  (disable the Save button in `NameModal` until the trimmed value is
  non-empty) — no backend validation needed for this, matching how no
  other user-text-entry field in the app validates emptiness server-side.
- Playing a Scene whose tracks were later favorited/unfavorited elsewhere
  is unaffected — Scenes reference `tracks.id`, not `track_favorites`, so
  favorite status is independent and not stored redundantly.
- Deleting a Scene mid-playback (if ever reachable) does not stop
  playback — `ON DELETE CASCADE` only removes the DB rows; the in-memory
  `Queue` a `play_scene` call already populated is untouched, matching how
  deleting a favorited track today doesn't stop it if it's currently
  playing.

## Testing

- Rust: unit tests for `Db::save_scene` (including the upsert-before-insert
  correctness property — a track never previously seen must not violate
  the foreign key), `list_scenes`, `scene_tracks` (order preserved),
  `rename_scene`, `delete_scene` (cascade removes `scene_tracks`), and
  `commands::queue::save_scene_impl`/`play_scene_impl` for the
  empty-queue/empty-scene error paths — same style already used throughout
  `db/library.rs`/`db/sessions.rs`/`commands/queue.rs`.
- Frontend: no automated tests (documented project constraint). Verified
  manually via `npm run lint`/`npm run build`, then live in the user's own
  terminal.
- Full verification gate per `CLAUDE.md`: `cargo fmt --check`, `cargo
  clippy --all-targets -- -D warnings`, `cargo test`, `npm run lint`, `npm
  run build`.
