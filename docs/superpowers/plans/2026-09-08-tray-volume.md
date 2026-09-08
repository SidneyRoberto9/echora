# Tray Volume Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the tray a real, continuous volume control (mouse-wheel-over-icon) that stays in sync with the player's main slider, by replacing Echora's Linux tray backend (`tauri::tray`/`muda`) with `ksni` — the only stack that exposes scroll events at all on Linux.

**Architecture:** `ksni::Tray` implementation (`EchoraTray`) replaces `tauri::tray::TrayIconBuilder`. A single existing choke point (`commands::playback::set_playback_volume_impl`) gains an event emit and a lock-free volume mirror so the tray, MPRIS, and the frontend slider all converge on one function instead of three independent volume-setters.

**Tech Stack:** Rust (`ksni` 0.3.6, `tauri` 2, `zbus` 5 — already present via `mpris-server`), TypeScript/React (`@tauri-apps/api/event`).

**Spec:** `docs/superpowers/specs/2026-09-08-tray-volume-design.md`

## Global Constraints

- Linux-only, x86_64-only — no cross-platform fallback code paths (`CLAUDE.md`).
- Package manager: npm only, no pnpm/yarn/bun.
- Rust is the source of truth for volume state — the frontend only ever mirrors what Rust reports, never invents it.
- No new WebView, no extra Tauri window.
- `ksni = "0.3.6"` — confirmed to share the `zbus 5` major version already pinned by `mpris-server 0.10.0` (no duplicate `zbus` copy), default `tokio` feature (shares Tauri's own tokio runtime, no extra async executor).
- Before claiming done: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test` (from `src-tauri/`) and `npm run lint && npm run build` (frontend root).

---

### Task 1: Single choke point for volume changes

**Files:**
- Modify: `src-tauri/src/platform/mpris.rs:55` (`APP_HANDLE` visibility), `src-tauri/src/platform/mpris.rs:395-400` (`set_volume`)
- Modify: `src-tauri/src/commands/playback.rs:41-58` (`set_playback_volume_impl`)
- Test: `src-tauri/src/commands/playback.rs` (existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `crate::platform::mpris::APP_HANDLE` (existing `OnceLock<AppHandle>`, becoming `pub(crate)`), `state.player.lock().await.set_volume(volume)` (existing, unchanged), `commands::playback::set_playback_volume_impl(state: &AppState, volume: u8, persist: bool) -> Result<()>` (existing signature, unchanged — this task only changes its body).
- Produces: `set_playback_volume_impl` now emits a `"volume-changed"` event (`u8` payload) on every successful volume apply, from every caller (frontend command, MPRIS, and — after Task 3 — the tray). Later tasks depend on this event existing and on this being the only place it's emitted.

This task doesn't yet reference `ksni` or `TRAY_VOLUME_HINT`/`TRAY_HANDLE` (those don't exist until Task 3) — write the emit now, and Task 3 will add the two `crate::platform::tray::*` calls into the same function.

- [ ] **Step 1: Make `APP_HANDLE` crate-visible**

In `src-tauri/src/platform/mpris.rs`, change:

```rust
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
```

to:

```rust
pub(crate) static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();
```

Also, in `src-tauri/src/commands/playback.rs`, change the top of the file from:

```rust
use tauri::State;
```

to:

```rust
use tauri::{Emitter, State};
```

(Step 4 below emits an event, which needs the `Emitter` trait in scope — matching how `platform/mpris.rs` already imports it at the top of its own file rather than inline.)

- [ ] **Step 2: Write the failing test for the emit**

Add to `src-tauri/src/commands/playback.rs`'s existing `#[cfg(test)] mod tests` block (near the other two `set_playback_volume_*` tests):

```rust
#[tokio::test]
async fn set_playback_volume_does_not_panic_without_an_app_handle() {
    // In this test binary no real Tauri app is ever built, so
    // `platform::mpris::APP_HANDLE` is never `.set()`. This just proves
    // `set_playback_volume_impl` doesn't panic when the handle is absent
    // (the `if let Some(app) = ...` guard is skipped entirely), matching
    // how `mpris::notify()` already degrades. It still returns `Err` here
    // because `test_state()`'s `Player` is never started — same reason
    // the sibling test `set_playback_volume_persists_even_when_the_live_apply_fails`
    // below asserts `is_err()`, not because of anything to do with the
    // event emit.
    let state = test_state();
    let result = set_playback_volume_impl(&state, 55, false).await;
    assert!(result.is_err());
}
```

- [ ] **Step 3: Run it to confirm it passes already (regression guard, not a red/green step)**

Run: `cd src-tauri && cargo test set_playback_volume_does_not_panic_without_an_app_handle`
Expected: PASS (this step exists to freeze today's "no app handle → no panic" behavior before the emit is added, not to drive new behavior — there is no user-visible assertion possible for "an event fired" without a real `AppHandle`, which this codebase doesn't construct in unit tests anywhere, including `mpris.rs`'s own untested `notify()`).

- [ ] **Step 4: Add the emit to `set_playback_volume_impl`**

In `src-tauri/src/commands/playback.rs`, change:

```rust
pub(crate) async fn set_playback_volume_impl(
    state: &AppState,
    volume: u8,
    persist: bool,
) -> Result<()> {
    if persist {
        let db = state.db.lock().unwrap();
        let mut settings = db.get_settings()?;
        settings.volume = volume;
        db.save_settings(&settings)?;
    }
    state.player.lock().await.set_volume(volume).await
}
```

to:

```rust
pub(crate) async fn set_playback_volume_impl(
    state: &AppState,
    volume: u8,
    persist: bool,
) -> Result<()> {
    if persist {
        let db = state.db.lock().unwrap();
        let mut settings = db.get_settings()?;
        settings.volume = volume;
        db.save_settings(&settings)?;
    }
    let result = state.player.lock().await.set_volume(volume).await;
    if let Some(app) = crate::platform::mpris::APP_HANDLE.get() {
        let _ = app.emit("volume-changed", volume);
    }
    result
}
```

Note: the emit fires even if `set_volume` itself errored (e.g. mpv not started) — same reasoning already documented above this function for `persist`: a volume preference the user set is real even if there's nothing playing to apply it to right now, and the frontend/tray should still reflect the number the user picked.

- [ ] **Step 5: Run the full existing test module**

Run: `cd src-tauri && cargo test --lib commands::playback`
Expected: PASS — all of `set_playback_volume_persists_even_when_the_live_apply_fails`, `set_playback_volume_does_not_persist_when_not_asked_to`, and the new test from Step 2.

- [ ] **Step 6: Route MPRIS's volume setter through the same function**

In `src-tauri/src/platform/mpris.rs`, change:

```rust
async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
    let state = self.app.state::<AppState>();
    let percent = (volume.max(0.0) * 100.0).round() as u8;
    let _ = state.player.lock().await.set_volume(percent).await;
    Ok(())
}
```

to:

```rust
async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
    let state = self.app.state::<AppState>();
    let percent = (volume.max(0.0) * 100.0).round() as u8;
    let _ = commands::playback::set_playback_volume_impl(&state, percent, true).await;
    Ok(())
}
```

This is a behavior change worth calling out in the commit message: MPRIS-set volume now persists to SQLite, which it didn't before.

- [ ] **Step 7: Verify the whole crate still compiles and existing tests pass**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all green.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/commands/playback.rs src-tauri/src/platform/mpris.rs
git commit -m "feat(playback): emit volume-changed from one choke point"
```

---

### Task 2: Add the `ksni` dependency

**Files:**
- Modify: `src-tauri/Cargo.toml:24-41` (`[dependencies]`)

**Interfaces:**
- Produces: the `ksni` crate (default features: `tokio`) available to `src-tauri/src/platform/tray.rs` in Task 3.

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml`, add to `[dependencies]` (alphabetical-ish placement next to the other single-purpose crates, e.g. after `attohttpc`):

```toml
ksni = "0.3.6"
```

- [ ] **Step 2: Resolve and verify no duplicate `zbus`**

Run: `cd src-tauri && cargo check`
Expected: succeeds. Then run:

```bash
grep -A1 '^name = "zbus"' Cargo.lock
```

Expected: exactly one `zbus` entry (still `5.19.0` or whatever `5.x` it resolves to) — if this prints two different `zbus` versions, stop and re-check `ksni`'s `zbus` requirement before continuing (this would mean the dependency-resolution assumption in the spec was wrong).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build(tray): add ksni for status-notifier-item scroll support"
```

---

### Task 3: Rewrite the tray backend on `ksni`

**Files:**
- Modify: `src-tauri/src/platform/tray.rs` (full rewrite)
- Modify: `src-tauri/src/lib.rs:101` (tray setup call site), `src-tauri/src/lib.rs:70` area (seed `TRAY_VOLUME_HINT`)
- Modify: `src-tauri/src/commands/playback.rs` (`set_playback_volume_impl`, adding the two `platform::tray` calls from the spec now that `tray.rs` exports them)
- Test: `src-tauri/src/platform/tray.rs` (new `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `commands::toggle_play_pause(&AppState) -> Result<()>`, `commands::queue::queue_next(AppHandle, State<AppState>) -> Result<...>`, `commands::queue::queue_previous(AppHandle, State<AppState>) -> Result<...>`, `commands::playback::set_playback_volume_impl` (Task 1), `AppState` (`crate::state::AppState`), `app.default_window_icon()` (existing `tauri::App`/`AppHandle` method).
- Produces: `pub(crate) static TRAY_VOLUME_HINT: AtomicU8`, `pub(crate) static TRAY_HANDLE: OnceLock<ksni::Handle<EchoraTray>>`, `pub fn setup(app: &App) -> tauri::Result<()>` (same signature as today, sync window-close wiring only), `pub async fn spawn(app: AppHandle)` (new — builds and registers the `ksni` tray, no return value; logs and degrades on D-Bus failure like `mpris::build` does). Task 1's `set_playback_volume_impl` is extended in this task to call `crate::platform::tray::TRAY_VOLUME_HINT.store(...)` and `crate::platform::tray::TRAY_HANDLE.get()...update(...)`.

- [ ] **Step 1: Write the two pure-function unit tests first (TDD)**

These don't need `ksni`, D-Bus, or a running app — pure input/output, matching how this codebase already isolates testable logic from IPC glue (see `media/player.rs`'s `OBSERVED_PROPERTIES` cache pattern and the audio-reactive-orb spec's normalization function). Add to the bottom of `src-tauri/src/platform/tray.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_argb32_reorders_bytes() {
        // One opaque red pixel: R=255 G=0 B=0 A=255.
        let rgba = vec![255, 0, 0, 255];
        assert_eq!(rgba_to_argb32(&rgba), vec![255, 255, 0, 0]);
    }

    #[test]
    fn rgba_to_argb32_handles_multiple_pixels() {
        // Opaque red, then opaque green (R=0 G=255 B=0 A=255).
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        assert_eq!(
            rgba_to_argb32(&rgba),
            vec![255, 255, 0, 0, 255, 0, 255, 0]
        );
    }

    #[test]
    fn scroll_down_decreases_and_clamps_at_zero() {
        assert_eq!(apply_scroll_delta(3, 1, Orientation::Vertical), 0);
    }

    #[test]
    fn scroll_up_increases_and_clamps_at_hundred() {
        assert_eq!(apply_scroll_delta(98, -1, Orientation::Vertical), 100);
    }

    #[test]
    fn scroll_steps_by_five() {
        assert_eq!(apply_scroll_delta(50, -1, Orientation::Vertical), 55);
        assert_eq!(apply_scroll_delta(50, 1, Orientation::Vertical), 45);
    }

    #[test]
    fn horizontal_scroll_is_ignored() {
        assert_eq!(apply_scroll_delta(50, 1, Orientation::Horizontal), 50);
    }

    #[test]
    fn zero_delta_is_ignored() {
        assert_eq!(apply_scroll_delta(50, 0, Orientation::Vertical), 50);
    }
}
```

- [ ] **Step 2: Run to confirm they fail (functions don't exist yet)**

Run: `cd src-tauri && cargo test --lib platform::tray`
Expected: FAIL to compile — `rgba_to_argb32`/`apply_scroll_delta`/`Orientation` not found.

- [ ] **Step 3: Replace the whole file with the `ksni`-based implementation**

Replace the full contents of `src-tauri/src/platform/tray.rs` with:

```rust
//! System tray icon/menu, backed by `ksni` (a pure-Rust
//! StatusNotifierItem/D-Bus implementation) instead of Tauri's own
//! `tauri::tray`/`muda` stack. See
//! docs/superpowers/specs/2026-09-08-tray-volume-design.md for why:
//! Tauri's Linux tray backend doesn't deliver click or scroll events at
//! all (tauri-apps/tray-icon#104), which rules out real volume control
//! via mouse wheel over the icon — the whole point of this rewrite.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

