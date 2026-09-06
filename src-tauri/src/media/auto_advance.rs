//! Detects when the currently playing track reaches the end on its own —
//! not via a manual skip, pause, or previous — and advances the queue the
//! same way `commands::queue::queue_next` does.
//!
//! Uses mpv's own `end-file` event (via `Player::end_file_eof_epoch`,
//! surfaced from the persistent IPC connection's reader task — see
//! `media::player`) rather than guessing from `time-pos`/`duration`, which
//! is what this used to do before the P2-1 persistent-connection rework.
//! Confirmed against a real mpv process (0.37, `--idle=yes`) that mpv's own
//! `reason` field on `end-file` already draws exactly the distinction this
//! needs: `"eof"` for a track that played through to its own end, `"stop"`
//! for one replaced by a new `load()` or explicitly stopped — so there's no
//! more inferring it from position/duration going stale.

use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::commands::queue::auto_advance_from_watcher;
use crate::state::AppState;

const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Runs forever. Once per tick: if there's a current queue track, compares
/// mpv's `end_file_eof_epoch` against the baseline captured for it (via
/// `tick_outcome`) and advances the queue when it's moved — i.e. mpv
/// reported a natural end-of-file for the track this loop has been
/// watching.
pub async fn watch(app: AppHandle) {
    let mut interval = tokio::time::interval(POLL_INTERVAL);
    let mut last_track_id: Option<String> = None;
    let mut baseline_epoch: u64 = 0;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();

        let current_id = state.queue.lock().unwrap().current().map(|t| t.id.clone());
        let Some(track_id) = current_id.clone() else {
            // Nothing queued at all -- nothing to watch, and nothing worth
            // locking the player for.
            last_track_id = None;
            continue;
        };

        let epoch = state.player.lock().await.end_file_eof_epoch();
        let (next_baseline, should_advance) = tick_outcome(
            current_id.as_deref(),
            last_track_id.as_deref(),
            baseline_epoch,
            epoch,
        );
        last_track_id = current_id;
        baseline_epoch = next_baseline;

        if should_advance && let Err(err) = auto_advance_from_watcher(&app, state, &track_id).await
        {
            // An unavailable track is *not* a failure here -- `advance_and_play`
            // already treats `EchoraError::TrackUnavailable` as an expected,
            // skippable case (skip to the next candidate, mark it, emit
            // `TRACK_UNAVAILABLE_EVENT`) and never returns it as an `Err`. A
            // real error reaching this point means something else went wrong
            // (DB, mpv/IPC, ...) while advancing off a track that really did
            // just finish playing. Log it instead of silently dropping it --
            // but keep the watcher loop running regardless, since the next
            // tick still needs to keep polling whatever's current.
            eprintln!("auto-advance: failed to advance past track {track_id}: {err}");
        }
    }
}

/// Pure decision for one poll tick — factored out of `watch()`'s loop so
/// it's testable without a real mpv process (mirrors
/// `sponsorblock::parse_segments` doing the same for its own watch loop).
///
/// `baseline_epoch`/`epoch` are `Player::end_file_eof_epoch()` as observed
/// on the previous tick and this one. Returns `(next_baseline_epoch,
/// should_advance)`.
///
/// - The track changed since the last tick (a manual action, or the first
///   tick watching a new track): nothing to compare yet, so this resets
///   the baseline to mpv's *current* epoch rather than advancing — a
///   natural end-of-file the *previous* track hit right as the switch
///   happened must not be misread as this new track already finishing.
/// - Same track, epoch unchanged: still playing (or paused/seeking) —
///   nothing conclusive.
/// - Same track, epoch advanced: mpv reported a natural end-of-file for
///   exactly the track this loop has been watching since the baseline was
///   set — advance.
fn tick_outcome(
    current_id: Option<&str>,
    last_track_id: Option<&str>,
    baseline_epoch: u64,
    epoch: u64,
) -> (u64, bool) {
    if current_id != last_track_id {
        return (epoch, false);
    }
    (epoch, epoch != baseline_epoch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unchanged_epoch_on_the_same_track_does_not_advance() {
        let (baseline, advance) = tick_outcome(Some("a"), Some("a"), 3, 3);
        assert_eq!(baseline, 3);
        assert!(!advance);
    }

    #[test]
    fn an_advanced_epoch_on_the_same_track_triggers_an_advance() {
        let (baseline, advance) = tick_outcome(Some("a"), Some("a"), 3, 4);
        assert_eq!(baseline, 4);
        assert!(advance);
    }

    #[test]
    fn a_track_change_resets_the_baseline_without_advancing() {
        // First tick watching a new track (or a manual action switched it)
        // -- nothing to compare against yet.
        let (baseline, advance) = tick_outcome(Some("b"), Some("a"), 3, 5);
        assert_eq!(baseline, 5);
        assert!(!advance);
    }

    #[test]
    fn the_first_ever_tick_with_no_prior_track_resets_without_advancing() {
        let (baseline, advance) = tick_outcome(Some("a"), None, 0, 0);
        assert_eq!(baseline, 0);
        assert!(!advance);
    }

    #[test]
    fn a_stale_epoch_bump_from_the_previous_track_is_not_carried_over() {
        // The previous track hit a natural eof right as the queue moved on
        // to a new one (epoch bumped from 3 to 4 in between ticks) -- the
        // new track's baseline must start at the *current* epoch (4), not
        // the old one (3), or the very next unrelated bump would look like
        // this brand new track already finished.
        let (baseline_after_switch, advanced_on_switch) = tick_outcome(Some("b"), Some("a"), 3, 4);
        assert_eq!(baseline_after_switch, 4);
        assert!(!advanced_on_switch);

        let (_, advanced_next_tick) = tick_outcome(Some("b"), Some("b"), baseline_after_switch, 4);
        assert!(!advanced_next_tick);
    }
}
