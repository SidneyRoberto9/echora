use tauri::State;

use crate::error::Result;
use crate::models::Settings;
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    state.db.lock().unwrap().get_settings()
}

#[tauri::command]
pub fn update_settings(state: State<AppState>, settings: Settings) -> Result<()> {
    state.db.lock().unwrap().save_settings(&settings)
}
