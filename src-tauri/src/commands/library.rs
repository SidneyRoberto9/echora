use tauri::State;

use crate::error::Result;
use crate::models::Track;
use crate::state::AppState;

#[tauri::command]
pub fn favorite_track(state: State<AppState>, track: Track) -> Result<()> {
    state.db.lock().unwrap().favorite_track(&track)
}

#[tauri::command]
pub fn unfavorite_track(state: State<AppState>, track_id: String) -> Result<()> {
    state.db.lock().unwrap().unfavorite_track(&track_id)
}

#[tauri::command]
pub fn is_track_favorited(state: State<AppState>, track_id: String) -> Result<bool> {
    state.db.lock().unwrap().is_track_favorited(&track_id)
}

#[tauri::command]
pub fn favorite_mood(state: State<AppState>, mood_id: String) -> Result<()> {
    state.db.lock().unwrap().favorite_mood(&mood_id)
}

#[tauri::command]
pub fn unfavorite_mood(state: State<AppState>, mood_id: String) -> Result<()> {
    state.db.lock().unwrap().unfavorite_mood(&mood_id)
}

#[tauri::command]
pub fn list_favorite_moods(state: State<AppState>) -> Result<Vec<String>> {
    state.db.lock().unwrap().list_favorite_moods()
}

#[tauri::command]
pub fn set_track_feedback(state: State<AppState>, track: Track, liked: bool) -> Result<()> {
    state.db.lock().unwrap().set_track_feedback(&track, liked)
}

#[tauri::command]
pub fn get_track_feedback(state: State<AppState>, track_id: String) -> Result<Option<bool>> {
    state.db.lock().unwrap().get_track_feedback(&track_id)
}
