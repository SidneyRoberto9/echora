//! Polls mpv's audio level (via `Player::audio_level_db`, see
//! `media::player`) and emits it to the frontend so the player screen's
//! orb can react to it — but only while there's actually something to
//! show: a track playing, with the window visible and focused. Mirrors
//! `media::auto_advance::watch`'s shape (a `tokio::time::interval` loop
//! spawned once at startup) and its pattern of factoring the per-tick
//! decision into pure, directly-testable functions.
//!
//! Runs at two speeds: `ACTIVE_POLL_INTERVAL` (~12.5Hz) while the gate is
//! open and a level is actually being read, and the much slower
//! `IDLE_POLL_INTERVAL` otherwise — so a hidden/minimized/unfocused/paused
//! player isn't making blocking `is_visible`/`is_minimized`/`is_focused`
//! round-trips to the GTK main thread dozens of times a second forever.
//! `tokio::time::Interval`'s period is fixed at construction (no public
//! setter), so this uses two separate `Interval`s rather than
//! reconfiguring one.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tokio::time::MissedTickBehavior;

use crate::state::AppState;

/// Tick rate while actively metering (gate open).
const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(80); // ~12.5Hz
/// Tick rate while idle (gate closed) — just fast enough to notice
/// playback resuming or the window becoming visible/focused again without
/// polling the GTK main thread at the active rate for no reason.
const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(500); // ~2Hz

/// Below this, a reading is treated as silence (maps to `0.0`); at or
/// above `0.0` dBFS maps to `1.0`. -60dBFS is a common "quiet but not
/// silent" floor for loudness UIs — not derived from any project
/// requirement, a reasonable default to tune later if it looks wrong on
/// real tracks.
const NOISE_FLOOR_DB: f64 = -60.0;

/// Whether the window is in a state where the orb can actually be seen —
/// the cheap, no-IPC half of the metering gate, checked before ever
/// locking the player or talking to mpv.
fn window_can_show(is_visible: bool, is_minimized: bool, is_focused: bool) -> bool {
    is_visible && !is_minimized && is_focused
}

/// Full metering gate: a track actually playing, on top of the window
/// being able to show it. Composes `window_can_show` rather than
/// re-deriving the same condition, so there's exactly one tested place
/// that decides the window half of the gate.
fn should_meter(is_playing: bool, is_visible: bool, is_minimized: bool, is_focused: bool) -> bool {
    is_playing && window_can_show(is_visible, is_minimized, is_focused)
}

/// Maps an RMS dBFS reading (or `None`, meaning no data this tick) to the
/// `0.0..=1.0` range the frontend's CSS custom property expects.
fn normalize_level(db: Option<f64>) -> f32 {
    let Some(db) = db else { return 0.0 };
    let clamped = db.clamp(NOISE_FLOOR_DB, 0.0);
    ((clamped - NOISE_FLOOR_DB) / (0.0 - NOISE_FLOOR_DB)) as f32
}

/// Runs forever. Once per tick: reads the current window/playback state,
/// decides via `should_meter` whether to bother, and if so, makes sure
/// metering is enabled and emits the latest normalized level. The first
/// tick where the gate has just closed (after having been open) emits one
/// final `0.0` so the frontend's `--orb-level` resets instead of freezing
/// at its last value.
pub async fn watch(app: AppHandle) {
    let mut active_interval = tokio::time::interval(ACTIVE_POLL_INTERVAL);
    active_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut idle_interval = tokio::time::interval(IDLE_POLL_INTERVAL);
    idle_interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    let mut was_metering = false;

    loop {
        if was_metering {
            active_interval.tick().await;
        } else {
            idle_interval.tick().await;
        }

        let Some(window) = app.get_webview_window("main") else {
            continue;
        };
        let is_visible = window.is_visible().unwrap_or(false);
        let is_minimized = window.is_minimized().unwrap_or(false);
        let is_focused = window.is_focused().unwrap_or(false);

        if !window_can_show(is_visible, is_minimized, is_focused) {
            // Cheap, local, no-IPC gate: skip the mpv round-trip entirely
            // (not just the emit) whenever the window can't be seen.
            if was_metering {
                was_metering = false;
                let _ = app.emit("audio-level", 0.0f32);
            }
            continue;
        }

        let state = app.state::<AppState>();
        let has_current_track = state.queue.lock().unwrap().current().is_some();

        let mut player = state.player.lock().await;
        let is_playing = has_current_track && matches!(player.is_paused().await, Ok(Some(false)));

        if !should_meter(is_playing, is_visible, is_minimized, is_focused) {
            drop(player);
            if was_metering {
                was_metering = false;
                let _ = app.emit("audio-level", 0.0f32);
            }
            continue;
        }

        was_metering = true;
        let _ = player.enable_level_metering().await;
        let level_db = player.audio_level_db().await.unwrap_or(None);
        drop(player);

        let _ = app.emit("audio-level", normalize_level(level_db));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_can_show_when_visible_unminimized_and_focused() {
        assert!(window_can_show(true, false, true));
    }

    #[test]
    fn window_can_show_false_when_minimized() {
        assert!(!window_can_show(true, true, true));
    }

    #[test]
    fn window_can_show_false_when_unfocused() {
        assert!(!window_can_show(true, false, false));
    }

    #[test]
    fn window_can_show_false_when_hidden_to_tray() {
        // "hidden" (REQUIREMENTS_FREEZE: closing minimizes to tray, see
        // platform::tray) reads as is_visible == false, distinct from the
        // OS-level "minimized" state.
        assert!(!window_can_show(false, false, true));
    }

    #[test]
    fn should_meter_true_when_playing_and_window_can_show() {
        assert!(should_meter(true, true, false, true));
    }

    #[test]
    fn should_meter_false_when_paused() {
        assert!(!should_meter(false, true, false, true));
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
