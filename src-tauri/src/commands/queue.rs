use tauri::{Emitter, State};

use crate::error::{EchoraError, Result};
use crate::models::{QueueView, SceneSummary, Track};
use crate::queue::Queue;
use crate::state::AppState;

/// Below this many upcoming tracks, ask the mood engine for more before
/// the queue actually runs dry — a starting default, to be tuned once
/// there's benchmarking (see docs/REQUIREMENTS_FREEZE.md's performance
/// goals).
const LOW_WATERMARK: usize = 3;

/// How many consecutive unavailable tracks one advance will skip past
/// before giving up rather than trying candidates forever. Not needed for
/// termination — `Queue::next()` already stops on its own at the end of
/// the queue — but bounds how much synchronous yt-dlp-round-tripping a
/// single advance can rack up if a long stretch of the queue turns out to
/// be dead all at once.
const MAX_UNAVAILABLE_SKIPS_PER_ADVANCE: usize = 5;

/// Emitted after the queue advances on its own (a track finished playing
/// naturally — see `media::auto_advance`), never after a manual
/// next/previous/skip-to. Those are user-triggered and the frontend
/// already knows to refresh its own state right after calling them; a
/// track ending on its own is the one transition Rust has to actively
/// push, since nothing else on the frontend polls the queue.
pub(crate) const TRACK_AUTO_ADVANCED_EVENT: &str = "track-auto-advanced";

/// Emitted whenever an advance had to skip one or more tracks because they
/// resolved as unavailable (`EchoraError::TrackUnavailable` — private,
/// removed, region-blocked, etc.), in addition to (not instead of)
/// `TRACK_AUTO_ADVANCED_EVENT`/the command's own return value. This is the
/// user-facing signal for "some tracks were skipped and marked unavailable"
/// — including the case where *every* remaining candidate was unavailable,
/// so nothing at all ended up playing. Payload: every skipped track, in the
/// order they were tried, each with the reason it failed.
pub(crate) const TRACK_UNAVAILABLE_EVENT: &str = "track-unavailable";

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct SkippedTrack {
    pub id: String,
    pub title: String,
    pub reason: String,
}

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

    let initial = advance_queue_if_expected(&state, expected_current);

    // `try_play` owns its own clone of `app`/`state` per call (it's
    // `FnMut`, called once per candidate tried) so the originals below
    // stay available for the DB-marking/event/top-up work afterward.
    let try_play_app = app.clone();
    let try_play_state = state.clone();
    let (playing, skipped) = advance_past_unavailable(&state.queue, initial, move |track| {
        let app = try_play_app.clone();
        let state = try_play_state.clone();
        async move { super::resolve_and_load(&app, &state, &track).await }
    })
    .await?;

    if !skipped.is_empty() {
        {
            let db = state.db.lock().unwrap();
            for (track, reason) in &skipped {
                // Best-effort: failing to persist "this one's dead" must
                // not turn an otherwise-successful skip into a hard error
                // -- worst case it just gets offered again next round.
                let _ = db.mark_track_unavailable(&track.id, reason);
            }
        }
        let payload: Vec<SkippedTrack> = skipped
            .into_iter()
            .map(|(track, reason)| SkippedTrack {
                id: track.id,
                title: track.title,
                reason,
            })
            .collect();
        let _ = app.emit(TRACK_UNAVAILABLE_EVENT, payload);
    }

    // Best-effort: a stalled top-up shouldn't fail an otherwise-successful skip.
    let _ = ensure_queue_topped_up(app.clone(), state.clone()).await;
    Ok(playing)
}

