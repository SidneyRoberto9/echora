use tauri::State;

use crate::models::MoodSummary;
use crate::state::AppState;

#[tauri::command]
pub fn list_moods(state: State<AppState>) -> Vec<MoodSummary> {
    state.moods.list()
}