use ksni::menu::StandardItem;
use ksni::{Icon, MenuItem, Orientation, ToolTip, Tray, TrayMethods};
use tauri::{App, AppHandle, Manager, WindowEvent};

use crate::commands;
use crate::state::AppState;

/// Lock-free mirror of the current volume, readable from `ksni`'s sync
/// `Tray` trait methods (`tool_tip`/`scroll`) without touching the
/// `tokio::sync::Mutex<Player>` those methods can't safely `.await` on
/// (see the design doc's nested-runtime-panic note). Updated by
/// `commands::playback::set_playback_volume_impl` — the same single
/// choke point that emits `volume-changed` to the frontend.
pub(crate) static TRAY_VOLUME_HINT: AtomicU8 = AtomicU8::new(100);

/// Keeps the `ksni` D-Bus service alive for the app's lifetime — unlike
/// `tauri::tray::TrayIconBuilder::build()`, dropping this handle would
/// tear the tray down.
pub(crate) static TRAY_HANDLE: OnceLock<ksni::Handle<EchoraTray>> = OnceLock::new();

pub struct EchoraTray {
    app: AppHandle,
}

impl EchoraTray {
    fn show_and_focus(&self) {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

impl Tray for EchoraTray {
    fn id(&self) -> String {
        "io.github.sidneyroberto9.echora".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        let Some(icon) = self.app.default_window_icon() else {
            return Vec::new();
        };
        vec![Icon {
            width: icon.width() as i32,
            height: icon.height() as i32,
            data: rgba_to_argb32(icon.rgba()),
        }]
    }

    fn tool_tip(&self) -> ToolTip {
        let percent = TRAY_VOLUME_HINT.load(Ordering::Relaxed);
        ToolTip {
            title: "Echora".into(),
            description: format!("Volume {percent}%"),
            ..Default::default()
        }
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let show = StandardItem {
            label: "Show Echora".into(),
            activate: Box::new(|this: &mut Self| this.show_and_focus()),
            ..Default::default()
        };
        let previous = StandardItem {
            label: "Previous".into(),
            activate: Box::new(|this: &mut Self| {
                let app = this.app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let _ = commands::queue::queue_previous(app.clone(), state).await;
                });
            }),
            ..Default::default()
        };
        let play_pause = StandardItem {
            label: "Play / Pause".into(),
            activate: Box::new(|this: &mut Self| {
                let app = this.app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let _ = commands::toggle_play_pause(&state).await;
                });
            }),
            ..Default::default()
        };
        let next = StandardItem {
            label: "Next".into(),
            activate: Box::new(|this: &mut Self| {
                let app = this.app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<AppState>();
                    let _ = commands::queue::queue_next(app.clone(), state).await;
                });
            }),
            ..Default::default()
        };
        let quit = StandardItem {
            label: "Quit Echora".into(),
            activate: Box::new(|this: &mut Self| this.app.exit(0)),
            ..Default::default()
        };
        vec![
            MenuItem::Standard(show),
            MenuItem::Standard(previous),
            MenuItem::Standard(play_pause),
            MenuItem::Standard(next),
            MenuItem::Separator,
            MenuItem::Standard(quit),
        ]
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Linux tray backends historically don't deliver this at all
        // (see module doc comment) — ksni is what makes left-click-to-
        // show newly possible here, not a regression risk.
        self.show_and_focus();
    }

    fn scroll(&mut self, delta: i32, orientation: Orientation) {
        let app = self.app.clone();
        let current = TRAY_VOLUME_HINT.load(Ordering::Relaxed);
        let new_volume = apply_scroll_delta(current, delta, orientation);
        if new_volume == current {
            return;
        }
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _ = commands::playback::set_playback_volume_impl(&state, new_volume, true).await;
        });
    }
}

