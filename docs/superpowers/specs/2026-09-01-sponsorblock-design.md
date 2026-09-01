# SponsorBlock — Design

Status: Approved. Second of the post-v1-audit features (SponsorBlock →
Intensity → Auto-update → Crash report → Packaging), per the user's
explicit priority order. Free-text search was removed from scope in the
same session (dead IPC surface, no UI ever existed for it).

## Purpose

Echora already has a `sponsorblock_categories: Vec<String>` toggle list in
Settings (sponsor/selfpromo/intro/outro), but nothing acts on it — it's
stored and never read outside the Settings round-trip. This feature makes
it real: while a track plays, Echora fetches SponsorBlock's community
segment data for that YouTube video and seeks past any segment matching an
enabled category, silently (no visual indicator — matches the existing
minimalist direction).

## Licensing (resolved before this spec, not re-litigated here)

SponsorBlock's database/API content is CC BY-NC-SA 4.0. A compliance
review (2026-09-01) found **RISK (low), not BLOCKER** — see
`docs/adr/0009-sponsorblock-api-noncommercial-license.md` for the full
reasoning and `THIRD_PARTY_NOTICES.md` for the entry. The user chose to
proceed under the review's mitigations **without** contacting the
SponsorBlock maintainer for written confirmation. This spec's design
follows those mitigations as hard requirements, not suggestions:
ephemeral per-video queries only (no bulk/database download), no
persistence beyond the current playback session, attribution in
`THIRD_PARTY_NOTICES.md` (already done) and Settings/About (this spec adds
it), and no monetization tied to this feature.

## Non-goals (v1)

- No visual indicator when a segment is skipped (decided in brainstorm —
  silent skip only, matches DESIGN.md's minimalist direction, no new UI
  component).
- No synchronous/blocking fetch — every track starts playing immediately;
  segment data arrives best-effort, async. A sponsor segment inside the
  first ~1-2 seconds of a track would not be caught (fetch + one watcher
  tick take a moment) — accepted, since SponsorBlock segments are
  essentially never that early in practice.
- No caching of segment data across sessions or across replays of the same
  video — every play re-fetches. Required by the licensing mitigation
  (no persistence beyond the current playback session), not just laziness.
- No change to the Settings UI — `sponsorblock_categories` already exists
  and is already wired to `Settings`/`SettingsView.tsx`; this spec makes it
  functional, not visible-for-the-first-time. One small addition: an
  attribution line in Settings (see Frontend section).
- No handling of `actionType` values other than `"skip"` (SponsorBlock also
  has `"mute"`, `"full"`/highlight-only "poi" markers) — Echora only ever
  seeks past a range, so only skip-type segments apply.

## Backend (Rust)

All new code lives in one new file, `src-tauri/src/media/sponsorblock.rs`
(alongside `player.rs`/`resolver.rs` — this is core playback behavior, not
a `platform/` OS-integration concern like MPRIS/tray).

### New dependencies (`src-tauri/Cargo.toml`)

```toml
attohttpc = { version = "0.31.0", default-features = false, features = ["json", "tls-rustls-native-roots"] }
sha2 = "0.11.0"
```

`attohttpc`'s default features are `["compress", "tls-native"]` — both
dropped: `tls-native` links `native-tls` (OpenSSL on Linux), which Echora
doesn't want as a system dependency for a portable binary; `compress`
(response compression) is unneeded for a payload this small. Chosen over
`reqwest` (Echora's only other viable option) because this is exactly one
blocking-shaped GET-and-parse-JSON call, not a client used throughout the
app — `attohttpc`'s ~4-6 transitive deps vs. `reqwest`'s ~40+ even
minimally configured is the right trade for priority #1 (lightness).
`sha2` is pure Rust, zero extra dependencies, used only for the
K-Anonymity hash-prefix scheme SponsorBlock's API requires.

### `src-tauri/src/error.rs`

```rust
    #[error("sponsorblock error: {0}")]
    SponsorBlock(String),
```

And in `code()`:

```rust
            EchoraError::SponsorBlock(_) => "sponsorblock_error",
```

### `src-tauri/src/media/sponsorblock.rs`

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

/// Fetches SponsorBlock's segment data for exactly `video_id`, filtered to
/// `categories` and to skip-type segments (`actionType == "skip"` — mute
/// and highlight-only markers don't apply to a plain seek-past skip).
/// Ephemeral: nothing here is cached or written to disk (see ADR 0009's
/// licensing mitigation) — callers hold the result only in memory for the
/// current playback session.
pub async fn fetch_segments(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    let video_id = video_id.to_string();
    let categories = categories.to_vec();
    tokio::task::spawn_blocking(move || fetch_segments_blocking(&video_id, &categories))
        .await
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?
}

fn fetch_segments_blocking(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    let hash = Sha256::digest(video_id.as_bytes());
    let prefix = format!("{:x}", hash)[..4].to_string();
    let categories_json =
        serde_json::to_string(categories).map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    let url = format!("{API_BASE}/{prefix}");
    let response = attohttpc::get(&url)
        .param("categories", categories_json)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    if response.status() == attohttpc::StatusCode::NOT_FOUND {
        // No submissions for any video sharing this hash prefix — a
        // completely normal, expected outcome, not a failure.
        return Ok(Vec::new());
    }
    if !response.is_success() {
        return Err(EchoraError::SponsorBlock(format!(
            "unexpected status {}",
            response.status()
        )));
    }

    let entries: Vec<VideoEntry> = response
        .json()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

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
                    // The track may have changed again while this fetch was
                    // in flight — only apply if it's still current.
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
        if let Some(hit) = segments.iter().find(|s| position >= s.start && position < s.end) {
            let _ = player.seek_to(hit.end).await;
        }
    }
}
```

### `src-tauri/src/state.rs`

```rust
pub struct AppState {
    pub db: Mutex<Db>,
    pub queue: Mutex<Queue>,
    pub moods: MoodCatalog,
    pub resolver: Resolver,
    pub player: tokio::sync::Mutex<Player>,
    pub mpris: Option<mpris::Handle>,
    /// Skip-segment data for whatever track is currently playing, kept only
    /// in memory for the current playback session (see ADR 0009) —
    /// populated/cleared by `media::sponsorblock::watch`.
    pub sponsorblock_segments: Mutex<Vec<media::sponsorblock::Segment>>,
}
```

(`media::sponsorblock::Segment` needs `pub` visibility on the module and
struct, already given above; add `pub mod sponsorblock;` to
`src-tauri/src/media/mod.rs`.)

### `src-tauri/src/lib.rs`

In `app.manage(AppState { ... })`, add:

```rust
                sponsorblock_segments: Mutex::new(Vec::new()),
