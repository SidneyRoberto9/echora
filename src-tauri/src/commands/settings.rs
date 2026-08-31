use tauri::{AppHandle, State};

use crate::error::Result;
use crate::models::Settings;
use crate::platform::autostart;
use crate::state::AppState;

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    state.db.lock().unwrap().get_settings()
}

#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<()> {
    state.db.lock().unwrap().save_settings(&settings)?;
    autostart::sync(&app, settings.autostart_enabled)
}
