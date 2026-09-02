//! Detects when the currently playing track reaches the end on its own —
//! not via a manual skip, pause, or previous — and advances the queue the
//! same way `commands::queue::queue_next` does.
//!
//! mpv is only ever talked to over one-shot IPC commands today (see
//! `media::player`'s Fase 3 note on `observe_property`), so there's no
//! `end-file` event to listen for yet. Instead this infers "the track just
//! finished" the same way it already infers anything else about live
//! playback state: polling mpv's own `time-pos`/`duration` a la
//! `sponsorblock::watch`. Confirmed against a real mpv process (0.37,
//! `--idle=yes`, no `--keep-open`): both a natural end-of-file *and* a
//! failed `loadfile` make `time-pos`/`duration` go "property unavailable"
//! and `idle-active` become `true` — there is no separate mpv-side signal
//! to tell them apart. The two are told apart here by whether the track
//! was actually observed nearing its own duration first; a failed load
//! never was.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::queue::auto_advance_from_watcher;
use crate::state::AppState;

/// How close to the end (seconds remaining) counts as "this track was
/// about to finish" — deliberately wider than one poll tick so a short
/// track's last real position reading before mpv unloads isn't missed.
const NEAR_END_MARGIN_SECONDS: f64 = 2.0;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Runs forever. Once per tick: if the current queue track hasn't changed
/// since the last tick, checks mpv's position/duration for it and decides
/// (via `tick_outcome`) whether that means the track just finished on its
/// own.
pub async fn watch(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut last_track_id: Option<String> = None;
    let mut neared_completion = false;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();

        let current_id = state.queue.lock().unwrap().current().map(|t| t.id.clone());
        if current_id != last_track_id {
            // Track changed (a manual action, a previous auto-advance, or
            // the very first track of a session) — nothing to compare
            // against yet, so just start tracking it from here.
            last_track_id = current_id;
            neared_completion = false;
            continue;
        }
        let Some(track_id) = current_id else {
            continue; // nothing queued at all
        };

        let mut player = state.player.lock().await;
        let position = player.position_seconds().await.unwrap_or(None);
        let duration = player.duration_seconds().await.unwrap_or(None);
        drop(player);

        let (next_neared_completion, should_advance) =
            tick_outcome(position, duration, neared_completion);
        neared_completion = next_neared_completion;
        if should_advance {
            let _ = auto_advance_from_watcher(&app, state, &track_id).await;
        }
    }
}

/// Pure decision for one poll tick against a single, unchanged current
/// track — factored out of `watch()`'s loop so it's testable without a
/// real mpv process (mirrors `sponsorblock::parse_segments` doing the same
/// for its own watch loop). Returns the `neared_completion` flag to carry
/// into the next tick and whether this tick means "the track just finished
/// naturally, advance now."
///
/// - `(Some, Some)` with a known duration: mpv has the file loaded and is
///   reporting live position — always the case whether playing, paused, or
///   mid-seek. Just updates how close to the end it is.
/// - `(None, _)` after having been observed near the end: mpv went idle
///   right after this exact track was close to done — a natural finish.
/// - `(None, _)` without that: mpv went idle without the track ever having
///   been seen nearing completion (e.g. a failed load) — not a natural
///   finish, leave `neared_completion` as-is (it's already `false` in
///   every reachable case, since a track that starts by failing to load
///   never had a `(Some, Some)` tick to set it `true`).
/// - anything else (duration not yet known, or reported as `<= 0.0`):
///   nothing conclusive this tick, carry the flag forward unchanged.
fn tick_outcome(
    position: Option<f64>,
    duration: Option<f64>,
    neared_completion: bool,
) -> (bool, bool) {
    match (position, duration) {
        (Some(pos), Some(dur)) if dur > 0.0 => (dur - pos <= NEAR_END_MARGIN_SECONDS, false),
        (None, _) if neared_completion => (false, true),
        _ => (neared_completion, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_from_the_end_does_not_count_as_near_completion() {
        let (neared, advance) = tick_outcome(Some(10.0), Some(180.0), false);
        assert!(!neared);
        assert!(!advance);
    }

    #[test]
    fn within_the_margin_of_the_end_counts_as_near_completion() {
        let (neared, advance) = tick_outcome(Some(179.0), Some(180.0), false);
        assert!(neared);
        assert!(!advance);
    }

    #[test]
    fn going_idle_right_after_nearing_the_end_triggers_an_advance_and_resets() {
        let (neared, advance) = tick_outcome(None, None, true);
        assert!(!neared);
        assert!(advance);
    }

    #[test]
    fn going_idle_without_ever_nearing_the_end_does_not_advance() {
        // The shape of a failed load: mpv reports nothing loaded, but this
        // track was never observed close to its own duration first.
        let (neared, advance) = tick_outcome(None, None, false);
        assert!(!neared);
        assert!(!advance);
    }

    #[test]
    fn an_unknown_or_invalid_duration_is_not_treated_as_near_completion() {
        let (neared, advance) = tick_outcome(Some(5.0), Some(0.0), false);
        assert!(!neared);
        assert!(!advance);

        let (neared, advance) = tick_outcome(Some(5.0), None, false);
        assert!(!neared);
        assert!(!advance);
    }

    #[test]
    fn seeking_back_from_the_end_clears_a_stale_near_completion_flag() {
        // A user seeks close to the end, then back to the middle, without
        // the track ever actually finishing — the next live reading must
        // overwrite the flag, not leave a stale `true` that a later
        // unrelated idle transition could misread as a natural finish.
        let (neared, _) = tick_outcome(Some(30.0), Some(180.0), true);
        assert!(!neared);
    }
}
