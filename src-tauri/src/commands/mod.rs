pub mod crash;
pub mod library;
pub mod mood;
pub mod playback;
pub mod queue;
pub mod session;
pub mod settings;

use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::error::Result;
use crate::models::{Mood, SessionInfo, Track};
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// Generates a fresh batch of candidates for `moods` (1-3 weighted moods)
/// and appends them to the queue. Shared by starting a mixed session and
/// topping the queue back up mid-session — both are "get more candidates
/// for this mix," just triggered at different times.
pub(crate) async fn top_up_queue<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
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
    state.queue.lock().unwrap().add_candidates(candidates);
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
    let resolved = state.resolver.resolve_with_retry(app, &track.id).await?;
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
    crate::platform::mpris::notify(state).await;
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
    crate::platform::mpris::notify(state).await;
    Ok(())
}

/// Records how far the listener got into the currently-current track
/// before it stops being current (advancing, going back, or the app
/// closing mid-track). A no-op if there's no current track or no active
/// session — nothing to attribute the play to.
pub(crate) async fn record_current_completion(state: &AppState) -> Result<()> {
    let snapshot = {
        let queue = state.queue.lock().unwrap();
        match (queue.current().cloned(), queue.position()) {
            (Some(track), Some(position)) => Some((track, position)),
            _ => None,
        }
    };
    let Some((track, position)) = snapshot else {
        return Ok(());
    };

    let session = state.db.lock().unwrap().current_session()?;
    let Some(session) = session else {
        return Ok(());
    };

    let elapsed = state
        .player
        .lock()
        .await
        .position_seconds()
        .await
        .unwrap_or(None);
    let duration = state
        .player
        .lock()
        .await
        .duration_seconds()
        .await
        .unwrap_or(None);
    let completion = match (elapsed, duration) {
        (Some(e), Some(d)) if d > 0.0 => Some((e / d).clamp(0.0, 1.0)),
        _ => None,
    };

    state
        .db
        .lock()
        .unwrap()
        .record_play(session.id, &track, position as u32, completion)?;
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
    let session = crate::commands::session::start_session_impl(state, moods)?;
    top_up_queue(app, state, moods).await?;

    let current = state.queue.lock().unwrap().current().cloned();
    if let Some(track) = current {
        resolve_and_load(app, state, &track).await?;
    }
    Ok(session)
}
