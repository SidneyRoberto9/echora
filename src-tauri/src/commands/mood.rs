use tauri::State;

use crate::error::Result;
use crate::models::{MoodSummary, SessionInfo};
use crate::mood_engine::surprise;
use crate::state::AppState;

#[tauri::command]
pub fn list_moods(state: State<AppState>) -> Vec<MoodSummary> {
    state.moods.list()
}

/// Picks a mood (favorited moods more likely, recently-played moods less
/// likely — see `mood_engine::surprise`) and starts a session for it.
#[tauri::command]
pub async fn surprise_me(state: State<'_, AppState>) -> Result<SessionInfo> {
    let moods = state.moods.list();
    let mood_id = {
        let db = state.db.lock().unwrap();
        let favorited = db.list_favorite_moods()?.into_iter().collect();
        let recently_played = db.recent_mood_ids(5)?;
        let mut rng = rand::rng();
        let picked = surprise::pick_surprise_mood(&moods, &favorited, &recently_played, &mut rng)?;
        picked.id.clone()
    };

    let session = super::session::start_session_impl(&state, &mood_id)?;
    super::top_up_queue(&state, &mood_id).await?;
    Ok(session)
}
