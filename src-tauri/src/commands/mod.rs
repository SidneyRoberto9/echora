pub mod crash;
pub mod library;
pub mod link;
pub mod mood;
pub mod playback;
pub mod queue;
pub mod session;
pub mod settings;

use std::collections::HashSet;

use rand::SeedableRng;
use rand::rngs::StdRng;
use tauri::Manager;

use crate::error::Result;
use crate::models::{Mood, SessionInfo, Track};
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// True when `current` (the session id read back from the DB right before a
/// top-up's fetched candidates are appended) still matches the session the
/// fetch was started for. `false` means a newer session took over while the
/// fetch was in flight -- a concurrent top-up finishing late, or the user
/// starting a new mood/link session -- and the candidates must be dropped
/// instead of leaking into whatever's playing now.
pub(crate) fn session_still_current(current: Option<i64>, expected: i64) -> bool {
    current == Some(expected)
}

/// Drops candidates already present in the queue. Two top-ups triggered by
/// the same low-watermark crossing (e.g. two advances inside one slow
/// fetch) can both compute an overlapping batch from the same snapshot;
/// this keeps the second append from duplicating whatever the first one
/// already added.
pub(crate) fn dedup_against_queue(candidates: Vec<Track>, existing: &[Track]) -> Vec<Track> {
    let existing_ids: HashSet<&str> = existing.iter().map(|t| t.id.as_str()).collect();
    candidates
        .into_iter()
        .filter(|t| !existing_ids.contains(t.id.as_str()))
        .collect()
}

/// Generates a fresh batch of candidates for `moods` (1-3 weighted moods)
/// and appends them to the queue. Shared by starting a mixed session and
/// topping the queue back up mid-session — both are "get more candidates
/// for this mix," just triggered at different times.
pub(crate) async fn top_up_queue<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    session_id: i64,
    moods: &[(String, u8)],
) -> Result<()> {
    let resolved: Vec<(&Mood, u8)> = moods
        .iter()
        .map(|(mood_id, weight)| state.moods.get(mood_id).map(|m| (m, *weight)))
        .collect::<Result<_>>()?;
    let config = GenerationConfig::default();
    let ctx = {
        let db = state.db.lock().unwrap();
        mood_engine::build_scoring_context(&db, config.recent_session_window)?
    };

    // A plain `StdRng`, not the thread-local `rand::rng()` — Tauri's async
    // commands require `Send` futures, and `ThreadRng` (`Rc`-based) isn't.
    let mut rng = StdRng::from_rng(&mut rand::rng());
    let candidates = mood_engine::generate_mixed_candidates(
        app,
        &resolved,
        &state.resolver,
        &ctx,
        &config,
        &mut rng,
    )
    .await?;

    // The fetch above can take a while; if a newer session started (or this
    // one just ended) while it was in flight, these candidates belong to a
    // mix nobody's listening to anymore -- drop them instead of appending
    // into whatever's current now.
    let current = state.db.lock().unwrap().current_session()?.map(|s| s.id);
    if !session_still_current(current, session_id) {
        return Ok(());
    }
    let mut queue = state.queue.lock().unwrap();
    let candidates = dedup_against_queue(candidates, queue.all_tracks());
    queue.add_candidates(candidates);
    Ok(())
}

/// Resolves `track` to a playable stream and hands it to the mpv sidecar,
/// starting mpv on first use. The one place playback actually begins —
/// every command that changes "what's current" routes through this.
pub(crate) async fn resolve_and_load(
    app: &tauri::AppHandle,
    state: &AppState,
    track: &Track,
) -> Result<()> {
    let resolved = match state.prefetch.take_matching(&track.id).await {
        Some(result) => result?,
        None => state.resolver.resolve_with_retry(app, &track.id).await?,
    };
    let mut player = state.player.lock().await;
    if !player.is_started() {
        player.start(app).await?;
        // A freshly-spawned mpv always starts at its own default volume --
        // apply whatever the user last set before this one existed.
        let saved_volume = state.db.lock().unwrap().get_settings()?.volume;
        player.set_volume(saved_volume).await?;
    }
    player.load(&resolved.stream_url).await?;
    drop(player);
    crate::platform::notify_playback_changed(state).await;

    // Best-effort: resolve whatever's now next in the background, so it's
    // ready by the time the user actually gets there instead of paying the
    // yt-dlp/Deno round trip on the critical path again.
    let next = state.queue.lock().unwrap().upcoming().first().cloned();
    if let Some(next) = next {
        let app = app.clone();
        let next_id = next.id.clone();
        let handle = tokio::spawn(async move {
            let state = app.state::<AppState>();
            state.resolver.resolve_with_retry(&app, &next_id).await
        });
        state.prefetch.spawn(next.id, handle).await;
    }

    Ok(())
}

/// Toggles play/pause, used by both the tray menu and MPRIS's `PlayPause`
/// method — the one place that needs to know the *current* paused state to
/// decide which way to flip it (MPRIS's own `Play`/`Pause` are directed and
/// don't need this).
pub(crate) async fn toggle_play_pause(state: &AppState) -> Result<()> {
    let mut player = state.player.lock().await;
    let paused = player.is_paused().await?.unwrap_or(false);
    player.set_paused(!paused).await?;
    drop(player);
    crate::platform::notify_playback_changed(state).await;
    Ok(())
}

