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

#[tauri::command]
pub fn queue_next(state: State<AppState>) -> Option<Track> {
    state.queue.lock().unwrap().next().cloned()
}

#[tauri::command]
pub fn queue_previous(state: State<AppState>) -> Option<Track> {
    state.queue.lock().unwrap().previous().cloned()
}

#[tauri::command]
pub fn queue_skip_to(state: State<AppState>, index: usize) -> Result<Track> {
    state.queue.lock().unwrap().skip_to(index).cloned()
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