/// Converts Tauri's RGBA icon bytes to the ARGB32-network-byte-order
/// format `ksni::Icon::data` expects. Pure and unit-tested in isolation
/// — no icon/D-Bus plumbing needed to verify the byte reordering.
fn rgba_to_argb32(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|px| [px[3], px[0], px[1], px[2]])
        .collect()
}

/// One scroll notch = one 5-point volume step, direction from `delta`'s
/// sign (magnitude is ignored — different StatusNotifierItem hosts
/// report different units per notch, so scaling by magnitude isn't
/// reliable; a fixed step per callback invocation is). Horizontal
/// scroll is ignored — no horizontal-scroll-changes-volume convention
/// exists to match.
///
/// NOTE: the sign below (`delta > 0` => volume down) is a guess made
/// without a real desktop to test scroll direction against — if
/// scrolling up on the tray icon turns volume *down* once this ships,
/// flip this one comparison.
fn apply_scroll_delta(current: u8, delta: i32, orientation: Orientation) -> u8 {
    if orientation != Orientation::Vertical || delta == 0 {
        return current;
    }
    const STEP: i32 = 5;
    let change = if delta > 0 { -STEP } else { STEP };
    (current as i32 + change).clamp(0, 100) as u8
}

/// Builds and registers the window's close-hides-to-tray behavior —
/// only the tray's own "Quit Echora" item exits the app
/// (REQUIREMENTS_FREEZE: closing the window minimizes to tray). This is
/// unrelated to the D-Bus tray service itself and unchanged from the
/// previous `tauri::tray`-based implementation.
pub fn setup(app: &App) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .expect("the main window is declared in tauri.conf.json");

    let close_target = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = close_target.hide();
        }
    });

    Ok(())
}