```

After the existing `platform::autostart::sync(...)` call, before `Ok(())`:

```rust
            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));
```

## Frontend (React)

One small addition for the licensing attribution requirement — a credit
line in Settings, right after the existing
`{SPONSORBLOCK_CATEGORIES.map(...)}` block and before the next
`<h2 className="settings-section__title">` (the "Startup" section) in
`src/components/SettingsView.tsx`:

```tsx
        <p className="settings-section__hint">
          Segment data from{" "}
          <a href="https://sponsor.ajay.app" target="_blank" rel="noreferrer">
            SponsorBlock
          </a>
          .
        </p>
```

New CSS in `src/styles.css`, after the `.settings-row__label` block:

```css
.settings-section__hint {
  font-size: 12px;
  color: var(--text-secondary);
  margin: 4px 0 0 0;
}
```

(No existing hint/help-text class in this file — confirmed by grep — so
this is one new class, not a reuse.)

No other frontend change. `sponsorblock_categories` already round-trips
through Settings; nothing about how segments are fetched or applied is
frontend-visible.

## Error handling / edge cases

- **Network failure, timeout, malformed JSON, non-200/404 status**: caught
  inside `watch`'s `if let Ok(segments) = fetch_segments(...).await` —
  silently no-ops (no segments for that track this session), never
  surfaces an error to the user, never blocks or delays playback. This
  matches the existing best-effort convention already used for MPRIS
  notify and other non-critical background work in this codebase.
- **404 from the API** (no submissions for any video sharing the hash
  prefix): explicitly treated as success-with-empty-list inside
  `fetch_segments_blocking`, not an error path — this is the common case
  for any video with no sponsor segments at all.
- **Track changes again while a fetch is in flight**: the `still_current`
  check discards a stale fetch result rather than applying segments for a
  track that's no longer playing.
- **`sponsorblock_categories` is empty** (user unticked every category):
  `fetch_segments` short-circuits before any network call — checked both
  in the pure function (testable) and implicitly by `watch`'s own
  `!categories.is_empty()` guard.
- **Player not started / IPC hiccup during the position check**: the
  `let-else continue` on `position_seconds()` skips that tick rather than
  panicking or erroring the loop — `watch` must never itself return or
  panic, it runs for the app's whole lifetime.

## Testing

- Rust:
  - `fetch_segments`/`fetch_segments_blocking` parsing logic: unit tests
    against a canned JSON response (fixture, same pattern as
    `src/media/fixtures/resolve_bestaudio.json`) covering: multiple videos
    sharing a hash prefix (only the exact `videoID` match is kept),
    mixed `actionType` values (only `"skip"` survives), an empty
    `categories` input short-circuiting before any request is attempted.
  - A real-network smoke test (`#[ignore]`, same convention as
    `resolver.rs`/`player.rs`'s smoke tests) hitting the real API for a
    known video with well-established sponsor segments.
  - `watch`'s track-change/segment-clearing logic is small enough to not
    need its own harness-heavy test — it's a thin loop over already-tested
    primitives (`fetch_segments`, `Queue::current`, `Player::seek_to`); the
    smoke test above plus manual verification (see final task) cover it.
- Frontend: no test framework change — `npm run lint && npm run build`
  covers the one-paragraph Settings addition.
