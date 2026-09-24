# Link Radio Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Paste a YouTube link on Home → a persisted session that plays that track, then an endless stream of similar tracks from YouTube's own Mix, until another link or a mood is picked.

**Architecture:** Rust parses/validates the link, fetches `watch?v=ID&list=RDID` via the existing yt-dlp sidecar (`--flat-playlist`), filters with existing mood-engine helpers while keeping Mix order, and stores the session with a `seed_track_id` (no mood rows). `ensure_queue_topped_up` re-seeds from the last queued track. React only adds an input and labels.

**Tech Stack:** Rust (Tauri 2, rusqlite + rusqlite_migration, tokio), yt-dlp sidecar, React + TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-23-link-radio-design.md`

## Global Constraints

- npm only (never pnpm/yarn/bun). Never run `git commit` directly — the orchestrator commits via the `auto-commit` skill (no `Co-Authored-By`, no attribution footers).
- Sidecar args are always an argument array; the YouTube URL is built only from an id that passed `metadata::is_valid_youtube_id`.
- No new crates. No new npm packages.
- Rust is the source of truth: React never parses links or decides seeds.
- User-facing strings (verbatim): `That's not a YouTube link.` · `Couldn't build a mix from this track.` · placeholder `Paste a YouTube link to start a mix` · button `Start mix` · label `Mix · <seed title>`.
- Gates before a task is done: from `src-tauri/`: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`; frontend tasks also `npm run lint && npm run build` from repo root.
- Never launch the live app (`npm run tauri dev`); the user tests it.

## Review Focus

1. Links with extra params (`&t=42s`, `&si=…`, `&list=PL…`, `&pp=…`) and whitespace/trailing slash → still the right seed id (Task 1 tests).
2. `youtube.com/shorts/ID`, playlist-only links, channel links, `youtube.com.evil.com` → rejected, never passed to yt-dlp (Task 1 tests).
3. A failed start (bad link, empty Mix, timeout) must NOT end or clear the currently playing session (Task 5: fetch before `start_link_session`; test on the pure seed-selection helper).
4. Mix that is 100% already-played tracks → falls back to original seed, never loops forever or queues duplicates (Task 4 tests).
5. Library row for a link session (empty `moods`) must not crash and must replay via link (Task 6).

---

### Task 1: Link parsing

