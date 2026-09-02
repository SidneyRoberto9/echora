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
        if !is_visible || is_minimized || !is_focused {
            // Cheap, local, no-IPC gate: skip the mpv round-trip entirely
            // (not just the emit) whenever the window can't be seen.
            continue;
        }

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
