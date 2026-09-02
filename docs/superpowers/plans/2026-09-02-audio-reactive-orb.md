# Audio-reactive orb Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the player screen's orb react to the real audio level of the currently playing track, and stop consuming CPU/IPC entirely when the window is minimized or unfocused.

**Architecture:** mpv's `astats` audio filter is attached once per mpv process (via IPC `af add`), exposing a continuously-updating RMS level through `get_property af-metadata/echora_level`. A Rust background task (`media::audio_level::watch`, mirroring the existing `media::auto_advance::watch` shape) polls this at ~12.5Hz, but only while a track is actively playing *and* the window is visible, not minimized, and focused — gated by a pure, unit-tested predicate. Each successful reading is normalized to `0.0..=1.0` and emitted as a Tauri event (`audio-level`); the frontend writes it straight into a CSS custom property via a `ref` (no React state, no re-renders), which modulates the orb's existing `echora-pulse` CSS animation. No live data ever arriving (filter failed to attach, track hasn't buffered yet, or the gate is closed) means the orb just keeps running its current static pulse — there's no code path that requires this data to exist.

**Tech Stack:** Rust (Tokio, `tauri`, `serde_json`), TypeScript/React (`@tauri-apps/api/event`), CSS custom properties.

**Spec:** `docs/superpowers/specs/2026-09-02-audio-reactive-orb-design.md`

## Global Constraints

- Rust is the source of truth for all playback/audio state — the frontend never invents or infers audio-level data itself (project rule, CLAUDE.md).
- No `observe_property` for this — confirmed in the spec's research that mpv's `observe_property` is documented as unsuitable for high-frequency properties (mpv issue #5661); this plan polls via `get_property` only.
- The `astats` filter is attached **once** per mpv process lifetime via a `level_metering_ready` flag on `Player`, not per track — verified against a real running mpv (see Task 1) that `af add` on a fresh filter chain succeeds and its metadata updates continuously across the filter's lifetime; there was no need to and this plan does not re-add it per track.
- Verified against a real mpv 0.37.0 process (this repo's target version) that `get_property af-metadata/<label>` returns `data` as a flat object keyed by strings like `"lavfi.astats.Overall.RMS_level"`, with **string-typed** values (e.g. `"-21.123621"`), not numbers — parsing code in this plan reflects that, not a guess.
- `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (from `src-tauri/`) must pass after every Rust task. `npm run lint && npm run build` (from repo root) must pass after the frontend task.
- Do not run `git commit` directly — this repo's commit steps below use the message content only as a description of intent; the actual commit for each task must go through this repo's `auto-commit` skill/flow, not a raw `git commit -m` invocation with an AI-attribution footer.

---

### Task 1: `Player` audio-level API

**Files:**
- Modify: `src-tauri/src/media/player.rs`

**Interfaces:**
- Produces: `Player::enable_level_metering(&mut self) -> Result<()>` (idempotent), `Player::audio_level_db(&mut self) -> Result<Option<f64>>`, new private field `level_metering_ready: bool` on `Player`.

- [ ] **Step 1: Add the `level_metering_ready` field**

In `Player`'s struct definition (around line 20-25) and its `new()` constructor (around line 28-39), add:

```rust
pub struct Player {
    socket_path: PathBuf,
    child: Option<CommandChild>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
    /// Whether the `astats` metering filter has been successfully attached
    /// for this mpv process yet — attached once, lazily, not per track (see
    /// `media::audio_level::watch`), so later calls to
    /// `enable_level_metering` are a cheap no-op instead of re-adding the
    /// filter every track change.
    level_metering_ready: bool,
}

impl Player {
    pub fn new(
        socket_path: PathBuf,
        app_dir: PathBuf,
        crash_reporting_enabled: Arc<AtomicBool>,
    ) -> Self {
        Player {
            socket_path,
            child: None,
            app_dir,
            crash_reporting_enabled,
            level_metering_ready: false,
        }
    }
```

- [ ] **Step 2: Write the failing tests**

Add to `player.rs`'s existing `#[cfg(test)] mod tests` block (it already has a `test_app_handle()` helper — reuse it):

```rust
    #[tokio::test]
    async fn enable_level_metering_is_a_noop_the_second_time() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();

        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        // Second call must not error even though the filter is already
        // attached — this is what makes it safe to call unconditionally
        // from audio_level::watch every tick before reading a level.
        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn audio_level_db_returns_none_before_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-none-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();

        assert_eq!(player.audio_level_db().await.unwrap(), None);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn audio_level_db_reads_a_real_rms_level_once_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-real-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();

        // Give mpv a moment to actually start decoding/filtering audio —
        // mirrors the existing smoke tests' pattern of a short sleep after
        // a state-changing IPC call before reading it back.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let level = player.audio_level_db().await.unwrap();
        assert!(level.is_some());
        assert!(level.unwrap().is_finite());

        player.shutdown().await.unwrap();
    }
```

These three tests need real mpv (they spawn the actual sidecar and play a synthetic tone), so mark them `#[ignore]` the same way this file's existing real-mpv smoke tests already are (confirmed: this file uses a bare `#[ignore]`, no reason string) — add a plain `#[ignore]` line directly above each `#[tokio::test]` line just added.

- [ ] **Step 3: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test enable_level_metering -- --ignored` and `cargo test audio_level_db -- --ignored`
Expected: FAIL to compile — `enable_level_metering` and `audio_level_db` don't exist yet.

- [ ] **Step 4: Implement `enable_level_metering` and `audio_level_db`**

Add these methods to `impl Player`, near the other IPC-command methods (after `duration_seconds`, before `shutdown`, around line 165):

```rust
    /// Attaches the RMS-level metering filter, once per mpv process. Later
    /// calls are a no-op — safe to call unconditionally from every poll
    /// tick in `media::audio_level::watch` rather than tracking "have I
    /// called this yet" at the call site too.
    ///
    /// Verified against a real mpv 0.37.0 process: `af add` with this
    /// exact filter spec succeeds once a file is loaded, and its metadata
    /// (read via `audio_level_db`) updates continuously afterward without
    /// needing to be re-added.
    pub async fn enable_level_metering(&mut self) -> Result<()> {
        if self.level_metering_ready {
            return Ok(());
        }
        let reply = self
            .send_command(json!([
                "af",
                "add",
                "@echora_level:lavfi=[astats=metadata=1:reset=1]"
            ]))
            .await?;
        if reply.get("error").and_then(Value::as_str) == Some("success") {
            self.level_metering_ready = true;
        }
        Ok(())
    }

    /// Current RMS level in dBFS from the `astats` filter `enable_level_metering`
    /// attaches, or `None` if metering isn't enabled yet, the filter hasn't
    /// produced a reading yet, or the reading was non-finite (e.g. true
    /// digital silence reads as `-inf`, which this treats the same as "no
    /// data" rather than propagating an infinite value to the frontend).
    ///
    /// mpv's `af-metadata/<label>` property replies with `data` as a flat
    /// object keyed by strings like `"lavfi.astats.Overall.RMS_level"`,
    /// with **string-typed** values (e.g. `"-21.123621"`) — confirmed
    /// against a real mpv process, not assumed from FFmpeg's docs alone.
    pub async fn audio_level_db(&mut self) -> Result<Option<f64>> {
        if !self.level_metering_ready {
            return Ok(None);
        }
        let reply = self
            .send_command(json!(["get_property", "af-metadata/echora_level"]))
            .await?;
        Ok(reply
            .get("data")
            .and_then(|d| d.get("lavfi.astats.Overall.RMS_level"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite()))
    }
```

- [ ] **Step 5: Run the tests to verify they pass**

Run (from `src-tauri/`): `cargo test enable_level_metering -- --ignored --nocapture` and `cargo test audio_level_db -- --ignored --nocapture`
Expected: PASS (3 tests). This actually spawns mpv, so it needs `mpv` on `PATH` or the sidecar binary available — same requirement the file's existing ignored smoke tests already have.

- [ ] **Step 6: Run the full non-ignored suite**

Run (from `src-tauri/`): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: everything passes; the 3 new tests show up as `ignored`, same as the file's existing real-mpv tests.

- [ ] **Step 7: Commit**

Stage `src-tauri/src/media/player.rs` and hand off to this repo's commit flow (not a raw `git commit`) with intent: "feat(player): add audio-level metering via mpv astats".

---

### Task 2: `media::audio_level` background watcher

**Files:**
- Create: `src-tauri/src/media/audio_level.rs`
- Modify: `src-tauri/src/media/mod.rs` (add `pub mod audio_level;`)
- Modify: `src-tauri/src/lib.rs` (spawn the watcher in `.setup()`)

**Interfaces:**
- Consumes: `Player::enable_level_metering`/`Player::audio_level_db` (Task 1), `AppState.queue: Mutex<Queue>` and `AppState.player: tokio::sync::Mutex<Player>` (existing), `Player::is_paused(&mut self) -> Result<Option<bool>>` (existing).
- Produces: `pub async fn watch(app: AppHandle)`, emits Tauri event `"audio-level"` with an `f32` payload in `0.0..=1.0`.

- [ ] **Step 1: Write the failing tests for the pure logic**

Create `src-tauri/src/media/audio_level.rs` with just the pure functions and their tests first (no `watch()` yet):

```rust
//! Polls mpv's audio level (via `Player::audio_level_db`, see
//! `media::player`) and emits it to the frontend so the player screen's
//! orb can react to it — but only while there's actually something to
//! show: a track playing, with the window visible and focused. Mirrors
//! `media::auto_advance::watch`'s shape (a `tokio::time::interval` loop
//! spawned once at startup) and its pattern of factoring the per-tick
//! decision into a pure, directly-testable function.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

const POLL_INTERVAL: Duration = Duration::from_millis(80); // ~12.5Hz

/// Below this, a reading is treated as silence (maps to `0.0`); at or
/// above `0.0` dBFS maps to `1.0`. -60dBFS is a common "quiet but not
/// silent" floor for loudness UIs — not derived from any project
/// requirement, a reasonable default to tune later if it looks wrong on
/// real tracks.
const NOISE_FLOOR_DB: f64 = -60.0;

/// Whether the watch loop should be attempting to read a level *this
/// tick* — pulled out of `watch()` so it's testable without mpv or a real
/// Tauri window.
fn should_meter(is_playing: bool, is_visible: bool, is_minimized: bool, is_focused: bool) -> bool {
    is_playing && is_visible && !is_minimized && is_focused
}

/// Maps an RMS dBFS reading (or `None`, meaning no data this tick) to the
/// `0.0..=1.0` range the frontend's CSS custom property expects.
fn normalize_level(db: Option<f64>) -> f32 {
    let Some(db) = db else { return 0.0 };
    let clamped = db.clamp(NOISE_FLOOR_DB, 0.0);
    ((clamped - NOISE_FLOOR_DB) / (0.0 - NOISE_FLOOR_DB)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters_only_when_playing_visible_unminimized_and_focused() {
        assert!(should_meter(true, true, false, true));
    }

    #[test]
    fn does_not_meter_when_paused() {
        assert!(!should_meter(false, true, false, true));
    }

    #[test]
    fn does_not_meter_when_minimized() {
        assert!(!should_meter(true, true, true, true));
    }

    #[test]
    fn does_not_meter_when_unfocused() {
        assert!(!should_meter(true, true, false, false));
    }

    #[test]
    fn does_not_meter_when_hidden_to_tray() {
        // "hidden" (REQUIREMENTS_FREEZE: closing minimizes to tray, see
        // platform::tray) reads as is_visible == false, distinct from the
        // OS-level "minimized" state.
        assert!(!should_meter(true, false, false, true));
    }

    #[test]
    fn no_reading_normalizes_to_zero() {
        assert_eq!(normalize_level(None), 0.0);
    }

    #[test]
    fn silence_floor_normalizes_to_zero() {
        assert_eq!(normalize_level(Some(-60.0)), 0.0);
    }

    #[test]
    fn quieter_than_the_floor_still_clamps_to_zero() {
        assert_eq!(normalize_level(Some(-120.0)), 0.0);
    }

    #[test]
    fn full_scale_normalizes_to_one() {
        assert_eq!(normalize_level(Some(0.0)), 1.0);
    }

    #[test]
    fn louder_than_full_scale_still_clamps_to_one() {
        assert_eq!(normalize_level(Some(6.0)), 1.0);
    }

    #[test]
    fn midpoint_normalizes_to_one_half() {
        assert_eq!(normalize_level(Some(-30.0)), 0.5);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `src-tauri/`): `cargo test --lib media::audio_level`
Expected: FAIL to compile — `src-tauri/src/media/mod.rs` doesn't declare this module yet.

- [ ] **Step 3: Wire the module in and run again**

In `src-tauri/src/media/mod.rs`, add (alphabetical, matching the existing list's ordering):

```rust
pub mod audio_level;
```

Run: `cargo test --lib media::audio_level`
Expected: PASS (10 tests).

- [ ] **Step 4: Add the `watch()` loop**

Append to `src-tauri/src/media/audio_level.rs`, above the `#[cfg(test)]` block:

```rust
/// Runs forever. Once per tick: reads the current window/playback state,
/// decides via `should_meter` whether to bother, and if so, makes sure
/// metering is enabled and emits the latest normalized level.
pub async fn watch(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);

    loop {
        interval.tick().await;

        let Some(window) = app.get_webview_window("main") else {
            continue;
        };
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);

        let state = app.state::<AppState>();
        let has_current_track = state.queue.lock().unwrap().current().is_some();

        let mut player = state.player.lock().await;
        let is_playing = has_current_track && matches!(player.is_paused().await, Ok(Some(false)));

        if !should_meter(is_playing, is_visible, is_minimized, is_focused) {
            continue;
        }

        let _ = player.enable_level_metering().await;
        let level_db = player.audio_level_db().await.unwrap_or(None);
        drop(player);

        let _ = app.emit("audio-level", normalize_level(level_db));
    }
}
```

`Emitter` (imported in Step 1) is the trait `.emit()` needs — confirmed against `commands/queue.rs`'s own `app.emit(TRACK_AUTO_ADVANCED_EVENT, ())` call (added by the in-flight auto-advance work), which uses the identical `use tauri::{Emitter, ...}` import.

- [ ] **Step 5: Spawn it from `lib.rs`**

In `src-tauri/src/lib.rs`'s `.setup()` closure, right next to the existing `media::auto_advance::watch` spawn line, add:

```rust
            tauri::async_runtime::spawn(media::audio_level::watch(app.handle().clone()));
```

- [ ] **Step 6: Run the full verification gate**

Run (from `src-tauri/`): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: everything passes, including the 10 new pure-logic tests.

- [ ] **Step 7: Commit**

Stage `src-tauri/src/media/audio_level.rs`, `src-tauri/src/media/mod.rs`, `src-tauri/src/lib.rs` and hand off to this repo's commit flow with intent: "feat(player): poll and broadcast audio level when the orb can be seen".

---

### Task 3: frontend event wrapper

**Files:**
- Modify: `src/lib/api.ts`

**Interfaces:**
- Consumes: Tauri event `"audio-level"` (Task 2), `f32` payload.
- Produces: `api.onAudioLevel(callback: (level: number) => void) => UnlistenFn`.

- [ ] **Step 1: Add the wrapper**

`import { listen } from "@tauri-apps/api/event";` is already present at the top of `src/lib/api.ts` (added by the separate, already-landed auto-advance work, for its own `onTrackAutoAdvanced` — check it's still there; if this task runs against a tree from before that landed, add that import line yourself). Next to `onTrackAutoAdvanced` in the `api` object, add:

```typescript
  /** Fires ~12x/sec with the current track's normalized audio level
   * (0..1) while the player screen can actually show it — Rust already
   * gates this off when paused, minimized, or unfocused, so the frontend
   * doesn't need its own visibility checks before subscribing. */
  onAudioLevel: (callback: (level: number) => void) =>
    listen<number>("audio-level", (event) => callback(event.payload)),
```

- [ ] **Step 2: Verify it compiles**

Run (from repo root): `npm run lint && npm run build`
Expected: both pass. There is no unit test framework in this project (vitest isn't installed) — this is intentionally a thin, type-checked wrapper with no logic of its own to unit-test.

- [ ] **Step 3: Commit**

Stage `src/lib/api.ts` and hand off to this repo's commit flow with intent: "feat(api): expose the audio-level event to the frontend".

---

### Task 4: orb reacts

**Files:**
- Modify: `src/components/PlayerView.tsx`
- Modify: `src/styles.css`

**Interfaces:**
- Consumes: `api.onAudioLevel` (Task 3).

- [ ] **Step 1: Read the current orb JSX and add a ref**

In `src/components/PlayerView.tsx`, the orb markup (around line 101-104) is:

```tsx
          <div className="orb-wrap">
            <div className="orb-ring" aria-hidden="true" />
            <div className="orb" aria-hidden="true" />
          </div>
```

Add a `useRef` for the wrapping element so the level can be written straight to its `style` without going through React state (12x/sec state updates would cause unnecessary re-renders — this project treats Lightness/efficiency as priority #1):

```tsx
  const orbWrapRef = useRef<HTMLDivElement | null>(null);
```

Place this near this component's other hook calls at the top of the function body. Update the markup to attach it:

```tsx
          <div className="orb-wrap" ref={orbWrapRef}>
```

This file currently only imports `useState` from `"react"` (confirmed) — change its import line to `import { useEffect, useRef, useState } from "react";` (this task's Step 2 needs `useEffect` too).

- [ ] **Step 2: Subscribe to the event**

Add an effect (near this component's other `useEffect`s):

```tsx
  useEffect(() => {
    let smoothed = 0;
    const unlisten = api.onAudioLevel((level) => {
      // Exponential moving average so the visual doesn't jump frame to
      // frame — 0.25 is a starting point, tune during manual testing.
      smoothed = smoothed + (level - smoothed) * 0.25;
      orbWrapRef.current?.style.setProperty("--orb-level", smoothed.toFixed(3));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [track?.id]);
```

Key this effect on `track?.id` (confirmed: `const track = queue.current;` inside this component, same variable the existing `disabled={!track}` on the Save button already uses) so a track change resets the smoothing state and re-subscribes cleanly.

- [ ] **Step 3: Add the CSS variable to the orb's animation**

In `src/styles.css`, modify `.orb-ring` and `.orb` (around lines 493-505) to layer the live level on top of the existing `echora-pulse` keyframe rather than replacing it:

```css
.orb-ring {
  position: absolute;
  inset: 0;
  border-radius: 999px;
  border: 1px solid oklch(70% 0.14 300 / 0.5);
  animation: echora-pulse 3.6s ease-in-out infinite;
  transform: scale(calc(1 + var(--orb-level, 0) * 0.08));
  transition: transform 80ms linear;
}
.orb {
  position: absolute;
  inset: 26px;
  border-radius: 999px;
  background: radial-gradient(circle at 38% 32%, oklch(72% 0.17 300 / 0.95), oklch(45% 0.14 300 / 0.55) 70%);
  transform: scale(calc(1 + var(--orb-level, 0) * 0.15));
  opacity: calc(0.85 + var(--orb-level, 0) * 0.15);
  transition: transform 80ms linear, opacity 80ms linear;
}
```

`var(--orb-level, 0)` defaults to `0` (no visible change) whenever the custom property is unset — which is exactly the "no live data" fallback case (filter never attached, gated off, or before the first event arrives): the orb keeps doing exactly what it does today, `echora-pulse`'s own keyframe untouched. The `transition` softens the 80ms-interval steps from the poll into something that reads as continuous motion rather than a visible stair-step.

- [ ] **Step 4: Manual verification**

This project has no frontend test framework and no automated way to verify a visual/audio-reactive feature — say so explicitly rather than claiming it "works." Run the app (`npm run tauri dev`, in the user's own terminal, not launched by an agent — see this project's standing note on that), play a track, and confirm: the orb visibly pulses more with louder passages, keeps its baseline pulse during quiet ones, and stops updating (settles back to its static baseline) when the window is minimized or you switch to another app.

- [ ] **Step 5: Run the full verification gate**

Run (from repo root): `npm run lint && npm run build`
Expected: both pass.

- [ ] **Step 6: Commit**

Stage `src/components/PlayerView.tsx` and `src/styles.css` and hand off to this repo's commit flow with intent: "feat(player): make the orb react to the current track's audio level".