/// Records how far the listener got into the currently-current track
/// before it stops being current (advancing, going back, or the app
/// closing mid-track). A no-op if there's no current track or no active
/// session — nothing to attribute the play to.
pub(crate) async fn record_current_completion(state: &AppState) -> Result<()> {
    let snapshot = {
        let queue = state.queue.lock().unwrap();
        match (queue.current().cloned(), queue.play_ordinal()) {
            (Some(track), Some(ordinal)) => Some((track, ordinal)),
            _ => None,
        }
    };
    let Some((track, play_ordinal)) = snapshot else {
        return Ok(());
    };

    let session = state.db.lock().unwrap().current_session()?;
    let Some(session) = session else {
        return Ok(());
    };

    // Privacy gate: "Save listening history" off means nothing gets
    // written, full stop — checked before touching the player at all, so
    // disabling it also skips the two mpv round-trips below (P1-2).
    if !state.db.lock().unwrap().get_settings()?.history_enabled {
        return Ok(());
    }

    // Both reads in one lock acquisition: between two separate `lock()`
    // calls mpv can unload the track (e.g. it just hit end-of-file),
    // turning `duration` into `None` and `completion` into `None` — which
    // `Db::listening_stats` then reads back as zero seconds listened for
    // a play that actually completed (P2-4).
    let (elapsed, duration) = {
        let mut player = state.player.lock().await;
        let elapsed = player.position_seconds().await.unwrap_or(None);
        let duration = player.duration_seconds().await.unwrap_or(None);
        (elapsed, duration)
    };
    let completion = match (elapsed, duration) {
        (Some(e), Some(d)) if d > 0.0 => Some((e / d).clamp(0.0, 1.0)),
        _ => None,
    };

    state
        .db
        .lock()
        .unwrap()
        .record_play(session.id, &track, play_ordinal as u32, completion)?;
    Ok(())
}

/// Starts a session for `moods`, fetches its first batch of candidates, and
/// immediately starts playing the first one — shared by `start_mood_session`,
/// `start_mixed_session`, and `surprise_me`, which only differ in how they
/// pick `moods`.
pub(crate) async fn start_session_and_play(
    app: &tauri::AppHandle,
    state: &AppState,
    moods: &[(String, u8)],
) -> Result<SessionInfo> {
    let session = crate::commands::session::start_session_impl(state, moods).await?;
    top_up_queue(app, state, session.id, moods).await?;

    let current = state.queue.lock().unwrap().current().cloned();
    if let Some(track) = current {
        resolve_and_load(app, state, &track).await?;
    }
    Ok(session)
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

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

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
                PathBuf::from("/tmp/echora-test-commands-mod-unused.sock"),
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
            mpris: None,
            discord: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// The player is never started here (no mpv socket to connect to), so
    /// `position_seconds`/`duration_seconds` both fail and fall back to
    /// `None` -- exercising `record_current_completion` without needing a
    /// live sidecar, same as `playback::tests::test_state`.
    #[tokio::test]
    async fn record_current_completion_is_a_noop_when_history_disabled() {
        let state = test_state();
        {
            let db = state.db.lock().unwrap();
            let mut settings = db.get_settings().unwrap();
            settings.history_enabled = false;
            db.save_settings(&settings).unwrap();
            db.start_session(&[("villain".to_string(), 100)]).unwrap();
        }
        state.queue.lock().unwrap().add_candidates([track("a")]);

        record_current_completion(&state).await.unwrap();

        let sessions = state.db.lock().unwrap().list_sessions(10, 0).unwrap();
        assert_eq!(sessions[0].track_count, 0);
    }

    #[test]
    fn session_still_current_is_true_when_ids_match() {
        assert!(session_still_current(Some(1), 1));
    }

    #[test]
    fn session_still_current_is_false_when_a_different_session_is_open() {
        assert!(!session_still_current(Some(2), 1));
    }

    #[test]
    fn session_still_current_is_false_when_no_session_is_open() {
        assert!(!session_still_current(None, 1));
    }

    #[test]
    fn dedup_against_queue_drops_candidates_already_in_the_queue() {
        let existing = [track("a"), track("b")];
        let deduped = dedup_against_queue(vec![track("b"), track("c")], &existing);
        assert_eq!(
            deduped.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["c"]
        );
    }

    #[test]
    fn dedup_against_queue_keeps_everything_when_the_queue_is_empty() {
        let deduped = dedup_against_queue(vec![track("a"), track("b")], &[]);
        assert_eq!(deduped.len(), 2);
    }

    #[tokio::test]
    async fn record_current_completion_writes_when_history_enabled() {
        let state = test_state();
        {
            let db = state.db.lock().unwrap();
            db.start_session(&[("villain".to_string(), 100)]).unwrap();
        }
        state.queue.lock().unwrap().add_candidates([track("a")]);

        record_current_completion(&state).await.unwrap();

        let sessions = state.db.lock().unwrap().list_sessions(10, 0).unwrap();
        assert_eq!(sessions[0].track_count, 1);
    }
}