/// Builds and registers the `ksni` tray service. Best-effort like
/// `platform::mpris::build` — a missing/misconfigured D-Bus session
/// must not stop Echora from starting (stability over a nice-to-have
/// desktop integration).
pub async fn spawn(app: AppHandle) {
    let tray = EchoraTray { app };
    match tray.spawn().await {
        Ok(handle) => {
            let _ = TRAY_HANDLE.set(handle);
        }
        Err(err) => {
            eprintln!("tray: session bus unavailable, tray icon disabled: {err}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_to_argb32_reorders_bytes() {
        // One opaque red pixel: R=255 G=0 B=0 A=255.
        let rgba = vec![255, 0, 0, 255];
        assert_eq!(rgba_to_argb32(&rgba), vec![255, 255, 0, 0]);
    }

    #[test]
    fn rgba_to_argb32_handles_multiple_pixels() {
        // Opaque red, then opaque green (R=0 G=255 B=0 A=255).
        let rgba = vec![255, 0, 0, 255, 0, 255, 0, 255];
        assert_eq!(
            rgba_to_argb32(&rgba),
            vec![255, 255, 0, 0, 255, 0, 255, 0]
        );
    }

    #[test]
    fn scroll_down_decreases_and_clamps_at_zero() {
        assert_eq!(apply_scroll_delta(3, 1, Orientation::Vertical), 0);
    }

    #[test]
    fn scroll_up_increases_and_clamps_at_hundred() {
        assert_eq!(apply_scroll_delta(98, -1, Orientation::Vertical), 100);
    }

    #[test]
    fn scroll_steps_by_five() {
        assert_eq!(apply_scroll_delta(50, -1, Orientation::Vertical), 55);
        assert_eq!(apply_scroll_delta(50, 1, Orientation::Vertical), 45);
    }

    #[test]
    fn horizontal_scroll_is_ignored() {
        assert_eq!(apply_scroll_delta(50, 1, Orientation::Horizontal), 50);
    }

    #[test]
    fn zero_delta_is_ignored() {
        assert_eq!(apply_scroll_delta(50, 0, Orientation::Vertical), 50);
    }
}
```

(Step 1's test module is superseded by this full-file replacement — it's the same test code, now alongside real implementations instead of stub-free.)

- [ ] **Step 4: Run the tray unit tests**

Run: `cd src-tauri && cargo test --lib platform::tray`
Expected: all 7 tests PASS. If `ksni`'s actual `menu::StandardItem`/`MenuItem`/`Tray`/`Orientation`/`Icon`/`ToolTip` import paths differ from `use ksni::menu::StandardItem; use ksni::{Icon, MenuItem, Orientation, ToolTip, Tray, TrayMethods};` above (this was sourced from `ksni` 0.3.6's GitHub source, not a local compile), fix the `use` lines per the compiler's suggestions — the type *names* and *fields* are verified accurate, only their exact module paths carry this risk.

- [ ] **Step 5: Wire the two `platform::tray` calls into `set_playback_volume_impl`**

In `src-tauri/src/commands/playback.rs`, extend the function from Task 1 to:

```rust
pub(crate) async fn set_playback_volume_impl(
    state: &AppState,
    volume: u8,
    persist: bool,
) -> Result<()> {
    if persist {
        let db = state.db.lock().unwrap();
        let mut settings = db.get_settings()?;
        settings.volume = volume;
        db.save_settings(&settings)?;
    }
    let result = state.player.lock().await.set_volume(volume).await;

    crate::platform::tray::TRAY_VOLUME_HINT.store(volume, std::sync::atomic::Ordering::Relaxed);
    if let Some(app) = crate::platform::mpris::APP_HANDLE.get() {
        let _ = app.emit("volume-changed", volume);
    }
    if let Some(handle) = crate::platform::tray::TRAY_HANDLE.get() {
        let _ = handle.update(|_| {}).await;
    }

    result
}
```

- [ ] **Step 6: Wire tray startup into `lib.rs`**

In `src-tauri/src/lib.rs`, find:

```rust
            let initial_settings = db.get_settings()?;
```

and immediately after it (still inside the `.setup(|app| { ... })` closure, before `AppState` is constructed) add:

```rust
            platform::tray::TRAY_VOLUME_HINT.store(
                initial_settings.volume,
                std::sync::atomic::Ordering::Relaxed,
            );
```

Then find:

```rust
            platform::tray::setup(app)?;
```

and change it to:

```rust
            platform::tray::setup(app)?;
            tauri::async_runtime::block_on(platform::tray::spawn(app.handle().clone()));
```

- [ ] **Step 7: Full crate build/lint/test**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all green. This is the first point where the whole crate — including `lib.rs`'s new call sites — is exercised together.

- [ ] **Step 8: Manual smoke test**

Run: `cargo tauri dev` (from `src-tauri/`, in the user's own terminal — not something to launch as a background tool process). With Echora running:
1. Confirm the tray icon still appears and the same five menu items still work (Show/Previous/Play-Pause/Next/Quit).
2. Left-click the tray icon — the main window should show and focus (new behavior, wasn't possible before).
3. Scroll the mouse wheel over the tray icon — the player's on-screen volume slider should move. If it moves the wrong direction, flip the `delta > 0` comparison in `apply_scroll_delta` (see the `NOTE` comment above it) and re-run Steps 4/7.
4. Hover the tray icon — the tooltip should read "Echora" / "Volume NN%" and update after both a scroll and a drag of the player's own slider.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/platform/tray.rs src-tauri/src/lib.rs src-tauri/src/commands/playback.rs
git commit -m "feat(tray): replace tauri::tray with ksni for scroll-wheel volume"
```

---

### Task 4: Mirror `volume-changed` into the player's slider

**Files:**
- Modify: `src/lib/api.ts:200-207` (add `onVolumeChanged`)
- Modify: `src/hooks/usePlayback.ts:172-188` area (subscribe to it)

**Interfaces:**
- Consumes: the `"volume-changed"` Tauri event (`number` payload, `0..=100`) from Task 1/3.
- Produces: `api.onVolumeChanged: (callback: (percent: number) => void) => Promise<() => void>`, matching the existing `onTrackAutoAdvanced`/`onPlaybackChanged`/`onTrackUnavailable`/`onAudioLevel` shape in the same file.

- [ ] **Step 1: Add the API wrapper**

In `src/lib/api.ts`, add after `onAudioLevel` (around line 207):

```typescript
  /** Fires whenever the volume actually changes, from any source — the
   * tray's scroll wheel, MPRIS/media keys, or this window's own slider.
   * Mirrors Rust's single choke point, `commands::playback::set_playback_volume_impl`
   * (see docs/superpowers/specs/2026-09-08-tray-volume-design.md). */
  onVolumeChanged: (callback: (percent: number) => void) =>
    listen<number>("volume-changed", (event) => callback(event.payload)),
```

- [ ] **Step 2: Subscribe in `usePlayback`**

In `src/hooks/usePlayback.ts`, add a new `useEffect` right after the existing "Seeds the volume slider from the last saved value" effect (after line 188):

```typescript
  // Rust pushes this whenever volume changes from outside this window's
  // own slider -- the tray's scroll wheel or MPRIS/media keys. Without
  // this, only a fresh app launch would ever pick up a tray/MPRIS
  // volume change (P1-1-style staleness, same class of bug already
  // fixed for queue/playback state via onPlaybackChanged).
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const stop = await api.onVolumeChanged((percent) => {
        setVolumeState(percent);
      });
      if (cancelled) {
        stop();
      } else {
        unlisten = stop;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
```

Note this calls `setVolumeState` directly (the raw `useState` setter), never the `setVolume` callback further down the file — `setVolume` calls back into `api.setPlaybackVolume`, which would turn an incoming external change into a redundant outgoing IPC call right back to Rust.

- [ ] **Step 3: Lint and build**

Run: `npm run lint && npm run build`
Expected: both succeed with no new errors.

- [ ] **Step 4: Manual verification**

With `cargo tauri dev` running (user's own terminal): scroll the tray icon (per Task 3 Step 8) and confirm the on-screen player slider moves to match, live, without needing to click into the window first.

- [ ] **Step 5: Commit**

```bash
git add src/lib/api.ts src/hooks/usePlayback.ts
git commit -m "feat(player): mirror tray/mpris volume changes into the slider"
```

---

### Task 5: Final verification

**Files:** none (verification only)

- [ ] **Step 1: Full backend gate**

Run (from `src-tauri/`): `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: all green.

- [ ] **Step 2: Full frontend gate**

Run (from repo root): `npm run lint && npm run build`
Expected: both succeed.

- [ ] **Step 3: Re-run the full manual smoke sequence from Task 3 Step 8 and Task 4 Step 4 together**

Confirm, in one `cargo tauri dev` session: tray menu items work, left-click shows/focuses the window, scroll changes volume in the correct direction (flip the sign in `apply_scroll_delta` first if Task 3's manual check found it backwards), the tooltip reflects the current percentage, and the player's on-screen slider mirrors a tray-originated change without needing a restart.

- [ ] **Step 4: Report what was and wasn't verified**

Per this project's `CLAUDE.md` ("Before claiming something works"): state plainly which of the above actually ran and passed, and call out anything skipped (e.g. no CI coverage exists or is being added for the D-Bus-driven tray behavior — matching the existing precedent for MPRIS).
