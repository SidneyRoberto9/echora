//! System tray icon/menu, backed by `ksni` (a pure-Rust
//! StatusNotifierItem/D-Bus implementation) instead of Tauri's own
//! `tauri::tray`/`muda` stack. See
//! docs/superpowers/specs/2026-09-08-tray-volume-design.md for why:
//! Tauri's Linux tray backend doesn't deliver click or scroll events at
//! all (tauri-apps/tray-icon#104), which rules out real volume control
//! via mouse wheel over the icon — the whole point of this rewrite.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

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

    fn title(&self) -> String {
        "Echora".into()
    }

    fn tool_tip(&self) -> ToolTip {
        let percent = TRAY_VOLUME_HINT.load(Ordering::Relaxed);
        // StatusNotifierItem's `ToolTip.description` can contain a subset of
        // HTML markup, per the spec. The only interpolated value today is a
        // `u8` percentage, so there's nothing to sanitize yet -- but if this
        // is ever extended to show track metadata (title/artist), that data
        // is untrusted external metadata from yt-dlp/YouTube (per this
        // project's CLAUDE.md) and must be sanitized before landing in an
        // HTML-capable field the desktop panel renders.
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
        // fetch_update (not load-then-compute-then-store) so that scroll
        // notches arriving within the same D-Bus/mpv/SQLite round-trip each
        // see the previous notch's result instead of all reading the same
        // stale value and collapsing into one step -- the spawned task below
        // still takes tens of milliseconds to land in `TRAY_VOLUME_HINT` via
        // `set_playback_volume_impl`, but the hint itself is now updated
        // synchronously right here, before that task ever runs.
        let Ok(previous) =
            TRAY_VOLUME_HINT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |c| {
                let n = apply_scroll_delta(c, delta, orientation);
                (n != c).then_some(n)
            })
        else {
            return;
        };
        let new_volume = apply_scroll_delta(previous, delta, orientation);
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
    rgba.as_chunks::<4>()
        .0
        .iter()
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

    // If `spawn()` (below, called separately from `lib.rs`) failed, there is
    // no tray icon to bring the window back with once this hides it. Still
    // recoverable: relaunching the app hits the single-instance plugin,
    // which shows the existing hidden window instead of starting a second
    // instance.
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
    // Echora's own autostart launches the app before the desktop shell's
    // StatusNotifierWatcher is necessarily up yet. The default
    // `assume_sni_available: false` makes `ksni` hard-fail
    // (`Error::Watcher`/`Error::WontShow`) in that ordinary case, with no
    // retry -- so no tray for the whole session. `assume_sni_available(true)`
    // tells `ksni` to register anyway and pick the service up once the
    // watcher appears, instead of giving up immediately.
    match tray.assume_sni_available(true).spawn().await {
        Ok(handle) => {
            let _ = TRAY_HANDLE.set(handle);
        }
        Err(err) => {
            eprintln!("tray: could not register status-notifier item, tray icon disabled: {err}");
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
        assert_eq!(rgba_to_argb32(&rgba), vec![255, 255, 0, 0, 255, 0, 255, 0]);
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
