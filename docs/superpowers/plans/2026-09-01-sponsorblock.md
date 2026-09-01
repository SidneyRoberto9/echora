# SponsorBlock Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing `sponsorblock_categories` Settings toggle actually
skip matching segments during playback, by querying the SponsorBlock API
per track and seeking past matches via mpv's existing IPC connection.

**Architecture:** One new file, `src-tauri/src/media/sponsorblock.rs`, holds
a pure JSON-parsing function (`parse_segments`, TDD'd against a fixture —
no network involved), a blocking-HTTP fetch function wrapped in
`spawn_blocking` (`fetch_segments`), and a single long-running background
task (`watch`) that both detects track changes (by polling
`queue.current()`) and polls playback position to trigger skips. Centralizing
both jobs in one task, spawned once at startup with an owned `AppHandle`,
avoids threading a new dependency through the existing
`resolve_and_load`/`top_up_queue` call chain — `queue.current()` already
reflects every playback entry point uniformly. One small, independent
frontend task adds the licensing-required attribution line to Settings.

**Tech Stack:** Tauri 2, Rust (`attohttpc` for HTTP, `sha2` for the
K-Anonymity hash), React + TypeScript (attribution copy only, no new
component).

**Spec:** `docs/superpowers/specs/2026-09-01-sponsorblock-design.md`

## Global Constraints

- Package manager: npm only (repo-wide, see CLAUDE.md).
- No persistence of segment data beyond the current playback session — no
  disk cache, no export, no re-serving. This is a licensing mitigation
  (ADR 0009), not a style preference — segments live only in
  `AppState.sponsorblock_segments` and are replaced/cleared on every track
  change.
- No visual indicator when a segment is skipped — silent seek only.
- The fetch is always async/best-effort: playback never waits on it, and
  any failure (network, parse, non-2xx/404) results in "no segments for
  this track," never a surfaced error.
- `attohttpc` dependency uses `default-features = false, features = ["json",
  "tls-rustls-native-roots"]` — never `tls-native` (would link
  `native-tls`/OpenSSL, which this project avoids for portable-binary
  reasons).
- Every command run before claiming a task done: `cargo fmt --check &&
  cargo clippy --all-targets -- -D warnings && cargo test` (from
  `src-tauri/`) and `npm run lint && npm run build` (frontend task).

---

### Task 1: Backend — SponsorBlock fetch and skip-watcher

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/error.rs`
- Create: `src-tauri/src/media/sponsorblock.rs`
- Create: `src-tauri/src/media/fixtures/sponsorblock_response.json`
- Modify: `src-tauri/src/media/mod.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `media::sponsorblock::Segment { start: f64, end: f64, category: String }` (`Debug, Clone, PartialEq`); `media::sponsorblock::fetch_segments(video_id: &str, categories: &[String]) -> Result<Vec<Segment>>`; `media::sponsorblock::watch(app: AppHandle) -> impl Future<Output = ()>` (runs forever, spawned once); `AppState.sponsorblock_segments: Mutex<Vec<Segment>>`; `EchoraError::SponsorBlock(String)`.
- Consumes: `AppState.queue`/`AppState.player`/`AppState.db` (existing), `Db::get_settings()` → `Settings.sponsorblock_categories` (existing), `Player::position_seconds()`/`Player::seek_to()` (existing).

- [ ] **Step 1: Add the new dependencies**

In `src-tauri/Cargo.toml`, add to `[dependencies]`:

```toml
attohttpc = { version = "0.31.0", default-features = false, features = ["json", "tls-rustls-native-roots"] }
sha2 = "0.11.0"
```

- [ ] **Step 2: Add the error variant**

In `src-tauri/src/error.rs`, add to the `EchoraError` enum (after
`UnknownMood`... or wherever fits alongside the other single-String
variants):

```rust
    #[error("sponsorblock error: {0}")]
    SponsorBlock(String),
```

And to `code()`:

