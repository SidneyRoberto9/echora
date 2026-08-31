use tauri::State;

use crate::error::Result;
use crate::models::Track;
use crate::state::AppState;

#[tauri::command]
pub async fn search_tracks(
    state: State<'_, AppState>,
    query: String,
    limit: u32,
) -> Result<Vec<Track>> {
    state.resolver.search(&query, limit).await
}
