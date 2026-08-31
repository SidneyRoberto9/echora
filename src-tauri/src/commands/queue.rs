use tauri::State;

use crate::error::Result;
use crate::models::{QueueView, Track};
use crate::state::AppState;

/// Below this many upcoming tracks, ask the mood engine for more before
/// the queue actually runs dry — a starting default, to be tuned once
/// there's benchmarking (see docs/REQUIREMENTS_FREEZE.md's performance
/// goals).
const LOW_WATERMARK: usize = 3;

#[tauri::command]
pub fn get_queue(state: State<AppState>) -> QueueView {
    state.queue.lock().unwrap().view()
}

/// Records how the current track went, advances the queue, and starts
/// playing whatever's now current — a no-op-ish `None` if the queue was
/// already at its end (callers should top up and retry, or show an
/// end-of-queue state).
#[tauri::command]
pub async fn queue_next(state: State<'_, AppState>) -> Result<Option<Track>> {
    super::record_current_completion(&state).await?;

    let advanced = state.queue.lock().unwrap().next().cloned();
    if let Some(track) = &advanced {
        super::resolve_and_load(&state, track).await?;
    }

    // Best-effort: a stalled top-up shouldn't fail an otherwise-successful skip.
    let _ = ensure_queue_topped_up(state.clone()).await;
    Ok(advanced)
}

/// Goes back one track and starts playing it. `None` if already at the
/// first track of the queue — callers should just seek the current track
/// to 0 in that case rather than treating it as an error.
#[tauri::command]
pub async fn queue_previous(state: State<'_, AppState>) -> Result<Option<Track>> {
    let went_back = state.queue.lock().unwrap().previous().cloned();
    if let Some(track) = &went_back {
        super::resolve_and_load(&state, track).await?;
    }
    Ok(went_back)
}

#[tauri::command]
pub async fn queue_skip_to(state: State<'_, AppState>, index: usize) -> Result<Track> {
    super::record_current_completion(&state).await?;

    let track = {
        let mut queue = state.queue.lock().unwrap();
        queue.skip_to(index)?.clone()
    };
    super::resolve_and_load(&state, &track).await?;
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
pub async fn ensure_queue_topped_up(state: State<'_, AppState>) -> Result<()> {
    let needs_more = state.queue.lock().unwrap().upcoming().len() < LOW_WATERMARK;
    if !needs_more {
        return Ok(());
    }

    let mood_id = {
        let db = state.db.lock().unwrap();
        match db.current_session()? {
            Some(session) => session.mood_id,
            None => return Ok(()),
        }
    };

    super::top_up_queue(&state, &mood_id).await
}