/// Tries `try_play` against `first`, then each subsequent track
/// `queue.next()` yields, for as long as it keeps failing with
/// `EchoraError::TrackUnavailable` — the "skip a dead track and try the
/// next one" behavior `advance_and_play` needs on top of resolving a
/// single track. Any other error (mpv/IO/DB trouble, not the track's own
/// fault) is propagated immediately instead of being treated as skippable.
/// Gives up after `MAX_UNAVAILABLE_SKIPS_PER_ADVANCE` consecutive
/// unavailable tracks, or once the queue itself runs out — whichever comes
/// first.
///
/// ponytail: when it gives up, the queue's `current()` is left on the last
/// track that was tried (now marked unavailable by the caller) rather than
/// rolled back or cleared — `Queue` has no "unset current" operation to do
/// either. A later top-up can still append fresh candidates after it, but
/// nothing auto-resumes into them on its own; that needs a manual
/// next/skip-to (or a `Queue` API addition), whichever comes up first. This
/// is still strictly better than the bug being fixed here, where even one
/// unavailable track froze the session for good.
///
/// Split out from `advance_and_play` so the skip-and-cap behavior is
/// testable against a fake `try_play`, without a real resolver, mpv, or DB.
/// Returns the track that ended up loaded (`None` if every candidate up to
/// the cap or the queue's end was unavailable) plus every track that was
/// skipped along the way, paired with its unavailability reason.
async fn advance_past_unavailable<F, Fut>(
    queue: &std::sync::Mutex<Queue>,
    first: Option<Track>,
    mut try_play: F,
) -> Result<(Option<Track>, Vec<(Track, String)>)>
where
    F: FnMut(Track) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut skipped = Vec::new();
    let mut current = first;
    while let Some(track) = current {
        match try_play(track.clone()).await {
            Ok(()) => return Ok((Some(track), skipped)),
            Err(EchoraError::TrackUnavailable(reason)) => {
                skipped.push((track, reason));
                if skipped.len() >= MAX_UNAVAILABLE_SKIPS_PER_ADVANCE {
                    return Ok((None, skipped));
                }
                current = queue.lock().unwrap().next().cloned();
            }
            Err(err) => return Err(err),
        }
    }
    Ok((None, skipped))
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

    #[tokio::test]
    async fn advance_past_unavailable_plays_the_first_track_immediately_when_it_works() {
        let queue = Mutex::new(Queue::new());
        let (playing, skipped) =
            advance_past_unavailable(&queue, Some(track("a")), |_track| async { Ok(()) })
                .await
                .unwrap();

        assert_eq!(playing.unwrap().id, "a");
        assert!(skipped.is_empty());
    }

    #[tokio::test]
    async fn advance_past_unavailable_is_a_noop_when_there_is_nothing_to_try() {
        // Matches `advance_queue_if_expected` returning `None` (queue
        // already at its end, or a stale auto-advance lost the race) --
        // must not call `try_play` at all.
        let queue = Mutex::new(Queue::new());
        let called = std::cell::Cell::new(false);
        let (playing, skipped) = advance_past_unavailable(&queue, None, |_track: Track| {
            called.set(true);
            async { Ok(()) }
        })
        .await
        .unwrap();

        assert!(playing.is_none());
        assert!(skipped.is_empty());
        assert!(!called.get());
    }

    #[tokio::test]
    async fn advance_past_unavailable_skips_a_dead_track_and_plays_the_next_one() {
        let queue = Mutex::new(Queue::new());
        queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);
        // Position is now at "a" -- matches what `advance_queue_if_expected`
        // would already have done before handing "a" over as `first`.
        let attempts = std::cell::RefCell::new(Vec::new());

        let (playing, skipped) = advance_past_unavailable(&queue, Some(track("a")), |t| {
            attempts.borrow_mut().push(t.id.clone());
            async move {
                if t.id == "a" {
                    Err(EchoraError::TrackUnavailable("removed".into()))
                } else {
                    Ok(())
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(playing.unwrap().id, "b");
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].0.id, "a");
        assert_eq!(skipped[0].1, "removed");
        assert_eq!(*attempts.borrow(), vec!["a".to_string(), "b".to_string()]);
        // The queue itself ends up parked on the track that actually played.
        assert_eq!(queue.lock().unwrap().current().unwrap().id, "b");
    }

    #[tokio::test]
    async fn advance_past_unavailable_stops_cleanly_once_the_queue_runs_out() {
        let queue = Mutex::new(Queue::new());
        queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);

        let (playing, skipped) = advance_past_unavailable(&queue, Some(track("a")), |_t| async {
            Err(EchoraError::TrackUnavailable("removed".into()))
        })
        .await
        .unwrap();

        assert!(playing.is_none());
        assert_eq!(skipped.len(), 2);
    }

    #[tokio::test]
    async fn advance_past_unavailable_gives_up_after_the_cap_even_with_more_tracks_left() {
        // One more track than the cap allows, all unavailable -- proves
        // the cap actually bounds the work done, instead of the queue's
        // own length being the only thing stopping it.
        let queue = Mutex::new(Queue::new());
        let ids: Vec<String> = (0..MAX_UNAVAILABLE_SKIPS_PER_ADVANCE + 2)
            .map(|i| format!("t{i}"))
            .collect();
        queue
            .lock()
            .unwrap()
            .add_candidates(ids.iter().map(|id| track(id)));
        let attempts = std::cell::Cell::new(0usize);

        let (playing, skipped) = advance_past_unavailable(&queue, Some(track(&ids[0])), |_t| {
            attempts.set(attempts.get() + 1);
            async { Err(EchoraError::TrackUnavailable("removed".into())) }
        })
        .await
        .unwrap();

        assert!(playing.is_none());
        assert_eq!(skipped.len(), MAX_UNAVAILABLE_SKIPS_PER_ADVANCE);
        assert_eq!(attempts.get(), MAX_UNAVAILABLE_SKIPS_PER_ADVANCE);
    }

    #[tokio::test]
    async fn advance_past_unavailable_propagates_a_non_unavailability_error_without_skipping() {
        let queue = Mutex::new(Queue::new());
        queue
            .lock()
            .unwrap()
            .add_candidates([track("a"), track("b")]);
        let attempts = std::cell::Cell::new(0usize);

        let err = advance_past_unavailable(&queue, Some(track("a")), |_t| {
            attempts.set(attempts.get() + 1);
            async { Err(EchoraError::Sidecar("mpv died".into())) }
        })
        .await
        .unwrap_err();

        assert!(matches!(err, EchoraError::Sidecar(_)));
        assert_eq!(attempts.get(), 1);
    }
}
