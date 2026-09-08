# Tray volume control — Design

Status: Approved. Not yet implemented.

## Purpose

The tray dropdown (`src-tauri/src/platform/tray.rs`) has no volume
control at all today. The ask is a volume control in the tray that
mirrors the player's main slider (`PlayerView.tsx`'s native
`<input type="range">`, driven by `usePlayback.ts`).

## Key constraint discovered during research

Linux desktop trays (GNOME/Zorin, KDE, XFCE) render tray context menus
through the **DBusMenu** protocol (via AppIndicator/StatusNotifierItem),
which only serializes label/icon/checkbox state over D-Bus — it has no
concept of an embedded widget, so a real GTK `Scale` cannot be embedded
in the menu and rendered by the desktop panel. Tauri's own tray
abstraction (`tauri::tray` / the `tray-icon` crate, built on `muda`)
compounds this: on Linux its Linux backend doesn't emit `Click` or
`Scroll` tray-icon events at all (confirmed via
[tray-icon#104](https://github.com/tauri-apps/tray-icon/issues/104)) —
only the context menu works.

A true continuous control (scroll-wheel-over-icon, the same interaction
`pasystray`/`volumeicon` use) requires the **`ksni`** crate instead — a
pure-Rust, `zbus`-based StatusNotifierItem implementation whose `Tray`
trait exposes `scroll(delta, orientation)` and `activate(x, y)`
directly, matching the StatusNotifierItem D-Bus spec's `Scroll` signal.
This means replacing `tauri::tray`/`muda` for the tray subsystem
entirely (both live on the same D-Bus object; there is no way to keep
`muda`'s menu and bolt on `ksni`'s scroll handling side by side — a
StatusNotifierItem is one D-Bus service, not a menu-plus-icon pair that
can be split across two implementations).

This is a bigger change than adding a menu item — it is a swap of how
Echora's Linux tray integrates with the desktop, which is why this
went through the full brainstorming design process rather than being
implemented directly (see conversation: initial ask was a slider
*inside* the dropdown, which was found infeasible; a GNOME Shell Quick
Settings entry was also considered and rejected — that panel is
GNOME-Shell-owned UI requiring a separate GJS extension stack, GNOME-
only, breaks across Shell versions, out of proportion to this feature).

## Non-goals

- No slider *widget* rendered inside the dropdown menu — the DBusMenu
  limitation above makes this impossible regardless of implementation
  effort. "Mirrors the player slider" is satisfied by two mouse-wheel
  turns and one tray, one number, staying in sync — not by drawing a
  bar in the menu.
- No GNOME Shell Quick Settings integration (see above — separate,
  disproportionate architecture).
- No cross-platform tray code path — Echora is Linux-only
  (`CLAUDE.md`), so `ksni` fully replaces `tauri::tray` with no
  Windows/macOS fallback to maintain.
- No debounce on the scroll-driven volume writes (see Backend §4) —
  deliberate simplification, not an oversight.

## Backend (Rust)

### 1. Single choke point for volume changes — `commands/playback.rs`

Today three code paths can change volume and only one of them
(`set_playback_volume_impl`, called from the frontend's
`set_playback_volume` command) persists to SQLite; MPRIS's
`set_volume` (`platform/mpris.rs:395-400`) calls `Player::set_volume`
directly, silently skipping persistence — an existing bug this design
fixes as a side effect, not a new requirement.

`set_playback_volume_impl`'s signature does **not** change. The
codebase already has an established mechanism for exactly this
problem — `platform/mpris.rs`'s `APP_HANDLE: OnceLock<AppHandle>`,
set once in `mpris::build()` and read by `mpris::notify()` so command
functions that only take `State<AppState>` (no `AppHandle`) can still
push events to the frontend. `notify()` isn't reused as-is here (it
also recomputes queue/MPRIS-property state that's irrelevant to a
volume change); instead `APP_HANDLE` itself becomes `pub(crate)` (it's
already conceptually a generic "reach the frontend" handle per its own
doc comment, not an MPRIS-private detail) and `set_playback_volume_impl`
reads it directly:

```rust
if let Some(app) = crate::platform::mpris::APP_HANDLE.get() {
    let _ = app.emit("volume-changed", volume);
}
```

- `platform/mpris.rs`'s `set_volume` is changed to call
  `commands::playback::set_playback_volume_impl(&state, percent, true)`
  instead of `Player::set_volume` directly — this also makes MPRIS-set
  volume persist for the first time (it doesn't today).
- The tray (`ksni::Tray::scroll`, below) is the third caller, going
  through the same function, with no signature threading needed there
  either.

This is the only place `volume-changed` is emitted — the tray, MPRIS,
and the frontend's own slider all converge on one function, so there
is exactly one place to get this right rather than three.

### 2. Tray rewrite — `platform/tray.rs`

Replace the `tauri::tray`/`MenuBuilder` implementation with a
`ksni`-based one:

- `struct EchoraTray { app: AppHandle }` implementing `ksni::Tray`.
- `menu()` — same five items as today (Show Echora / Previous /
  Play‑Pause / Next / separator / Quit Echora), built with
  `ksni::menu::StandardItem`/`Separator`. Same dispatch logic as the
  current `on_menu_event` match (each item's `activate` closure spawns
  `tauri::async_runtime::spawn` calling the existing
  `commands::*` functions — no behavior change here).
- `activate(x, y)` — show + focus the main window. This is a genuine
  fix, not scope creep: Linux tray backends don't deliver left-click
  today (see the `tray.rs` comment being removed), so this is new
  working behavior that falls directly out of implementing the trait,
  not extra work taken on deliberately.
- `scroll(&mut self, delta: i32, orientation: Orientation)` — adjust
  volume by ±5 percentage points per scroll notch (direction from
  `delta`'s sign; **verify empirically at implementation time** which
  sign means "up" — this varies enough across `Scroll` D-Bus emitters
  that it isn't safe to assume from documentation alone), clamped to
  `0..=100`, calling `set_playback_volume_impl(..., persist: true)`.
- `icon_pixmap()` — convert `app.default_window_icon()`'s RGBA bytes to
  the ARGB32 (network byte order) format `ksni::Icon` expects. This is
  a byte-order transform done once at startup, not a hot path.
- `tool_tip()` and `scroll()` are sync `Tray` trait methods invoked
  from inside `ksni`'s own async service task — blocking on
  `state.player.lock().await` (a `tokio::sync::Mutex`) via
  `tauri::async_runtime::block_on` from in there risks a nested-runtime
  panic (calling `block_on` from a thread already executing inside that
  same tokio runtime). Neither method reads `Player` directly:
  `tray.rs` keeps its own `pub(crate) static TRAY_VOLUME_HINT: AtomicU8`
  (`AtomicU8::new(100)`, matching `Settings::volume`'s own default), a
  lock-free mirror updated by the single choke point in §1 alongside
  the event emit. `tool_tip()` and `scroll()` both just
  `TRAY_VOLUME_HINT.load(Ordering::Relaxed)` — synchronous, no lock, no
  IPC, safe from any calling context. `tool_tip()` returns
  `ksni::ToolTip { title: "Echora".into(), description: format!("Volume {n}%"), ..Default::default() }`.
  Seeded once at startup from `initial_settings.volume` (`lib.rs`'s
  `setup` closure already reads this before anything else runs).
- The `ksni::Handle<EchoraTray>` returned by `tray.spawn().await?`
  (via the `ksni::TrayMethods` trait) must be kept alive for the app's
  lifetime — unlike today's `tauri::tray::TrayIconBuilder::build()`,
  `ksni` has no implicit registry keeping the service alive if the
  handle is dropped. Stored the same way `mpris.rs` stores its
  `AppHandle` — a second `pub(crate) static TRAY_HANDLE: OnceLock<ksni::Handle<EchoraTray>>`
  in `tray.rs`.
- `scroll(&mut self, ...)` itself never blocks or awaits anything — it
  reads `TRAY_VOLUME_HINT`, computes the new value, and
  `tauri::async_runtime::spawn`s a task that calls
  `set_playback_volume_impl` and returns immediately, so by the time
  that spawned task actually runs (and, per §1, calls
  `TRAY_HANDLE.get()` → `handle.update(|_| {}).await` to make `ksni`
  re-read `tool_tip()` and push it over D-Bus), the original `scroll`
  callback has already returned control to `ksni`'s service loop —
  there's no reentrant call into `ksni` from inside its own callback
  frame. The same choke point calls `update()` unconditionally
  regardless of which of the three callers triggered it; a
  tray-originated change refreshing its own tooltip a second time this
  way is harmless.
- The window-close-hides-to-tray wiring (`on_window_event` /
  `WindowEvent::CloseRequested`) is unrelated to the menu/D-Bus backend
  and stays exactly as-is.

### 3. Dependency resolution (confirmed, not just a risk to watch)

`Cargo.lock` pins `zbus 5.19.0` (via `mpris-server 0.10.0`). `ksni
0.3.6`'s `Cargo.toml` declares `zbus = { version = "5", default-features = false }`
— same major version, no duplicate `zbus` copy. `ksni`'s default
feature is `tokio` (enables `zbus/tokio`), matching Tauri's own tokio
runtime — `ksni = "0.3.6"` with default features needs no feature-flag
changes. Add as a plain `ksni = "0.3.6"` dependency.

## Frontend — `src/hooks/usePlayback.ts`

- Add `listen<number>("volume-changed", (event) => setVolumeState(event.payload))`
  (a plain state setter, not the existing `setVolume` callback — that
  one calls back into `api.setPlaybackVolume`, which would loop a
  tray/MPRIS-originated change straight back into a redundant IPC
  call). This is the fix for the actual "mirror" requirement: today
  the frontend only fetches volume once on load and never again, so a
  tray or MPRIS volume change does not appear in the player's slider
  until next app restart.
- No debounce needed on the receiving end — this only ever *sets*
  local state from an external event, it doesn't originate writes.

## Testing

- `commands/playback.rs` already unit-tests `set_playback_volume_impl`
  (persist / no-persist cases) — these keep passing unchanged: in the
  test binary `APP_HANDLE`/`TRAY_HANDLE` are never `.set()` (no real
  Tauri app is constructed in these tests, matching `mpris.rs`, which
  has no unit tests of its own `notify()` emission either), so both
  `if let Some(...)` guards are `None` and the emit/update calls are
  no-ops. No new automated test asserts the event actually fires —
  that's a manual-verification item, consistent with this project's
  existing precedent of not unit-testing MPRIS's D-Bus-facing
  behavior.
- The dB-normalization-style pure-function split doesn't apply here —
  scroll-delta-to-volume-delta is a one-line clamp, not worth
  extracting/unit-testing in isolation.
- `EchoraTray::menu`/`scroll`/`activate` are D-Bus-driven and not
  practically unit-testable without a real D-Bus session + SNI host —
  covered by manual verification (scroll over the tray icon, confirm
  the player's slider moves; move the player's slider, confirm the
  tray tooltip updates) plus the project's standard
  `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
  gate before claiming done.
- Manual check across whatever desktop environment(s) are available
  (at minimum the user's Zorin/GNOME setup, where the AppIndicator
  extension is already confirmed active since the current menu
  renders at all) — no CI coverage for this exists or is being added,
  matching this project's existing precedent of no automated
  Linux-desktop-integration tests for MPRIS either.
