use tauri::State;

use crate::error::Result;
use crate::models::{QueueView, Track};
use crate::state::AppState;

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