```rust
            EchoraError::SponsorBlock(_) => "sponsorblock_error",
```

- [ ] **Step 3: Write the fixture**

`src-tauri/src/media/fixtures/sponsorblock_response.json` — models the
real SponsorBlock hash-prefix response shape: two videos sharing a hash
prefix (K-Anonymity always returns multiple candidates), and one video
with a mix of a skip-type and a mute-type segment, to exercise both the
exact-`videoID` filter and the `actionType == "skip"` filter:

```json
[
  {
    "videoID": "dQw4w9WgXcQ",
    "segments": [
      {
        "segment": [10.5, 45.2],
        "category": "sponsor",
        "actionType": "skip"
      },
      {
        "segment": [200.0, 210.0],
        "category": "selfpromo",
        "actionType": "mute"
      }
    ]
  },
  {
    "videoID": "otherVideoSharingPrefix",
    "segments": [
      {
        "segment": [5.0, 15.0],
        "category": "intro",
        "actionType": "skip"
      }
    ]
  }
]
```

- [ ] **Step 4: Write the failing test for `parse_segments`**

Create `src-tauri/src/media/sponsorblock.rs` with just enough to compile a
failing test — the module skeleton, types, and an empty `parse_segments`
stub are written together with the test in this step (there's no
meaningful intermediate "compiles but does nothing" state worth a separate
commit here — the test and the types it references are one unit):

```rust
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::error::{EchoraError, Result};
use crate::state::AppState;

const API_BASE: &str = "https://sponsor.ajay.app/api/skipSegments";
const USER_AGENT: &str = "Echora/0.1 (+https://github.com/SidneyRoberto9/echora)";

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub category: String,
}

#[derive(Debug, Deserialize)]
struct VideoEntry {
    #[serde(rename = "videoID")]
    video_id: String,
    segments: Vec<RawSegment>,
}

#[derive(Debug, Deserialize)]
struct RawSegment {
    segment: [f64; 2],
    category: String,
    #[serde(rename = "actionType")]
    action_type: String,
}

/// Parses a raw SponsorBlock hash-prefix response body, keeping only the
/// entry matching `video_id` exactly (the API returns every video sharing
/// the K-Anonymity hash prefix) and only its skip-type segments (`"mute"`
/// and highlight-only markers don't apply to a plain seek-past skip).
fn parse_segments(body: &str, video_id: &str) -> Result<Vec<Segment>> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_segments_keeps_only_the_matching_video_and_skip_segments() {
        let body = include_str!("fixtures/sponsorblock_response.json");
        let segments = parse_segments(body, "dQw4w9WgXcQ").unwrap();

        assert_eq!(
            segments,
            vec![Segment {
                start: 10.5,
                end: 45.2,
                category: "sponsor".into(),
            }]
        );
    }

    #[test]
    fn parse_segments_returns_empty_for_a_video_id_not_in_the_response() {
        let body = include_str!("fixtures/sponsorblock_response.json");
        let segments = parse_segments(body, "not-in-the-fixture").unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn parse_segments_errors_on_malformed_json() {
        let err = parse_segments("not json", "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, EchoraError::SponsorBlock(_)));
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cd src-tauri && cargo test media::sponsorblock::tests:: -- --skip errors_on_malformed`
Expected: FAIL (`not yet implemented` panic from `todo!()`) for the first
two tests. The malformed-JSON test may also fail/panic the same way — that's
fine, all three are expected red at this point.

- [ ] **Step 6: Implement `parse_segments`**

Replace the `todo!()` body:

```rust
fn parse_segments(body: &str, video_id: &str) -> Result<Vec<Segment>> {
    let entries: Vec<VideoEntry> =
        serde_json::from_str(body).map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    Ok(entries
        .into_iter()
        .find(|entry| entry.video_id == video_id)
        .map(|entry| {
            entry
                .segments
                .into_iter()
                .filter(|s| s.action_type == "skip")
                .map(|s| Segment {
                    start: s.segment[0],
                    end: s.segment[1],
                    category: s.category,
                })
                .collect()
        })
        .unwrap_or_default())
}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cd src-tauri && cargo test media::sponsorblock::tests::`
Expected: PASS, all 3 tests green

