# Discord Rich Presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show the currently playing track as a Discord "Listening to Echora" activity status (title, artist, elapsed timer), opt-in and off by default.

**Architecture:** A new `platform::discord` module hand-rolls Discord's local Rich Presence IPC protocol (Unix socket handshake + length-prefixed JSON frames) using only already-present dependencies. A background `tokio` task owns the connection and reconnects on a timer; a `tokio::sync::watch` channel carries the latest track/playback state into it with free debounce. Every place that already tells MPRIS "playback changed" is consolidated to also tell Discord, through one new `platform::notify_playback_changed` function.

**Tech Stack:** Rust (`tokio` `net`/`io-util`/`time` features, `serde_json` — both already dependencies, no new crate), React/TypeScript (one new settings toggle).

**Spec:** `docs/superpowers/specs/2026-09-09-discord-rich-presence-design.md`

## Global Constraints

- No new Cargo or npm dependency for this feature — `tokio` (`net`, `io-util`, `time` features already enabled in `src-tauri/Cargo.toml`) and `serde_json` are sufficient.
- Rust is the source of truth: React only renders one settings toggle and holds no Discord-related logic or state.
- Discord application client ID is `1547402077863542914` (already registered by the maintainer; public value, hardcoded as a constant — not a secret).
- The feature is opt-in and defaults to `false` (`Settings::discord_presence_enabled`), matching `crash_report_enabled`/`autostart_enabled`.
- No per-second refresh of the elapsed timer — send an update only on real events (track change, play, pause, seek); Discord renders `timestamps.start` client-side.
- No album art (`large_image`/`small_image`) — out of scope for v1 (see spec's Non-goals).
- Package manager is npm only for the one frontend change.
- Every commit in this plan is made by invoking the project's `auto-commit` skill — never a raw `git commit`. A `PreToolUse` hook (`~/.claude/hooks/enforce-auto-commit.py`) blocks any commit whose message doesn't follow that skill's Conventional Commits rules, and blocks attribution trailers.
- Before considering the feature done: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (from `src-tauri/`) and `npm run lint && npm run build` (from the repo root) must both pass — see Task 10.

---

### Task 1: Protocol utilities — sanitization and frame encoding

**Files:**
- Create: `src-tauri/src/platform/discord.rs`
- Modify: `src-tauri/src/platform/mod.rs:1-3` (add `pub mod discord;`, alphabetically after `autostart`)

**Interfaces:**
- Consumes: nothing (pure functions, no dependency on other tasks).
- Produces: `pub(crate) fn sanitize_field(input: &str, max_bytes: usize) -> String` and a private `fn encode_frame(opcode: i32, payload: &serde_json::Value) -> Vec<u8>` — both consumed by Task 2's `write_frame` and Task 3's activity builder.

- [ ] **Step 1: Add the module declaration**

In `src-tauri/src/platform/mod.rs`, change:

```rust
pub mod autostart;
pub mod mpris;
pub mod tray;
```

to:

```rust
pub mod autostart;
pub mod discord;
pub mod mpris;
pub mod tray;
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/platform/discord.rs` with only the test module (no implementation yet):

```rust
//! Discord Rich Presence integration. See
//! docs/superpowers/specs/2026-09-09-discord-rich-presence-design.md and
//! docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md. Hand-rolled
//! against Discord's local IPC protocol -- a Unix socket handshake
//! followed by length-prefixed JSON frames -- using only `tokio`
//! (already a dependency; `net`/`io-util`/`time` features already
//! enabled) and `serde_json`. Opt-in, off by default: see
//! `Settings::discord_presence_enabled`.

const MAX_FIELD_BYTES: usize = 128;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_field_strips_control_characters() {
        assert_eq!(sanitize_field("Song\u{0007}Title", MAX_FIELD_BYTES), "SongTitle");
    }

    #[test]
    fn sanitize_field_collapses_whitespace_runs() {
        assert_eq!(
            sanitize_field("Song   Title\n\nHere", MAX_FIELD_BYTES),
            "Song Title Here"
        );
    }

    #[test]
    fn sanitize_field_falls_back_to_echora_when_empty() {
        assert_eq!(sanitize_field("   ", MAX_FIELD_BYTES), "Echora");
        assert_eq!(sanitize_field("", MAX_FIELD_BYTES), "Echora");
    }

    #[test]
    fn sanitize_field_truncates_at_a_utf8_char_boundary() {
        // Each "é" is 2 bytes -- a naive byte-index truncation at an odd
        // offset would split the character and panic or produce garbage.
        let input = "é".repeat(100); // 200 bytes
        let result = sanitize_field(&input, 11); // odd byte budget
        assert!(result.len() <= 11);
        assert!(String::from_utf8(result.into_bytes()).is_ok());
    }

    #[test]
    fn encode_frame_header_matches_opcode_and_payload_length() {
        let payload = serde_json::json!({"a": 1});
        let body = serde_json::to_vec(&payload).unwrap();
        let frame = encode_frame(1, &payload);

        assert_eq!(&frame[0..4], &1i32.to_le_bytes());
        assert_eq!(&frame[4..8], &(body.len() as u32).to_le_bytes());
        assert_eq!(&frame[8..], body.as_slice());
    }
}
```

- [ ] **Step 2b: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib platform::discord`
Expected: compile error — `cannot find function 'sanitize_field'` / `cannot find function 'encode_frame'`.

- [ ] **Step 3: Implement the two functions**

Add above the `#[cfg(test)]` block:

```rust
/// Discord's `details`/`state` activity fields are untrusted-YouTube-
/// metadata boundaries (CLAUDE.md: normalize/validate external metadata
/// before it's used). Strips control characters, collapses whitespace
/// runs, truncates at a UTF-8 char boundary within `max_bytes`, and
/// falls back to "Echora" if nothing printable is left.
pub(crate) fn sanitize_field(input: &str, max_bytes: usize) -> String {
    let cleaned: String = input.chars().filter(|c| !c.is_control()).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = truncate_utf8(&collapsed, max_bytes);
    if truncated.is_empty() {
        "Echora".to_string()
    } else {
        truncated
    }
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// One Discord IPC frame: an opcode, then a little-endian `u32` byte
/// length, then the JSON payload -- no other framing exists in the
/// protocol.
fn encode_frame(opcode: i32, payload: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).expect("activity payload is always valid JSON");
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    buf
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --lib platform::discord`
Expected: 5 tests pass.

- [ ] **Step 5: Commit**

Invoke the `auto-commit` skill (not raw `git commit`) to commit `src-tauri/src/platform/discord.rs` and `src-tauri/src/platform/mod.rs`.

---

### Task 2: IPC transport — socket discovery, handshake, frame read/write

**Files:**
- Modify: `src-tauri/src/platform/discord.rs` (append)

**Interfaces:**
- Consumes: `encode_frame` (Task 1).
- Produces: `async fn connect() -> Option<UnixStream>`, `async fn write_frame(stream: &mut UnixStream, opcode: i32, payload: &serde_json::Value) -> std::io::Result<()>` — both consumed by Task 4's `run()` loop. Also `const OP_HANDSHAKE: i32`, `const OP_FRAME: i32`, `const DISCORD_CLIENT_ID: &str` — `OP_FRAME` is consumed by Task 4.

- [ ] **Step 1: Write the failing test for the pure part**

The actual socket connect/handshake can't be unit-tested without a live Discord process (same precedent as `platform/mpris.rs` and `platform/tray.rs`, which have no unit tests for their D-Bus-facing behavior either — see this task's manual-verification note in Task 10). The one pure, branching piece of this — which paths get tried — is extracted and tested.

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/platform/discord.rs`:

```rust
#[test]
fn candidate_paths_covers_all_ten_discord_ipc_indices() {
    let paths = candidate_paths("/run/user/1000");
    assert_eq!(paths.len(), 10);
    assert_eq!(paths[0], std::path::PathBuf::from("/run/user/1000/discord-ipc-0"));
    assert_eq!(paths[9], std::path::PathBuf::from("/run/user/1000/discord-ipc-9"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib platform::discord`
Expected: compile error — `cannot find function 'candidate_paths'`.

- [ ] **Step 3: Implement the transport layer**

Add near the top of `src-tauri/src/platform/discord.rs` (after the module doc comment, before `MAX_FIELD_BYTES`):

```rust
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const OP_HANDSHAKE: i32 = 0;
const OP_FRAME: i32 = 1;
const DISCORD_CLIENT_ID: &str = "1547402077863542914";
```

Add above the `#[cfg(test)]` block:

```rust
/// Discord's IPC socket lives at `<base>/discord-ipc-<0..9>`, `base`
/// being `$XDG_RUNTIME_DIR` (or `$TMPDIR`, or `/tmp` if neither is
/// set). Multiple indices exist for multiple concurrently-running
/// Discord clients (stable/PTB/canary); trying all ten and taking the
/// first that accepts a connection is standard practice for RPC
/// clients.
fn candidate_paths(base: &str) -> Vec<PathBuf> {
    (0..10)
        .map(|n| PathBuf::from(base).join(format!("discord-ipc-{n}")))
        .collect()
}

fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string())
}

async fn read_frame(stream: &mut UnixStream) -> std::io::Result<(i32, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await?;
    let opcode = i32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok((opcode, body))
}

async fn write_frame(
    stream: &mut UnixStream,
    opcode: i32,
    payload: &serde_json::Value,
) -> std::io::Result<()> {
    stream.write_all(&encode_frame(opcode, payload)).await
}

/// Tries every candidate socket path in order, handshaking with each
/// connection that accepts -- the first that completes the handshake
/// wins. Returns `None` (never an error) if nothing is listening
/// anywhere, which just means Discord isn't running right now; the
/// caller's reconnect loop tries again later.
async fn connect() -> Option<UnixStream> {
    let base = runtime_dir();
    for path in candidate_paths(&base) {
        let Ok(mut stream) = UnixStream::connect(&path).await else {
            continue;
        };
        let handshake = serde_json::json!({ "v": 1, "client_id": DISCORD_CLIENT_ID });
        if write_frame(&mut stream, OP_HANDSHAKE, &handshake).await.is_err() {
            continue;
        }
        if read_frame(&mut stream).await.is_err() {
            continue;
        }
        return Some(stream);
    }
    None
}
```

- [ ] **Step 4: Run to verify the new test passes and nothing else broke**

Run: `cargo test --lib platform::discord`
Expected: 6 tests pass. Also run `cargo clippy --all-targets -- -D warnings` — expect a warning about `connect`/`write_frame`/`read_frame` being unused (they're wired up in Task 4); silence it for now by adding `#[allow(dead_code)]` above `connect`, `write_frame`, and `OP_FRAME`/`DISCORD_CLIENT_ID` if clippy flags them, and remove those `#[allow(dead_code)]` markers in Task 4 once they're actually called.

- [ ] **Step 5: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/platform/discord.rs`.

---

### Task 3: Presence state and activity payload builder

**Files:**
- Modify: `src-tauri/src/platform/discord.rs` (append)

**Interfaces:**
- Consumes: `sanitize_field` (Task 1), `OP_FRAME` (Task 2, used later in Task 4).
- Produces: `pub(crate) enum PresenceState { Idle, Playing { title: String, artist: Option<String>, position_secs: f64 }, Paused { title: String, artist: Option<String> } }` and `fn set_activity_frame(state: &PresenceState) -> serde_json::Value` — both consumed by Task 4.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn activity_value_for_idle_is_null() {
    assert_eq!(activity_value(&PresenceState::Idle, 1000.0), serde_json::Value::Null);
}

#[test]
fn activity_value_for_playing_sets_start_timestamp_from_position() {
    let state = PresenceState::Playing {
        title: "Song".into(),
        artist: Some("Artist".into()),
        position_secs: 30.0,
    };
    let value = activity_value(&state, 1000.0);
    assert_eq!(value["details"], "Song");
    assert_eq!(value["state"], "Artist");
    assert_eq!(value["timestamps"]["start"], 970);
}

#[test]
fn activity_value_for_paused_omits_timestamps_and_labels_state() {
    let state = PresenceState::Paused {
        title: "Song".into(),
        artist: Some("Artist".into()),
    };
    let value = activity_value(&state, 1000.0);
    assert_eq!(value["details"], "Song");
    assert_eq!(value["state"], "Paused — Artist");
    assert!(value.get("timestamps").is_none());
}

#[test]
fn activity_value_sanitizes_title_and_artist() {
    let state = PresenceState::Playing {
        title: "Song\u{0007}".into(),
        artist: Some("  Art   ist  ".into()),
        position_secs: 0.0,
    };
    let value = activity_value(&state, 1000.0);
    assert_eq!(value["details"], "Song");
    assert_eq!(value["state"], "Art ist");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test --lib platform::discord`
Expected: compile error — `cannot find type 'PresenceState'` / `cannot find function 'activity_value'`.

- [ ] **Step 3: Implement**

Add above the `#[cfg(test)]` block:

```rust
/// What's currently true about playback, mapped from the same
/// queue/player state `mpris::notify` already reads. `Idle` covers both
/// "nothing loaded" and "the feature is enabled but disconnected" —
/// either way, Discord shows no activity.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PresenceState {
    Idle,
    Playing {
        title: String,
        artist: Option<String>,
        position_secs: f64,
    },
    Paused {
        title: String,
        artist: Option<String>,
    },
}

fn activity_value(state: &PresenceState, now_unix: f64) -> serde_json::Value {
    match state {
        PresenceState::Idle => serde_json::Value::Null,
        PresenceState::Playing {
            title,
            artist,
            position_secs,
        } => {
            let start = (now_unix - position_secs).max(0.0) as i64;
            serde_json::json!({
                "details": sanitize_field(title, MAX_FIELD_BYTES),
                "state": sanitize_field(artist.as_deref().unwrap_or(""), MAX_FIELD_BYTES),
                "timestamps": { "start": start },
            })
        }
        PresenceState::Paused { title, artist } => {
            let label = format!("Paused — {}", artist.as_deref().unwrap_or(""));
            serde_json::json!({
                "details": sanitize_field(title, MAX_FIELD_BYTES),
                "state": sanitize_field(&label, MAX_FIELD_BYTES),
            })
        }
    }
}

fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Builds the full `SET_ACTIVITY` IPC command for the given state.
/// `nonce` only needs to be unique per outgoing request for this
/// process's lifetime, per the protocol — a monotonic counter is
/// simpler than pulling in a UUID dependency for it.
fn set_activity_frame(state: &PresenceState) -> serde_json::Value {
    let nonce = NONCE
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .to_string();
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity_value(state, now_unix()),
        },
        "nonce": nonce,
    })
}
```

- [ ] **Step 4: Run to verify they pass**

Run: `cargo test --lib platform::discord`
Expected: 10 tests pass.

- [ ] **Step 5: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/platform/discord.rs`.

---

### Task 4: Handle, background task, public API

**Files:**
- Modify: `src-tauri/src/platform/discord.rs` (append)

**Interfaces:**
- Consumes: `connect`, `write_frame`, `OP_FRAME` (Task 2); `PresenceState`, `set_activity_frame` (Task 3).
- Produces: `pub struct Handle`, `pub fn spawn() -> Handle`, `pub(crate) fn notify(handle: &Handle, presence: PresenceState)`, `pub(crate) async fn notify_from_state(handle: &Handle, state: &crate::state::AppState)`, `pub fn set_enabled(handle: &Handle, enabled: bool)`, `pub fn clear(handle: &Handle)` — all consumed by Task 6 (AppState/lib.rs wiring) and Task 7 (broadcast consolidation).

This task has no new automated test: it's the stateful connection/reconnect loop, which needs a live Discord process to exercise meaningfully — same precedent as `platform/mpris.rs`'s D-Bus-facing code and `platform/tray.rs`'s `ksni::Tray` impl, neither of which is unit-tested. It's covered by the manual verification checklist in Task 10.

- [ ] **Step 1: Implement**

Add near the top of `src-tauri/src/platform/discord.rs`, alongside the existing `use` lines:

```rust
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::watch;
```

Add above the `#[cfg(test)]` block:

```rust
const RECONNECT_INTERVAL: Duration = Duration::from_secs(25);
/// Rapid successive calls to `notify` (e.g. clicking Next several times
/// fast) coalesce into one outgoing update instead of one per click —
/// `watch` already discards intermediate values, this just delays the
/// read so a whole burst lands in the same read.
const DEBOUNCE: Duration = Duration::from_millis(1000);

pub struct Handle {
    tx: watch::Sender<PresenceState>,
    enabled: Arc<AtomicBool>,
}

/// Spawns the background connection/reconnect task and returns a handle
/// to it. Safe to call unconditionally at startup regardless of whether
/// Discord is installed or the feature is enabled -- the task itself
/// checks `enabled` before doing any IO, and idles (re-checking every
/// `RECONNECT_INTERVAL`) whenever Discord isn't reachable.
pub fn spawn() -> Handle {
    let (tx, rx) = watch::channel(PresenceState::Idle);
    let enabled = Arc::new(AtomicBool::new(false));
    tauri::async_runtime::spawn(run(enabled.clone(), rx));
    Handle { tx, enabled }
}

/// Pushes a presence value toward the background task. Never blocks;
/// `watch::Sender::send` only fails if every receiver was dropped, which
/// only happens if the background task itself panicked.
pub(crate) fn notify(handle: &Handle, presence: PresenceState) {
    let _ = handle.tx.send(presence);
}

/// Reads the same queue/player state `mpris::notify` reads and maps it
/// to a `PresenceState`, then pushes it -- the bridge from `AppState` to
/// Discord.
pub(crate) async fn notify_from_state(handle: &Handle, state: &crate::state::AppState) {
    let queue = state.queue.lock().unwrap().view();
    let Some(track) = queue.current else {
        notify(handle, PresenceState::Idle);
        return;
    };
    let mut player = state.player.lock().await;
    let paused = player.is_paused().await.ok().flatten().unwrap_or(false);
    let position = player.position_seconds().await.ok().flatten().unwrap_or(0.0);
    drop(player);

    let presence = if paused {
        PresenceState::Paused {
            title: track.title,
            artist: track.artist,
        }
    } else {
        PresenceState::Playing {
            title: track.title,
            artist: track.artist,
            position_secs: position,
        }
    };
    notify(handle, presence);
}

/// Flips the feature on/off and wakes the background task immediately
/// (rather than waiting up to `RECONNECT_INTERVAL`), so a Settings
/// toggle takes effect right away -- `watch::Sender::send` always
/// notifies waiting receivers, even when resending the same value.
pub fn set_enabled(handle: &Handle, enabled: bool) {
    handle.enabled.store(enabled, std::sync::atomic::Ordering::Relaxed);
    let current = handle.tx.borrow().clone();
    let _ = handle.tx.send(current);
}

/// Best-effort immediate clear, for app shutdown. A plain channel send —
/// never blocking IO — so it can't hang the quit path even if Discord's
/// socket is stuck.
pub fn clear(handle: &Handle) {
    let _ = handle.tx.send(PresenceState::Idle);
}

async fn run(enabled: Arc<AtomicBool>, mut rx: watch::Receiver<PresenceState>) {
    let mut stream: Option<UnixStream> = None;
    let mut retry = tokio::time::interval(RECONNECT_INTERVAL);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            changed = rx.changed() => {
                if changed.is_err() {
                    return; // Handle dropped -- app is shutting down.
                }
                tokio::time::sleep(DEBOUNCE).await;
            }
            _ = retry.tick() => {}
        }

        if !enabled.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(mut s) = stream.take() {
                let _ = write_frame(&mut s, OP_FRAME, &set_activity_frame(&PresenceState::Idle)).await;
            }
            continue;
        }

        if stream.is_none() {
            stream = connect().await;
        }
        let Some(s) = stream.as_mut() else { continue };

        let presence = rx.borrow().clone();
        if write_frame(s, OP_FRAME, &set_activity_frame(&presence)).await.is_err() {
            stream = None;
        }
    }
}
```

- [ ] **Step 2: Verify it compiles and existing tests still pass**

Run: `cargo test --lib platform::discord`
Expected: still 10 tests pass, no new failures. Run `cargo clippy --all-targets -- -D warnings` and remove any `#[allow(dead_code)]` markers added in Task 2 (everything is now actually called from `run`/`spawn`).

- [ ] **Step 3: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/platform/discord.rs`.

---

### Task 5: Settings — `discord_presence_enabled`

**Files:**
- Modify: `src-tauri/src/models.rs:147-169` (`Settings` struct and its `Default` impl)
- Modify: `src-tauri/src/commands/settings.rs:26-63` (`SettingsPatch`, `apply_patch`)

**Interfaces:**
- Consumes: nothing new.
- Produces: `Settings.discord_presence_enabled: bool` and `SettingsPatch.discord_presence_enabled: Option<bool>`, both consumed by Task 6 (lib.rs startup read, `update_settings` side effect) and Task 8 (frontend `Settings` type).

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `src-tauri/src/commands/settings.rs`:

```rust
#[test]
fn patch_can_enable_discord_presence() {
    let next = apply_patch(
        Settings::default(),
        SettingsPatch {
            discord_presence_enabled: Some(true),
            ..Default::default()
        },
    );
    assert!(next.discord_presence_enabled);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib commands::settings`
Expected: compile error — `SettingsPatch` has no field `discord_presence_enabled`.

- [ ] **Step 3: Add the field**

In `src-tauri/src/models.rs`, change the `Settings` struct:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub cache_limit_mb: u32,
    pub history_enabled: bool,
    pub crash_report_enabled: bool,
    pub autostart_enabled: bool,
    pub sponsorblock_categories: Vec<String>,
    pub volume: u8,
    pub discord_presence_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            cache_limit_mb: 500,
            history_enabled: true,
            crash_report_enabled: false,
            autostart_enabled: false,
            sponsorblock_categories: vec!["sponsor".into(), "selfpromo".into(), "intro".into()],
            volume: 100,
            discord_presence_enabled: false,
        }
    }
}
```

In `src-tauri/src/commands/settings.rs`, change `SettingsPatch`:

```rust
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub history_enabled: Option<bool>,
    pub crash_report_enabled: Option<bool>,
    pub autostart_enabled: Option<bool>,
    pub sponsorblock_categories: Option<Vec<String>>,
    pub volume: Option<u8>,
    pub discord_presence_enabled: Option<bool>,
}
```

And in `apply_patch`, add alongside the other `if let Some(v) = patch...` lines:

```rust
    if let Some(v) = patch.discord_presence_enabled {
        next.discord_presence_enabled = v;
    }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test --lib` (from `src-tauri/`)
Expected: all tests pass, including the new one.

- [ ] **Step 5: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/models.rs` and `src-tauri/src/commands/settings.rs`.

---

### Task 6: Wire the Handle — `AppState`, startup, settings side effect

**Files:**
- Modify: `src-tauri/src/state.rs:20-46` (`AppState`)
- Modify: `src-tauri/src/lib.rs:89-112` (setup: spawn, `app.manage`)
- Modify: `src-tauri/src/commands/settings.rs:75-92` (`update_settings`)
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/playback.rs`, `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs` (each file's `test_state()` helper)

**Interfaces:**
- Consumes: `platform::discord::Handle`, `spawn`, `set_enabled` (Task 4); `Settings.discord_presence_enabled` (Task 5).
- Produces: `AppState.discord: Option<crate::platform::discord::Handle>`, consumed by Task 7.

- [ ] **Step 1: Add the field to `AppState`**

In `src-tauri/src/state.rs`, change:

```rust
    /// `None` when the D-Bus session bus wasn't reachable at startup — MPRIS
    /// is a nice-to-have desktop integration, never a startup requirement.
    pub mpris: Option<mpris::Handle>,
    /// Skip-segment data for whatever track is currently playing, kept only
```

to:

```rust
    /// `None` when the D-Bus session bus wasn't reachable at startup — MPRIS
    /// is a nice-to-have desktop integration, never a startup requirement.
    pub mpris: Option<mpris::Handle>,
    /// Always `Some` outside tests (`platform::discord::spawn()` never
    /// fails) — kept optional so test fixtures can pass `None` instead of
    /// spawning a real background task, matching `mpris` above.
    pub discord: Option<crate::platform::discord::Handle>,
    /// Skip-segment data for whatever track is currently playing, kept only
```

- [ ] **Step 2: Wire startup in `lib.rs`**

In `src-tauri/src/lib.rs`, change:

```rust
            let mpris =
                tauri::async_runtime::block_on(platform::mpris::build(app.handle().clone()));

            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                prefetch: media::prefetch::Prefetch::new(),
                player: tokio::sync::Mutex::new(player),
                mpris,
                sponsorblock_segments: Mutex::new(Vec::new()),
                app_dir: app_dir.clone(),
                crash_reporting_enabled: crash_reporting_enabled.clone(),
            });
```

to:

```rust
            let mpris =
                tauri::async_runtime::block_on(platform::mpris::build(app.handle().clone()));
            let discord = platform::discord::spawn();
            platform::discord::set_enabled(&discord, initial_settings.discord_presence_enabled);

            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                prefetch: media::prefetch::Prefetch::new(),
                player: tokio::sync::Mutex::new(player),
                mpris,
                discord: Some(discord),
                sponsorblock_segments: Mutex::new(Vec::new()),
                app_dir: app_dir.clone(),
                crash_reporting_enabled: crash_reporting_enabled.clone(),
            });
```

- [ ] **Step 3: Wire the settings side effect**

In `src-tauri/src/commands/settings.rs`, change `update_settings`'s body from:

```rust
    state.crash_reporting_enabled.store(
        next.crash_report_enabled,
        std::sync::atomic::Ordering::Relaxed,
    );
    autostart::sync(&app, next.autostart_enabled)?;
    Ok(next)
```

to:

```rust
    state.crash_reporting_enabled.store(
        next.crash_report_enabled,
        std::sync::atomic::Ordering::Relaxed,
    );
    autostart::sync(&app, next.autostart_enabled)?;
    if let Some(handle) = state.discord.as_ref() {
        crate::platform::discord::set_enabled(handle, next.discord_presence_enabled);
    }
    Ok(next)
```

- [ ] **Step 4: Update the four test fixtures**

In each of `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/playback.rs`, `src-tauri/src/commands/session.rs`, and `src-tauri/src/commands/queue.rs`, inside `fn test_state() -> AppState`, change:

```rust
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
```

to:

```rust
            mpris: None,
            discord: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
```

- [ ] **Step 5: Run to verify everything still compiles and passes**

Run (from `src-tauri/`): `cargo test`
Expected: all existing tests still pass (this task adds no new test — it's pure wiring, and `AppState.discord` is `None` in every test fixture, matching `mpris`, so no test's behavior changes).

- [ ] **Step 6: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/state.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands/settings.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/playback.rs`, `src-tauri/src/commands/session.rs`, and `src-tauri/src/commands/queue.rs`.

---

### Task 7: Consolidate the playback-change broadcast

**Files:**
- Modify: `src-tauri/src/platform/mod.rs`
- Modify: `src-tauri/src/platform/mpris.rs:262-343` (5 call sites)
- Modify: `src-tauri/src/commands/mod.rs:75,103` (2 call sites)
- Modify: `src-tauri/src/commands/playback.rs:9,16,23` (3 call sites)
- Modify: `src-tauri/src/commands/session.rs:30-41` (`end_session_impl`, new call)
- Modify: `src-tauri/src/lib.rs:179-192` (shutdown, new call)

**Interfaces:**
- Consumes: `mpris::notify` (existing), `discord::notify_from_state`, `discord::clear` (Task 4), `AppState.discord` (Task 6).
- Produces: `pub async fn notify_playback_changed(state: &AppState)` in `platform::mod` — the function every playback-changing command now calls instead of `mpris::notify` directly.

This is the task the design spec calls out as fixing two pre-existing gaps as a byproduct: `end_session_impl` and app shutdown never called `mpris::notify` either, so MPRIS's own D-Bus state (and the frontend's `playback-changed` event) was stale in both cases before this change too.

- [ ] **Step 1: Add the consolidated broadcaster**

In `src-tauri/src/platform/mod.rs`, add below the existing `pub mod` lines:

```rust
use crate::state::AppState;

/// Fans a playback/queue change out to every desktop integration that
/// needs to know about it -- the single point every playback-changing
/// command calls, instead of reaching into `mpris`/`discord`
/// individually. See docs/superpowers/specs/
/// 2026-09-09-discord-rich-presence-design.md.
pub async fn notify_playback_changed(state: &AppState) {
    mpris::notify(state).await;
    if let Some(handle) = state.discord.as_ref() {
        discord::notify_from_state(handle, state).await;
    }
}
```

- [ ] **Step 2: Replace the MPRIS-internal call sites**

In `src-tauri/src/platform/mpris.rs`, there are 5 identical occurrences of `notify(&state).await;` (inside the `pause`, `stop`, `play`, `seek`, and `set_position` trait methods). Replace all 5 with `crate::platform::notify_playback_changed(&state).await;`.

- [ ] **Step 3: Replace the `commands/mod.rs` call sites**

In `src-tauri/src/commands/mod.rs`, there are 2 identical occurrences of `crate::platform::mpris::notify(state).await;` (inside `resolve_and_load` and `toggle_play_pause`). Replace both with `crate::platform::notify_playback_changed(state).await;`.

- [ ] **Step 4: Replace the `commands/playback.rs` call sites**

In `src-tauri/src/commands/playback.rs`, there are 3 identical occurrences of `crate::platform::mpris::notify(&state).await;` (inside `pause_playback`, `resume_playback`, `seek_playback`). Replace all 3 with `crate::platform::notify_playback_changed(&state).await;`.

- [ ] **Step 5: Add the missing call in `end_session_impl`**

In `src-tauri/src/commands/session.rs`, change:

```rust
pub(crate) async fn end_session_impl(state: &AppState) -> Result<()> {
    let current = state.db.lock().unwrap().current_session()?;
    let session = current.ok_or(EchoraError::NoActiveSession)?;
    // Before the session (and the completion this would attribute to it)
    // stops existing -- same "record before it stops being current" rule
    // as every queue-command path (see
    // `commands::record_current_completion`'s doc comment).
    super::record_current_completion(state).await?;
    state.db.lock().unwrap().end_session(session.id)?;
    state.queue.lock().unwrap().clear();
    Ok(())
}
```

to:

```rust
pub(crate) async fn end_session_impl(state: &AppState) -> Result<()> {
    let current = state.db.lock().unwrap().current_session()?;
    let session = current.ok_or(EchoraError::NoActiveSession)?;
    // Before the session (and the completion this would attribute to it)
    // stops existing -- same "record before it stops being current" rule
    // as every queue-command path (see
    // `commands::record_current_completion`'s doc comment).
    super::record_current_completion(state).await?;
    state.db.lock().unwrap().end_session(session.id)?;
    state.queue.lock().unwrap().clear();
    crate::platform::notify_playback_changed(state).await;
    Ok(())
}
```

- [ ] **Step 6: Add the shutdown clear**

In `src-tauri/src/lib.rs`, change:

```rust
                tauri::async_runtime::block_on(async {
                    // Same "record before it stops being current" rule as
                    // every other way the current track changes (see
                    // `commands::record_current_completion`'s own doc
                    // comment) -- quitting mid-track is the one case that
                    // isn't a queue command, so it has to be recorded here
                    // instead, before the position it reads is gone.
                    let _ = commands::record_current_completion(&state).await;
                    let _ = state.player.lock().await.shutdown().await;
                });
```

to:

```rust
                tauri::async_runtime::block_on(async {
                    // Same "record before it stops being current" rule as
                    // every other way the current track changes (see
                    // `commands::record_current_completion`'s own doc
                    // comment) -- quitting mid-track is the one case that
                    // isn't a queue command, so it has to be recorded here
                    // instead, before the position it reads is gone.
                    let _ = commands::record_current_completion(&state).await;
                    if let Some(handle) = state.discord.as_ref() {
                        platform::discord::clear(handle);
                    }
                    let _ = state.player.lock().await.shutdown().await;
                });
```

Note: this is a synchronous channel send, not a blocking IO call — it can't hang app quit even if Discord's socket is stuck (see the design spec's Error handling section). If the process exits before the background task gets to actually write the clear frame, Discord's own disconnect detection (it clears a client's activity when its IPC socket closes) covers the same outcome as a fallback.

- [ ] **Step 7: Run to verify everything compiles and passes**

Run (from `src-tauri/`): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all pass. No behavior changes for any existing test — `state.discord` is `None` in every test fixture, so `notify_playback_changed`'s `if let Some(handle) = ...` branch is always skipped there, same as how `mpris::notify` already degrades with `state.mpris: None` in tests today.

- [ ] **Step 8: Commit**

Invoke the `auto-commit` skill to commit `src-tauri/src/platform/mod.rs`, `src-tauri/src/platform/mpris.rs`, `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/playback.rs`, `src-tauri/src/commands/session.rs`, and `src-tauri/src/lib.rs`.

---

### Task 8: Frontend — settings type and toggle

**Files:**
- Modify: `src/lib/api.ts:107-114` (`Settings` interface)
- Modify: `src/components/SettingsView.tsx:360-376` (Privacy section)

**Interfaces:**
- Consumes: `Settings.discord_presence_enabled` (Rust, Task 5) via the existing `api.getSettings`/`api.updateSettings` wire format.
- Produces: nothing consumed by a later task — this is the last code task before documentation.

- [ ] **Step 1: Add the field to the TypeScript `Settings` type**

In `src/lib/api.ts`, change:

```typescript
export interface Settings {
  cache_limit_mb: number;
  history_enabled: boolean;
  crash_report_enabled: boolean;
  autostart_enabled: boolean;
  sponsorblock_categories: string[];
  volume: number;
}
```

to:

```typescript
export interface Settings {
  cache_limit_mb: number;
  history_enabled: boolean;
  crash_report_enabled: boolean;
  autostart_enabled: boolean;
  sponsorblock_categories: string[];
  volume: number;
  discord_presence_enabled: boolean;
}
```

- [ ] **Step 2: Add the toggle to the Privacy section**

In `src/components/SettingsView.tsx`, change:

```tsx
        <Toggle
          on={settings.crash_report_enabled}
          label="Crash reports"
          onChange={() => update({ crash_report_enabled: !settings.crash_report_enabled })}
        />
        </div>
        <CrashReportsList enabled={settings.crash_report_enabled} onError={onError} />
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>
```

to:

```tsx
        <Toggle
          on={settings.crash_report_enabled}
          label="Crash reports"
          onChange={() => update({ crash_report_enabled: !settings.crash_report_enabled })}
        />
        </div>
        <CrashReportsList enabled={settings.crash_report_enabled} onError={onError} />

        <div className="settings-row">
          <span>
            <div className="settings-row__label">Discord Rich Presence</div>
            <div className="settings-row__hint">
              Shows what's playing as your Discord status — only sent while Discord is running
            </div>
          </span>
          <Toggle
            on={settings.discord_presence_enabled}
            label="Discord Rich Presence"
            onChange={() =>
              update({ discord_presence_enabled: !settings.discord_presence_enabled })
            }
          />
        </div>
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>
```

(Match the existing indentation in the file exactly — the snippet above is illustrative of content and order, not literal whitespace.)

- [ ] **Step 3: Run to verify**

Run (from the repo root): `npm run lint && npm run build`
Expected: both pass with no new errors.

- [ ] **Step 4: Commit**

Invoke the `auto-commit` skill to commit `src/lib/api.ts` and `src/components/SettingsView.tsx`.

---

### Task 9: Documentation — ADR and Requirements Freeze

**Files:**
- Create: `docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md`
- Modify: `docs/REQUIREMENTS_FREEZE.md` (V1 scope "In:" list, Telemetry note)

**Interfaces:** none — pure documentation, no code dependency.

- [ ] **Step 1: Write the ADR**

Create `docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md`:

```markdown
# ADR 0010: Discord Rich Presence via a hand-rolled IPC client, no crate

## Status
Accepted

## Context
The feature request originated from a misunderstanding worth recording:
Discord's own "Compartilhar minha atividade" (share my activity) privacy
toggle only controls whether Discord *shows* activity to friends — it
does nothing on its own. An app has to actively speak Discord's local
Rich Presence IPC protocol for anything to appear at all.

That protocol is a Unix domain socket (`$XDG_RUNTIME_DIR/discord-ipc-0`,
falling back to `/tmp`) carrying an 8-byte header (opcode + little-endian
`u32` length) followed by a JSON payload — a handshake frame once per
connection, then `SET_ACTIVITY` frames to update or clear the status.
Ready-made crates exist for this (e.g. `discord-rich-presence`).

## Decision
Implement the protocol directly in `src-tauri/src/platform/discord.rs`,
using only dependencies Echora already has (`tokio` with its `net`/
`io-util`/`time` features, already enabled; `serde_json`). No new Cargo
dependency was added.

This follows the same reasoning as ADR 0005 (prefer the option with
fewer/no new dependencies where the protocol involved is small and
stable) and CLAUDE.md's "don't add a dependency just in case" rule:
Discord's Rich Presence framing has been stable for years and amounts to
roughly 150-250 lines of code, well within what hand-rolling costs
against pulling in a new crate, a licensing review, and extra surface in
the dependency tree.

## Consequences
- No new entry needed in `THIRD_PARTY_NOTICES.md`, no
  `licensing-compliance-reviewer` pass required for this feature.
- Echora owns protocol correctness. If Discord ever changes the IPC
  framing (no indication it plans to — this has been stable for years),
  `platform/discord.rs` needs a matching update; a crate would have
  absorbed that instead.
- The feature is opt-in (`Settings.discord_presence_enabled`, default
  `false`), consistent with "no telemetry/cloud by default" — sending
  now-playing data to a third-party client like Discord only happens
  when the user turns it on.
```

- [ ] **Step 2: Update `docs/REQUIREMENTS_FREEZE.md`**

In the `## V1 scope` section, change:

```markdown
**In:** auto-update, Scenes, Mood Mixing, Discover, Statistics,
SponsorBlock, autostart on system startup.
```

to:

```markdown
**In:** auto-update, Scenes, Mood Mixing, Discover, Statistics,
SponsorBlock, autostart on system startup, Discord Rich Presence
(opt-in, off by default).
```

In the `## Product behavior` section, change the Telemetry bullet from:

```markdown
- Telemetry: none by default. The one exception is a fully manual,
  opt-in crash report — a local log plus a button that opens a
  pre-filled GitHub issue in the user's browser. No automatic network
  call, no third-party SDK.
```

to:

```markdown
- Telemetry: none by default. Two manual, opt-in exceptions exist: a
  crash report (a local log plus a button that opens a pre-filled
  GitHub issue in the user's browser — no automatic network call, no
  third-party SDK), and Discord Rich Presence (see
  `docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md`) — off by
  default, and even when on, no data leaves the device beyond the local
  Discord client's own IPC socket.
```

- [ ] **Step 3: Commit**

Invoke the `auto-commit` skill to commit `docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md` and `docs/REQUIREMENTS_FREEZE.md`.

---

### Task 10: Final verification gate and manual checklist

**Files:** none (verification only).

**Interfaces:** none.

- [ ] **Step 1: Run the full backend gate**

From `src-tauri/`:

```bash
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Expected: all pass, zero clippy warnings.

- [ ] **Step 2: Run the full frontend gate**

From the repo root:

```bash
npm run lint && npm run build
```

Expected: both pass.

- [ ] **Step 3: Manual verification checklist**

None of these are automatable in CI (no Discord process available there) — this matches the project's existing precedent of manual verification for MPRIS/tray D-Bus-facing behavior. Perform each with a real Discord desktop client and report which were actually checked, per CLAUDE.md's "Before claiming something works" rule:

- [ ] Enable the toggle in Settings with Discord already open and a track playing — status appears within ~25s (`RECONNECT_INTERVAL`) showing the correct title/artist and a running elapsed timer.
- [ ] With the toggle enabled and Discord closed, open Discord — status appears within one `RECONNECT_INTERVAL` tick without restarting Echora.
- [ ] Pause the track — status changes to show "Paused — {artist}" with no running timer.
- [ ] Resume — status returns to the running timer, restarted from the correct elapsed position.
- [ ] Skip through several tracks rapidly (Next several times fast) — Discord shows only the final track, not a flood of intermediate updates.
- [ ] End the session (or start a new one) — Discord's status clears.
- [ ] Quit Echora while a track is playing — Discord's status clears (either via the explicit clear call or Discord's own disconnect detection).
- [ ] Disable the toggle while a track is playing — status clears and does not come back until re-enabled.

- [ ] **Step 4: Commit**

If any fixes came out of manual verification, invoke the `auto-commit` skill for them. If nothing needs fixing, there is nothing to commit for this task — report the checklist results.
