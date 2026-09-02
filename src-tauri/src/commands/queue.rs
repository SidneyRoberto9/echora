use tauri::{Emitter, State};

use crate::error::{EchoraError, Result};
use crate::models::{QueueView, SceneSummary, Track};
use crate::state::AppState;

/// Below this many upcoming tracks, ask the mood engine for more before
/// the queue actually runs dry — a starting default, to be tuned once
/// there's benchmarking (see docs/REQUIREMENTS_FREEZE.md's performance
/// goals).
const LOW_WATERMARK: usize = 3;

/// Emitted after the queue advances on its own (a track finished playing
/// naturally — see `media::auto_advance`), never after a manual
/// next/previous/skip-to. Those are user-triggered and the frontend
/// already knows to refresh its own state right after calling them; a
/// track ending on its own is the one transition Rust has to actively
/// push, since nothing else on the frontend polls the queue.
pub(crate) const TRACK_AUTO_ADVANCED_EVENT: &str = "track-auto-advanced";

#[tauri::command]
pub fn get_queue(state: State<AppState>) -> QueueView {
    state.queue.lock().unwrap().view()
}

/// Records how the current track went, advances the queue, and starts
/// playing whatever's now current — a no-op-ish `None` if the queue was
/// already at its end (callers should top up and retry, or show an
/// end-of-queue state).
#[tauri::command]
pub async fn queue_next(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Track>> {
    advance_and_play(&app, state, None).await
}

/// Core of `queue_next`, shared with the automatic end-of-track watcher
/// (`media::auto_advance`). `expected_current` is `None` for a manual skip
/// (always advances — a user-triggered action takes effect regardless of
/// what's current) or `Some(track_id)` for an automatic advance, which
/// must become a no-op if the queue's current track already changed out
/// from under the watcher (e.g. the user clicked Next/Previous/skip-to at
/// the same moment mpv reported end-of-file) instead of double-advancing.
async fn advance_and_play(
    app: &tauri::AppHandle,
    state: State<'_, AppState>,
    expected_current: Option<&str>,
) -> Result<Option<Track>> {
    // Best-effort even when this turns out to be a stale auto-advance: the
    // `session_tracks` row this writes is keyed on `(session_id,
    // position)` and gets overwritten by whatever mutation "wins" the
    // guard below, so recording it unconditionally first isn't a
    // correctness issue, just occasionally redundant work.
    super::record_current_completion(&state).await?;

    let advanced = advance_queue_if_expected(&state, expected_current);
    if let Some(track) = &advanced {
        super::resolve_and_load(app, &state, track).await?;
    }

    // Best-effort: a stalled top-up shouldn't fail an otherwise-successful skip.
    let _ = ensure_queue_topped_up(app.clone(), state.clone()).await;
    Ok(advanced)
}

/// Advances the queue past its current track if `expected_current` still
/// matches it (or unconditionally when `None`) — the guard itself, atomic
/// under a single lock acquisition so it's race-free against a concurrent
/// manual `queue_next`/`queue_previous`/`queue_skip_to` even though the
/// rest of `advance_and_play` isn't. Split out from `advance_and_play` so
/// it's unit-testable without a real Tauri window (matching
/// `make_room_for_single_track`'s own `&AppState`-only shape).
fn advance_queue_if_expected(state: &AppState, expected_current: Option<&str>) -> Option<Track> {
    let mut queue = state.queue.lock().unwrap();
    if let Some(expected) = expected_current
        && !queue.current().is_some_and(|t| t.id == expected)
    {
        return None;
    }
    queue.next().cloned()
}

/// Entry point for `media::auto_advance`'s watcher: advances the queue only
/// if `track_id` is still current (see `advance_and_play`), then tells the
/// frontend a track just changed on its own — the one propagation gap that
/// isn't already covered by a user-triggered action's own follow-up
/// `refreshQueue()`.
pub(crate) async fn auto_advance_from_watcher(
    app: &tauri::AppHandle,
    state: State<'_, AppState>,
    track_id: &str,
) -> Result<Option<Track>> {
    let advanced = advance_and_play(app, state, Some(track_id)).await?;
    if advanced.is_some() {
        let _ = app.emit(TRACK_AUTO_ADVANCED_EVENT, ());
    }
    Ok(advanced)
}

/// Goes back one track and starts playing it. `None` if already at the
/// first track of the queue — callers should just seek the current track
/// to 0 in that case rather than treating it as an error.
#[tauri::command]
pub async fn queue_previous(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<Track>> {
    let went_back = state.queue.lock().unwrap().previous().cloned();
    if let Some(track) = &went_back {
        super::resolve_and_load(&app, &state, track).await?;
    }
    Ok(went_back)
}

#[tauri::command]
pub async fn queue_skip_to(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    index: usize,
) -> Result<Track> {
    super::record_current_completion(&state).await?;

    let track = {
        let mut queue = state.queue.lock().unwrap();
        queue.skip_to(index)?.clone()
    };
    super::resolve_and_load(&app, &state, &track).await?;
    Ok(track)
}

#[tauri::command]
pub fn queue_remove(state: State<AppState>, index: usize) -> Result<()> {
    state.queue.lock().unwrap().remove(index)
}

/// Tops the queue back up if it's running low, using the active session's
/// mood. A no-op if there's still plenty queued, or if the queue is low
/// but there's no active session to generate more candidates for.
#[tauri::command]
pub async fn ensure_queue_topped_up(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<()> {
    let needs_more = state.queue.lock().unwrap().upcoming().len() < LOW_WATERMARK;
    if !needs_more {
        return Ok(());
    }

    let moods = {
        let db = state.db.lock().unwrap();
        match db.current_session()? {
            Some(session) => session
                .moods
                .into_iter()
                .map(|m| (m.mood_id, m.weight))
                .collect::<Vec<_>>(),
            None => return Ok(()),
        }
    };

    super::top_up_queue(&app, &state, &moods).await
}

/// Clears the way for an ad-hoc single-track play: ends the active
/// session if there is one (a favorited-track replay isn't tied to a
/// mood, so it doesn't start a new one), or clears the queue directly if
/// there wasn't one — either way leaving the queue empty before the
/// caller adds the new track as `current`.
///
/// The "no session" branch matters on its own: without it, a second
/// favorited-track click while one ad-hoc track is already playing (no
/// session was ever started for it) would append behind that track
/// instead of replacing it, since `Queue::add_candidates` only makes a
/// track current when nothing is current yet.
pub(crate) fn make_room_for_single_track(state: &AppState) -> Result<()> {
    if state.db.lock().unwrap().current_session()?.is_some() {
        super::session::end_session_impl(state)?;
    } else {
        state.queue.lock().unwrap().clear();
    }
    Ok(())
}

/// Plays a single track outside of any mood session — used for replaying
/// a favorited track from Discover. Resolving and loading a stream isn't
/// enough on its own: the queue has to become this one track too, or
/// `MiniPlayerBar`/`PlayerView` (which read `queue.current`) would show
/// stale now-playing state. See `make_room_for_single_track`.
#[tauri::command]
pub async fn play_single_track(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    track: Track,
) -> Result<()> {
    make_room_for_single_track(&state)?;
    state.queue.lock().unwrap().add_candidates([track.clone()]);
    super::resolve_and_load(&app, &state, &track).await
}

pub(crate) fn save_scene_impl(state: &AppState, name: &str) -> Result<SceneSummary> {
    let tracks = state.queue.lock().unwrap().all_tracks().to_vec();
    if tracks.is_empty() {
        return Err(EchoraError::QueueEmpty);
    }
    state.db.lock().unwrap().save_scene(name, &tracks)
}

#[tauri::command]
pub fn save_scene(state: State<AppState>, name: String) -> Result<SceneSummary> {
    save_scene_impl(&state, &name)
}

#[tauri::command]
pub fn list_scenes(state: State<AppState>) -> Result<Vec<SceneSummary>> {
    state.db.lock().unwrap().list_scenes()
}

pub(crate) async fn play_scene_impl(
    app: &tauri::AppHandle,
    state: &AppState,
    scene_id: i64,
) -> Result<()> {
    let tracks = state.db.lock().unwrap().scene_tracks(scene_id)?;
    let Some(first) = tracks.first().cloned() else {
        return Err(EchoraError::QueueEmpty);
    };
    make_room_for_single_track(state)?;
    state.queue.lock().unwrap().add_candidates(tracks);
    super::resolve_and_load(app, state, &first).await
}

#[tauri::command]
pub async fn play_scene(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scene_id: i64,
) -> Result<()> {
    play_scene_impl(&app, &state, scene_id).await
}

#[tauri::command]
pub fn rename_scene(state: State<AppState>, scene_id: i64, name: String) -> Result<()> {
    state.db.lock().unwrap().rename_scene(scene_id, &name)
}

#[tauri::command]
pub fn delete_scene(state: State<AppState>, scene_id: i64) -> Result<()> {
    state.db.lock().unwrap().delete_scene(scene_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::media::player::Player;
    use crate::media::resolver::{Resolver, ResolverConfig};
    use crate::moods::MoodCatalog;
    use crate::queue::Queue;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    fn test_state() -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            prefetch: crate::media::prefetch::Prefetch::new(),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("/tmp/echora-test-queue-unused.sock"),
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn make_room_for_single_track_ends_an_active_session_and_clears_its_queue() {
        let state = test_state();
        let mood_id = state.moods.list()[0].id.clone();
        state
            .db
            .lock()
            .unwrap()
            .start_session(&[(mood_id, 100)])
            .unwrap();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);

        make_room_for_single_track(&state).unwrap();

        assert!(
            state
                .db
                .lock()
                .unwrap()
                .current_session()
                .unwrap()
                .is_none()
        );
        assert!(state.queue.lock().unwrap().current().is_none());
    }

    #[test]
    fn make_room_for_single_track_clears_a_leftover_ad_hoc_track_with_no_session() {
        let state = test_state();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("leftover")]);
        assert!(
            state
                .db
                .lock()
                .unwrap()
                .current_session()
                .unwrap()
                .is_none()
        );

        make_room_for_single_track(&state).unwrap();

        assert!(state.queue.lock().unwrap().current().is_none());
    }

    #[test]
    fn save_scene_impl_errors_on_an_empty_queue() {
        let state = test_state();
        let err = save_scene_impl(&state, "Empty").unwrap_err();
        assert!(matches!(err, EchoraError::QueueEmpty));
    }

    #[test]
    fn save_scene_impl_saves_the_whole_queue_including_past_tracks() {
        let state = test_state();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);
        state.queue.lock().unwrap().next(); // "a" is now in the past

        let summary = save_scene_impl(&state, "My Scene").unwrap();

        assert_eq!(summary.name, "My Scene");
        assert_eq!(summary.track_count, 2);
    }

    #[test]
    fn advance_queue_if_expected_with_no_expectation_always_advances() {
        // Matches a manual `queue_next`: a user-triggered skip always takes
        // effect regardless of what's current.
        let state = test_state();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);

        let advanced = advance_queue_if_expected(&state, None);

        assert_eq!(advanced.unwrap().id, "b");
        assert_eq!(state.queue.lock().unwrap().current().unwrap().id, "b");
    }

    #[test]
    fn advance_queue_if_expected_advances_when_the_expected_track_is_still_current() {
        let state = test_state();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);

        let advanced = advance_queue_if_expected(&state, Some("a"));

        assert_eq!(advanced.unwrap().id, "b");
        assert_eq!(state.queue.lock().unwrap().current().unwrap().id, "b");
    }

    #[test]
    fn advance_queue_if_expected_is_a_no_op_when_the_queue_already_moved_on() {
        // The race `media::auto_advance`'s watcher has to guard against:
        // the queue's current track no longer matches whatever the caller
        // last observed as current (e.g. a manual skip/previous already
        // ran) — must not double-advance.
        let state = test_state();
        state
            .queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b"), track("c")]);
        state.queue.lock().unwrap().next(); // current is now "b"

        let advanced = advance_queue_if_expected(&state, Some("a"));

        assert_eq!(advanced, None);
        assert_eq!(state.queue.lock().unwrap().current().unwrap().id, "b");
    }

    #[test]
    fn advance_queue_if_expected_returns_none_at_the_end_of_the_queue() {
        let state = test_state();
        state.queue.lock().unwrap().add_candidates([track("a")]);

        assert_eq!(advance_queue_if_expected(&state, Some("a")), None);
        assert_eq!(advance_queue_if_expected(&state, None), None);
    }
}