- [ ] **Step 8: Add the network fetch and the background watcher**

Append to `src-tauri/src/media/sponsorblock.rs` (after `parse_segments`,
before the `#[cfg(test)]` module):

```rust
/// Fetches SponsorBlock's segment data for exactly `video_id`, filtered to
/// `categories`. Ephemeral: nothing here is cached or written to disk (see
/// ADR 0009's licensing mitigation) — the caller holds the result only in
/// memory for the current playback session. Any failure (network, parse,
/// unexpected status) is a normal `Err` here — callers treat it as
/// best-effort and discard it rather than surfacing it to the user.
pub async fn fetch_segments(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    let video_id_owned = video_id.to_string();
    let categories_owned = categories.to_vec();
    tokio::task::spawn_blocking(move || fetch_segments_blocking(&video_id_owned, &categories_owned))
        .await
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?
}

fn fetch_segments_blocking(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    let hash = Sha256::digest(video_id.as_bytes());
    let prefix = format!("{:x}", hash)[..4].to_string();
    let categories_json = serde_json::to_string(categories)
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    let url = format!("{API_BASE}/{prefix}");
    let response = attohttpc::get(&url)
        .param("categories", categories_json)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    if response.status() == attohttpc::StatusCode::NOT_FOUND {
        // No submissions for any video sharing this hash prefix — normal,
        // not a failure.
        return Ok(Vec::new());
    }
    if !response.is_success() {
        return Err(EchoraError::SponsorBlock(format!(
            "unexpected status {}",
            response.status()
        )));
    }

    let body = response
        .text()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;
    parse_segments(&body, video_id)
}

/// Runs forever: once a second, detects whether the current track changed
/// (comparing `queue.current()`'s id against what this loop last saw) and,
/// on change, clears any stale segments immediately and kicks off a fetch
/// for the new track — then, every tick regardless, checks the player's
/// position against whatever segments are currently loaded and seeks past
/// a match. One task does both jobs so nothing needs to reach back into
/// `commands::resolve_and_load` or thread an `AppHandle` through the
/// existing command call chain — `queue.current()` already reflects every
/// playback entry point (mood session, scene, single favorited track)
/// uniformly.
pub async fn watch(app: AppHandle) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut last_track_id: Option<String> = None;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();

        let current_track = state.queue.lock().unwrap().current().cloned();
        let current_id = current_track.as_ref().map(|t| t.id.clone());

        if current_id != last_track_id {
            last_track_id = current_id;
            *state.sponsorblock_segments.lock().unwrap() = Vec::new();

            if let Some(track) = current_track {
                let categories = state
                    .db
                    .lock()
                    .unwrap()
                    .get_settings()
                    .map(|s| s.sponsorblock_categories)
                    .unwrap_or_default();

                if !categories.is_empty()
                    && let Ok(segments) = fetch_segments(&track.id, &categories).await
                {
                    let still_current = state
                        .queue
                        .lock()
                        .unwrap()
                        .current()
                        .is_some_and(|t| t.id == track.id);
                    if still_current {
                        *state.sponsorblock_segments.lock().unwrap() = segments;
                    }
                }
            }
            continue;
        }

        let segments = state.sponsorblock_segments.lock().unwrap().clone();
        if segments.is_empty() {
            continue;
        }

        let player = state.player.lock().await;
        let Ok(Some(position)) = player.position_seconds().await else {
            continue;
        };
        if let Some(hit) = segments
            .iter()
            .find(|s| position >= s.start && position < s.end)
        {
            let _ = player.seek_to(hit.end).await;
        }
    }
}
```

- [ ] **Step 9: Add a real-network smoke test**