**Files:**
- Modify: `src-tauri/src/error.rs` (new variant)
- Modify: `src-tauri/src/media/metadata.rs` (new fn + tests in its existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Produces: `pub fn youtube_id_from_link(input: &str) -> crate::error::Result<String>` in `media::metadata`; `EchoraError::InvalidLink` (code `"invalid_link"`, message `That's not a YouTube link.`); `EchoraError::MixUnavailable` (code `"mix_unavailable"`, message `Couldn't build a mix from this track.`) — used by Task 5.

- [ ] **Step 1: Add the error variant**

In `error.rs`, add to the enum (after `InvalidMoodMix`):

```rust
    #[error("That's not a YouTube link.")]
    InvalidLink,

    #[error("Couldn't build a mix from this track.")]
    MixUnavailable,
```

and to `code()`:

```rust
            EchoraError::InvalidLink => "invalid_link",
            EchoraError::MixUnavailable => "mix_unavailable",
```

- [ ] **Step 2: Write failing tests** (append inside `metadata.rs`'s test module)

```rust
    #[test]
    fn youtube_id_from_link_accepts_supported_forms() {
        for link in [
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://youtube.com/watch?v=dQw4w9WgXcQ",
            "http://m.youtube.com/watch?v=dQw4w9WgXcQ",
            "https://music.youtube.com/watch?v=dQw4w9WgXcQ&si=abc",
            "https://www.youtube.com/watch?list=PLx&v=dQw4w9WgXcQ&t=42s",
            "https://youtu.be/dQw4w9WgXcQ",
            "https://youtu.be/dQw4w9WgXcQ?si=xyz&t=10",
            "youtu.be/dQw4w9WgXcQ",
            "  https://www.youtube.com/watch?v=dQw4w9WgXcQ  ",
            "https://youtu.be/dQw4w9WgXcQ/",
        ] {
            assert_eq!(youtube_id_from_link(link).unwrap(), "dQw4w9WgXcQ", "{link}");
        }
    }

    #[test]
    fn youtube_id_from_link_rejects_everything_else() {
        for link in [
            "",
            "never gonna give you up",
            "https://www.youtube.com/playlist?list=PLx",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/shorts/dQw4w9WgXcQ",
            "https://youtube.com.evil.com/watch?v=dQw4w9WgXcQ",
            "https://evil.com/?u=youtube.com/watch?v=dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXc",
            "https://www.youtube.com/watch?v=dQw4w9WgX&Q",
            "https://youtu.be/",
            "ftp://youtu.be/dQw4w9WgXcQ",
        ] {
            assert!(
                matches!(youtube_id_from_link(link), Err(EchoraError::InvalidLink)),
                "{link}"
            );
        }
    }
```

(Add `use crate::error::EchoraError;` in the test module if not already imported.)

- [ ] **Step 3: Run** `cd src-tauri && cargo test --lib youtube_id_from_link` → FAIL (function not defined).

- [ ] **Step 4: Implement** (in `metadata.rs`, next to `is_valid_youtube_id`)

```rust
/// Extracts the video id from a pasted YouTube link -- `youtube.com`/
/// `www.`/`m.`/`music.` `/watch?v=ID` or `youtu.be/ID`, any extra query
/// params ignored. The id still goes through `is_valid_youtube_id`, so
/// nothing but a bare 11-char id ever reaches a yt-dlp URL.
pub fn youtube_id_from_link(input: &str) -> Result<String> {
    let rest = input.trim();
    let rest = rest
        .strip_prefix("https://")
        .or_else(|| rest.strip_prefix("http://"))
        .unwrap_or(rest);
    if rest.contains("://") {
        return Err(EchoraError::InvalidLink);
    }
    let (host, path_and_query) = rest.split_once('/').unwrap_or((rest, ""));
    let (path, query) = path_and_query.split_once('?').unwrap_or((path_and_query, ""));

    let id = match host.to_ascii_lowercase().as_str() {
        "youtu.be" => path.trim_end_matches('/'),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" | "music.youtube.com"
            if path == "watch" =>
        {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("v="))
                .unwrap_or("")
        }
        _ => "",
    };
    if is_valid_youtube_id(id) {
        Ok(id.to_string())
    } else {
        Err(EchoraError::InvalidLink)
    }
}
```

Make sure `EchoraError` and `Result` are imported at the top of `metadata.rs` (it already returns `Result<Track>`; add `EchoraError` to the `use` if missing).

- [ ] **Step 5: Run** `cargo test --lib youtube_id_from_link` → PASS; then full gates.

---

### Task 2: `Resolver::radio`

**Files:**
- Modify: `src-tauri/src/media/resolver.rs`

**Interfaces:**
- Consumes: `metadata::is_valid_youtube_id`, `metadata::parse_search_result`.
- Produces: `pub async fn radio<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, seed_id: &str, limit: u32) -> Result<Vec<Track>>` on `Resolver`.

- [ ] **Step 1: Failing test** (in the non-ignored test module, next to `resolve_with_retry_rejects_an_id_with_a_url_injection_character`, reusing its `test_config()`/`test_app_handle()` helpers):

```rust
    #[tokio::test]
    async fn radio_rejects_an_invalid_seed_id_before_spawning_yt_dlp() {
        let resolver = Resolver::new(test_config());
        let app = test_app_handle();
        let err = resolver.radio(&app, "abc&list=x", 30).await.unwrap_err();
        assert!(matches!(err, EchoraError::InvalidLink), "{err:?}");
    }
```

And in the `#[ignore]`d real-network module, next to `searching_a_real_mood_query_returns_tracks`:

```rust
    #[tokio::test]
    #[ignore]
    async fn radio_for_a_real_track_returns_related_tracks_seed_first() {
        let resolver = Resolver::new(dev_config());
        let app = test_app_handle();
        let tracks = resolver.radio(&app, "dQw4w9WgXcQ", 30).await.unwrap();
        assert!(tracks.len() >= 10, "got {}", tracks.len());
        assert_eq!(tracks[0].id, "dQw4w9WgXcQ");
    }
```

- [ ] **Step 2: Run** `cargo test --lib radio_rejects` → FAIL (no method).

- [ ] **Step 3: Implement** (below `search`)

```rust
    /// YouTube's own auto-generated Mix for `seed_id` (`list=RD<id>`) --
    /// the similarity source for link radio. Same flat, metadata-only
    /// output as `search`. The URL is built only from a validated id.
    pub async fn radio<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        seed_id: &str,
        limit: u32,
    ) -> Result<Vec<Track>> {
        if !metadata::is_valid_youtube_id(seed_id) {
            return Err(EchoraError::InvalidLink);
        }
        let url = format!("https://www.youtube.com/watch?v={seed_id}&list=RD{seed_id}");
        let stdout = self
            .run(
                app,
                vec![
                    "--js-runtimes".into(),
                    format!("deno:{}", self.config.deno_path.display()),
                    url,
                    "--flat-playlist".into(),
                    "--playlist-end".into(),
                    limit.to_string(),
                    "--dump-json".into(),
                    "--no-warnings".into(),
                ],
            )
            .await?;

        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(metadata::parse_search_result)
            .collect()
    }
```

- [ ] **Step 4: Run** test → PASS; full gates. Optionally run the ignored test if the dev sidecars exist: `cargo test --lib radio_for_a_real_track -- --ignored`.

---

### Task 3: Persistence — seed on sessions

**Files:**
- Create: `src-tauri/migrations/0005_link_sessions.sql`
- Modify: `src-tauri/src/db/mod.rs` (register migration)
- Modify: `src-tauri/src/models.rs` (`SessionInfo`, `SessionSummary`)
- Modify: `src-tauri/src/db/sessions.rs` (new `start_link_session`, `seed` population, tests)
- Modify: every other place that constructs `SessionInfo`/`SessionSummary` (compiler will list them) — add `seed: None`.

**Interfaces:**
- Produces: `SessionInfo.seed: Option<Track>`, `SessionSummary.seed: Option<Track>`; `Db::start_link_session(&self, seed: &Track) -> Result<SessionInfo>` (ends any open session and upserts the seed track in the same transaction).

- [ ] **Step 1: Migration file**

```sql
-- Link-radio sessions: seeded by one track instead of moods. A link
-- session has this set and no session_moods rows.
ALTER TABLE sessions ADD COLUMN seed_track_id TEXT REFERENCES tracks (id);
```

Register in `migrations()` after 0004:

```rust
        M::up(include_str!("../../migrations/0005_link_sessions.sql")),
```

- [ ] **Step 2: Models** — add to both structs (after `moods`):

```rust
    /// The track a link-radio session was started from; `None` for mood
    /// sessions.
    pub seed: Option<Track>,
```

Fix every constructor the compiler flags with `seed: None` (including `Db::start_session`).

- [ ] **Step 3: Failing tests** (in `db/sessions.rs` test module; reuse its existing track helper if there is one, otherwise this one):

```rust
    fn seed_track() -> Track {
        Track {
            id: "dQw4w9WgXcQ".into(),
            title: "Never Gonna Give You Up".into(),
            artist: Some("Rick Astley".into()),
            duration_seconds: Some(214),
            thumbnail_url: None,
        }
    }

    #[test]
    fn start_link_session_persists_the_seed_and_no_moods() {
        let db = Db::open_in_memory().unwrap();
        let info = db.start_link_session(&seed_track()).unwrap();
        assert!(info.moods.is_empty());
        assert_eq!(info.seed.as_ref().unwrap().id, "dQw4w9WgXcQ");

        let current = db.current_session().unwrap().unwrap();
        assert_eq!(current.id, info.id);
        assert_eq!(current.seed.unwrap().title, "Never Gonna Give You Up");

        let listed = db.list_sessions(10, 0).unwrap();
        assert_eq!(listed[0].seed.as_ref().unwrap().id, "dQw4w9WgXcQ");
    }

    #[test]
    fn start_link_session_ends_the_previous_session() {
        let db = Db::open_in_memory().unwrap();
        let mood = db.start_session(&[("villain".to_string(), 100)]).unwrap();
        let link = db.start_link_session(&seed_track()).unwrap();
        let listed = db.list_sessions(10, 0).unwrap();
        let old = listed.iter().find(|s| s.id == mood.id).unwrap();
        assert!(old.ended_at.is_some());
        assert!(old.seed.is_none());
        assert_eq!(db.current_session().unwrap().unwrap().id, link.id);
    }
```

Run `cargo test --lib start_link_session` → FAIL.

- [ ] **Step 4: Implement**

In `sessions.rs`:

```rust
    /// Ends any open session and starts a link-radio session seeded by
    /// `seed` -- one transaction, same reasoning as `start_session`
    /// (P2-13). The seed is upserted so its title is joinable later.
    pub fn start_link_session(&self, seed: &Track) -> Result<SessionInfo> {
        let started_at = now();
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO tracks (id, title, artist, duration_seconds, thumbnail_url, first_seen_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                duration_seconds = excluded.duration_seconds,
                thumbnail_url = excluded.thumbnail_url,
                last_seen_at = excluded.last_seen_at",
            rusqlite::params![
                seed.id,
                seed.title,
                seed.artist,
                seed.duration_seconds,
                seed.thumbnail_url,
                started_at,
            ],
        )?;
        tx.execute(
            "UPDATE sessions SET ended_at = ?1 WHERE ended_at IS NULL",
            [started_at],
        )?;
        tx.execute(
            "INSERT INTO sessions (started_at, seed_track_id) VALUES (?1, ?2)",
            rusqlite::params![started_at, seed.id],
        )?;
        let id = tx.last_insert_rowid();
        tx.commit()?;
        Ok(SessionInfo {
            id,
            moods: Vec::new(),
            seed: Some(seed.clone()),
            started_at,
            ended_at: None,
        })
    }

    fn session_seed(&self, session_id: i64) -> Result<Option<Track>> {
        self.conn
            .query_row(
                "SELECT t.id, t.title, t.artist, t.duration_seconds, t.thumbnail_url
                 FROM sessions s JOIN tracks t ON t.id = s.seed_track_id
                 WHERE s.id = ?1",
                [session_id],
                |r| {
                    Ok(Track {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        duration_seconds: r.get(3)?,
                        thumbnail_url: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }
```

(Check `Track.duration_seconds`'s Rust type in `models.rs` and match `r.get` to it; import `Track` if not already.) In `current_session` and `list_sessions`, set `seed: self.session_seed(id)?` next to `moods: self.session_moods(id)?`.

- [ ] **Step 5: Run** tests → PASS; full gates (includes the existing `opening_twice_is_idempotent` migration test).

---

### Task 4: Radio filtering + seed choice (pure)

**Files:**
- Create: `src-tauri/src/mood_engine/link_radio.rs`
- Modify: `src-tauri/src/mood_engine/mod.rs` (`pub mod link_radio;`)

**Interfaces:**
- Consumes: `candidates::{dedup, filter_non_music, filter_out_unavailable}`, `scoring::ScoringContext`.
- Produces:
  - `pub const RADIO_FETCH_LIMIT: u32 = 30;`
  - `pub fn filter_radio(raw: Vec<Track>, exclude_ids: &HashSet<String>, ctx: &ScoringContext) -> Vec<Track>` — keeps input order.
  - `pub fn next_seed(queue: &[Track], original_seed_id: &str) -> String` — id of the last track in `queue` (the full `Queue::all_tracks()` slice), else the original seed.

- [ ] **Step 1: Write module with tests first**

```rust
//! Link radio: turning a YouTube Mix into queue candidates. Pure and
//! network-free -- the yt-dlp call lives in `Resolver::radio`, the
//! orchestration in `commands::link`.

use std::collections::HashSet;

use crate::models::Track;
use crate::mood_engine::candidates;
use crate::mood_engine::scoring::ScoringContext;

/// Enough for ~an hour of music per fetch while staying one quick yt-dlp
/// call; top-up runs again long before it's exhausted.
pub const RADIO_FETCH_LIMIT: u32 = 30;

/// Same filters mood candidates get (dedup, non-music, known-unavailable)
/// plus disliked tracks and anything already in this session -- but no
/// score shuffle: Mix order is what carries the similarity.
pub fn filter_radio(
    raw: Vec<Track>,
    exclude_ids: &HashSet<String>,
    ctx: &ScoringContext,
) -> Vec<Track> {
    let musical = candidates::filter_non_music(candidates::dedup(raw));
    candidates::filter_out_unavailable(musical, |id| ctx.unavailable_tracks.contains(id))
        .into_iter()
        .filter(|t| !exclude_ids.contains(&t.id))
        .filter(|t| ctx.feedback.get(&t.id) != Some(&false))
        .collect()
}

/// The Mix to fetch next continues from the newest track in the queue,
/// so the radio drifts naturally; with an empty queue, the original seed.
pub fn next_seed(queue: &[Track], original_seed_id: &str) -> String {
    queue
        .last()
        .map(|t| t.id.clone())
        .unwrap_or_else(|| original_seed_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: format!("Song {id}"),
            artist: Some("Artist".into()),
            duration_seconds: Some(200),
            thumbnail_url: None,
        }
    }

    fn ids(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn filter_radio_keeps_mix_order() {
        let out = filter_radio(
            vec![track("c"), track("a"), track("b")],
            &HashSet::new(),
            &ScoringContext::default(),
        );
        assert_eq!(ids(&out), ["c", "a", "b"]);
    }

    #[test]
    fn filter_radio_drops_session_tracks_disliked_unavailable_and_dupes() {
        let mut ctx = ScoringContext::default();
        ctx.feedback.insert("disliked".into(), false);
        ctx.feedback.insert("liked".into(), true);
        ctx.unavailable_tracks.insert("gone".into());
        let exclude: HashSet<String> = ["played".to_string()].into();
        let out = filter_radio(
            vec![
                track("played"),
                track("disliked"),
                track("gone"),
                track("liked"),
                track("new"),
                track("new"),
            ],
            &exclude,
            &ctx,
        );
        assert_eq!(ids(&out), ["liked", "new"]);
    }

    #[test]
    fn filter_radio_of_all_seen_tracks_is_empty() {
        let exclude: HashSet<String> = ["a".to_string(), "b".to_string()].into();
        let out = filter_radio(vec![track("a"), track("b")], &exclude, &ScoringContext::default());
        assert!(out.is_empty());
    }

    #[test]
    fn next_seed_is_the_last_queued_track() {
        assert_eq!(next_seed(&[track("a"), track("b")], "seed"), "b");
    }

    #[test]
    fn next_seed_falls_back_to_the_original_seed_on_an_empty_queue() {
        assert_eq!(next_seed(&[], "seed"), "seed");
    }
}
```

Check that `filter_non_music` keeps a 200s track titled `Song x` by artist `Artist` (it should; if its denylist trips on these names, change the fixture titles, not the filter).

- [ ] **Step 2: Run** `cargo test --lib link_radio` → PASS (logic is trivial enough that tests and code land together; confirm each test fails if you invert its filter line). Full gates.

---

### Task 5: Commands — start + top-up

**Files:**
- Create: `src-tauri/src/commands/link.rs`
- Modify: `src-tauri/src/commands/mod.rs` (`pub mod link;`)
- Modify: `src-tauri/src/commands/queue.rs` (`ensure_queue_topped_up` branch)
- Modify: `src-tauri/src/lib.rs` (register `commands::link::start_link_session` in `generate_handler!`, next to `start_mixed_session`). App commands are exposed via `generate_handler!`; `capabilities/default.json` holds only plugin permissions, so no change there.

**Interfaces:**
- Consumes: `metadata::youtube_id_from_link`, `Resolver::radio`, `Db::start_link_session`, `link_radio::{filter_radio, next_seed, RADIO_FETCH_LIMIT}`, `commands::{record_current_completion, resolve_and_load}`, `mood_engine::build_scoring_context`, `GenerationConfig::default().recent_session_window`.
- Produces: `#[tauri::command] pub async fn start_link_session(app, state, url: String) -> Result<SessionInfo>`; `pub(crate) async fn top_up_link_queue(app: &tauri::AppHandle, state: &AppState, original_seed_id: &str) -> Result<()>`; pure `pub(crate) fn split_seed(mix: Vec<Track>, seed_id: &str) -> Result<(Track, Vec<Track>)>`.

- [ ] **Step 1: Failing tests for the pure helper** (in `link.rs` test module)

```rust
    #[test]
    fn split_seed_takes_the_seed_out_of_the_mix() {
        let (seed, rest) = split_seed(vec![track("s"), track("a"), track("b")], "s").unwrap();
        assert_eq!(seed.id, "s");
        assert_eq!(rest.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn split_seed_finds_the_seed_anywhere_in_the_mix() {
        let (seed, rest) = split_seed(vec![track("a"), track("s")], "s").unwrap();
        assert_eq!(seed.id, "s");
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn split_seed_errors_when_the_mix_is_empty_or_lacks_the_seed() {
        for mix in [vec![], vec![track("a")]] {
            let err = split_seed(mix, "s").unwrap_err();
            assert!(matches!(err, EchoraError::MixUnavailable), "{err:?}");
        }
    }
```

(with the same `track(id)` fixture as Task 4.)

- [ ] **Step 2: Implement `link.rs`**

```rust
use std::collections::HashSet;

use tauri::State;

use crate::error::{EchoraError, Result};
use crate::media::metadata;
use crate::models::{SessionInfo, Track};
use crate::mood_engine::link_radio::{self, RADIO_FETCH_LIMIT};
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// Pulls the seed out of its own Mix (YouTube puts it first, but don't
/// rely on it). No seed in the Mix means YouTube had nothing for it.
pub(crate) fn split_seed(mut mix: Vec<Track>, seed_id: &str) -> Result<(Track, Vec<Track>)> {
    let index = mix
        .iter()
        .position(|t| t.id == seed_id)
        .ok_or(EchoraError::MixUnavailable)?;
    let seed = mix.remove(index);
    Ok((seed, mix))
}

fn scoring_context(state: &AppState) -> Result<mood_engine::scoring::ScoringContext> {
    let db = state.db.lock().unwrap();
    mood_engine::build_scoring_context(&db, GenerationConfig::default().recent_session_window)
}

/// Starts link radio from a pasted YouTube link. The Mix is fetched
/// before anything about the current session changes, so a bad link or
/// an empty Mix leaves whatever is playing untouched.
#[tauri::command]
pub async fn start_link_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<SessionInfo> {
    let seed_id = metadata::youtube_id_from_link(&url)?;
    // A yt-dlp timeout/garbled output at start reads as "no mix" to the
    // user; a private/removed seed keeps its specific message.
    let mix = state
        .resolver
        .radio(&app, &seed_id, RADIO_FETCH_LIMIT)
        .await
        .map_err(|err| match err {
            EchoraError::TrackUnavailable(_) => err,
            _ => EchoraError::MixUnavailable,
        })?;
    let (seed, rest) = split_seed(mix, &seed_id)?;

    let ctx = scoring_context(&state)?;
    let exclude: HashSet<String> = [seed.id.clone()].into();
    let upcoming = link_radio::filter_radio(rest, &exclude, &ctx);

    super::record_current_completion(&state).await?;
    let session = state.db.lock().unwrap().start_link_session(&seed)?;
    {
        let mut queue = state.queue.lock().unwrap();
        queue.clear();
        queue.add_candidates(std::iter::once(seed.clone()).chain(upcoming));
    }
    super::resolve_and_load(&app, &state, &seed).await?;
    Ok(session)
}

/// Link-radio counterpart of `top_up_queue`: next Mix from the newest
/// queued track; if everything in it was already heard this session,
/// one retry from the original seed, then give up quietly (next top-up
/// tries again).
pub(crate) async fn top_up_link_queue(
    app: &tauri::AppHandle,
    state: &AppState,
    original_seed_id: &str,
) -> Result<()> {
    let (seed_id, exclude) = {
        let queue = state.queue.lock().unwrap();
        let all = queue.all_tracks();
        let exclude: HashSet<String> = all.iter().map(|t| t.id.clone()).collect();
        (link_radio::next_seed(all, original_seed_id), exclude)
    };
    let ctx = scoring_context(state)?;

    let mut fresh = link_radio::filter_radio(
        state.resolver.radio(app, &seed_id, RADIO_FETCH_LIMIT).await?,
        &exclude,
        &ctx,
    );
    if fresh.is_empty() && seed_id != original_seed_id {
        fresh = link_radio::filter_radio(
            state.resolver.radio(app, original_seed_id, RADIO_FETCH_LIMIT).await?,
            &exclude,
            &ctx,
        );
    }
    state.queue.lock().unwrap().add_candidates(fresh);
    Ok(())
}
```

Notes for the implementer:
- `queue.add_candidates` on an empty queue makes the first track `current` (verified in `queue.rs`), so the seed is current after `clear()` + `add_candidates`.
- `resolve_and_load` takes `&tauri::AppHandle` (not generic) — so `top_up_link_queue` uses `&tauri::AppHandle` too.
- `Resolver::radio` failing mid-session propagates an error from `ensure_queue_topped_up`, same as the mood path's search failure today.

- [ ] **Step 3: Branch `ensure_queue_topped_up`** in `commands/queue.rs`: replace the moods lookup with

```rust
    let session = match state.db.lock().unwrap().current_session()? {
        Some(session) => session,
        None => return Ok(()),
    };
    if let Some(seed) = session.seed {
        return super::link::top_up_link_queue(&app, &state, &seed.id).await;
    }
    let moods: Vec<(String, u8)> = session
        .moods
        .into_iter()
        .map(|m| (m.mood_id, m.weight))
        .collect();

    super::top_up_queue(&app, &state, &moods).await
```

and update its doc comment ("using the active session's moods, or its link-radio seed").

- [ ] **Step 4: Register** `commands::link::start_link_session,` in `lib.rs`'s `generate_handler!`.

- [ ] **Step 5: Run** `cargo test --lib split_seed` → PASS; full gates.

---

### Task 6: Frontend

**Files:**
- Modify: `src/lib/api.ts` (types + `startLinkSession`)
- Modify: `src/App.tsx` (`handleStartLink`, label state)
- Modify: `src/components/HomeView.tsx` (input row)
- Modify: `src/components/LibraryTab.tsx` (seed rows)
- Modify: `src/styles.css` (`.link-radio` styles)

**Interfaces:**
- Consumes: Rust command `start_link_session { url }` → `SessionInfo` with `seed: Track | null`.
- Produces: `api.startLinkSession(url: string): Promise<SessionInfo>`; `HomeView` prop `onStartLink: (url: string) => Promise<boolean>` (true on success); `LibraryTab` prop `onStartLink: (url: string) => Promise<boolean> | void`.

- [ ] **Step 1: api.ts**

```ts
export interface SessionInfo {
  id: number;
  moods: SessionMood[];
  /** Track a link-radio session started from; null for mood sessions. */
  seed: Track | null;
  started_at: number;
  ended_at: number | null;
}
```

(`Track` must be declared before or hoisted — interfaces hoist, fine.) In `api`:

```ts
  startLinkSession: (url: string) => call<SessionInfo>("start_link_session", { url }),
```

- [ ] **Step 2: App.tsx** — add `const [currentSeedTitle, setCurrentSeedTitle] = useState<string | null>(null);`. In every existing handler that calls `setCurrentMoods(...)`, also call `setCurrentSeedTitle(null)`. Add:

```tsx
  const handleStartLink = useCallback(
    async (url: string) => {
      setStartingMoodId("link");
      try {
        const session = await api.startLinkSession(url);
        setCurrentMoods(null);
        setCurrentSeedTitle(session.seed?.title ?? null);
        await playback.refreshQueue();
        setPlayerExpanded(true);
        return true;
      } catch (err) {
        reportError(messageOf(err));
        return false;
      } finally {
        setStartingMoodId(null);
      }
    },
    [playback, reportError],
  );
```

Change the label:

```tsx
  const currentMoodName = currentSeedTitle
    ? `Mix · ${currentSeedTitle}`
    : currentMoods
      ? currentMoods
          .map((m) => moodsData.moods.find((mood) => mood.id === m.mood_id)?.name ?? "Unknown mood")
          .join(" + ")
      : null;
```

Pass `onStartLink={handleStartLink}` to `HomeView` and `LibraryTab` (find where each is rendered).

- [ ] **Step 3: HomeView.tsx** — add prop `onStartLink: (url: string) => Promise<boolean>;`, state `const [link, setLink] = useState("");`, and right after the Surprise Me `</button>`:

```tsx
      <form
        className="link-radio"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!link.trim()) return;
          if (await onStartLink(link)) setLink("");
        }}
      >
        <input
          className="link-radio__input"
          type="url"
          inputMode="url"
          value={link}
          onChange={(e) => setLink(e.target.value)}
          placeholder="Paste a YouTube link to start a mix"
          aria-label="YouTube link"
          disabled={busy || mixMode}
          spellCheck={false}
        />
        <button className="link-radio__button" type="submit" disabled={busy || mixMode || !link.trim()}>
          {startingMoodId === "link" ? "Starting…" : "Start mix"}
        </button>
      </form>
```

Use `type="text"` instead of `type="url"` if the native URL validation blocks `youtu.be/…` without a scheme (it would — so use `type="text"`; Rust validates).

- [ ] **Step 4: styles.css** — next to `.mix-toggle`, reusing its tokens:

```css
.link-radio {
  display: flex;
  gap: 8px;
  margin-bottom: 12px;
}
.link-radio__input {
  flex: 1;
  min-width: 0;
  min-height: 44px;
  padding: 0 14px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border);
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
}
.link-radio__input:focus-visible {
  outline: none;
  border-color: var(--accent);
}
.link-radio__button {
  min-height: 44px;
  padding: 0 16px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--accent);
  background: transparent;
  color: var(--text-primary);
  font-size: 13px;
  font-weight: 600;
}
.link-radio__input:disabled,
.link-radio__button:disabled {
  opacity: 0.5;
}
```

- [ ] **Step 5: LibraryTab.tsx** — add prop `onStartLink`; replace the history row's `onClick` and title:

```tsx
              onClick={() =>
                session.seed
                  ? onStartLink(`https://youtu.be/${session.seed.id}`)
                  : session.moods.length > 1
                    ? onStartMix(session.moods)
                    : onStartMood(session.moods[0].mood_id)
              }
            >
              <span className="library-row__title">
                {session.seed
                  ? `Mix · ${session.seed.title}`
                  : session.moods.map((m) => moodName(m.mood_id)).join(" + ")}
              </span>
```

Grep the frontend for any other `.moods[0]` / `session.moods` use (e.g. StatsTab, DiscoverView) and guard the empty case the same way.

- [ ] **Step 6: Run** `npm run lint && npm run build` → clean. Then full Rust gates once more.

---

## Final verification (orchestrator)

- `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
- `npm run lint && npm run build`
- If dev sidecars present: `cargo test --lib radio_for_a_real_track -- --ignored`
- Hand live-app testing to the user: paste a link → plays seed then similar; let queue run low → keeps going; pick a mood → radio stops; bad link → banner, current playback untouched; Library row → replays.