Append to the `#[cfg(test)]` module in `src-tauri/src/media/sponsorblock.rs`:

```rust
    #[tokio::test]
    #[ignore]
    async fn fetching_segments_for_a_video_with_known_sponsors_returns_some() {
        // Rick Astley - Never Gonna Give You Up: long-standing, heavily
        // annotated video, stable real-network fixture for this smoke test.
        let segments = fetch_segments(
            "dQw4w9WgXcQ",
            &["sponsor".to_string(), "selfpromo".to_string()],
        )
        .await
        .unwrap();
        // Not asserting exact segment count/timestamps — community data
        // changes over time. Only proving the real request/parse path works.
        assert!(!segments.is_empty(), "expected at least one real segment");
    }
```

- [ ] **Step 10: Register the module**

In `src-tauri/src/media/mod.rs`, add:

```rust
pub mod sponsorblock;
```

(alongside the existing `pub mod metadata;` etc., alphabetically after
`sidecar_paths`)

- [ ] **Step 11: Add the new `AppState` field**

In `src-tauri/src/state.rs`, add to the `AppState` struct (check the exact
current field list and imports before editing — add `use crate::media;` or
reference the type via its full path if `media` isn't already imported):

```rust
    /// Skip-segment data for whatever track is currently playing, kept only
    /// in memory for the current playback session (see ADR 0009) —
    /// populated/cleared by `media::sponsorblock::watch`.
    pub sponsorblock_segments: Mutex<Vec<media::sponsorblock::Segment>>,
```

- [ ] **Step 12: Wire it into `lib.rs`**

In `src-tauri/src/lib.rs`, inside `app.manage(AppState { ... })`, add:

```rust
                sponsorblock_segments: Mutex::new(Vec::new()),
```

After the existing `platform::autostart::sync(app.handle(), autostart_enabled)?;` line, before the closing `Ok(())` of the `setup` closure:

```rust
            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));
```

- [ ] **Step 13: Run the full backend test suite**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, zero warnings (the new `#[ignore]`d smoke test doesn't run
by default, matching every other real-network smoke test in this codebase)

- [ ] **Step 14: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/error.rs \
  src-tauri/src/media/sponsorblock.rs \
  src-tauri/src/media/fixtures/sponsorblock_response.json \
  src-tauri/src/media/mod.rs src-tauri/src/state.rs src-tauri/src/lib.rs
git commit -m "feat(sponsorblock): fetch and skip segments during playback"
```

---

### Task 2: Frontend — SponsorBlock attribution in Settings

Required by the licensing mitigation (ADR 0009: "Attribution ... surfaced
in Settings/About"). Purely additive, independent of Task 1's backend
work — no shared file, no interface dependency either direction.

**Files:**
- Modify: `src/components/SettingsView.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: none (static copy, no new props/state).
- Produces: none.

- [ ] **Step 1: Add the attribution line**

In `src/components/SettingsView.tsx`, right after the existing
`{SPONSORBLOCK_CATEGORIES.map((category) => ( ... ))}` block closes and
before the next `<h2 className="settings-section__title">Startup</h2>`:

```tsx
        <p className="settings-section__hint">
          Segment data from{" "}
          <a href="https://sponsor.ajay.app" target="_blank" rel="noreferrer">
            SponsorBlock
          </a>
          .
        </p>
```

- [ ] **Step 2: Add the CSS**

In `src/styles.css`, after the `.settings-row__label` block:

```css
.settings-section__hint {
  font-size: 12px;
  color: var(--text-secondary);
  margin: 4px 0 0 0;
}
```

- [ ] **Step 3: Verify frontend build**

Run: `npm run lint && npm run build`
Expected: PASS, zero errors

- [ ] **Step 4: Commit**

```bash
git add src/components/SettingsView.tsx src/styles.css
git commit -m "feat(sponsorblock): attribute SponsorBlock in Settings"
```
