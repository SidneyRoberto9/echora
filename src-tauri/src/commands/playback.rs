use tauri::State;

use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub async fn pause_playback(state: State<'_, AppState>) -> Result<()> {
    state.player.lock().await.set_paused(true).await?;
    crate::platform::mpris::notify(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn resume_playback(state: State<'_, AppState>) -> Result<()> {
    state.player.lock().await.set_paused(false).await?;
    crate::platform::mpris::notify(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn seek_playback(state: State<'_, AppState>, seconds: f64) -> Result<()> {
    state.player.lock().await.seek_to(seconds).await?;
    crate::platform::mpris::notify(&state).await;
    Ok(())
}

#[tauri::command]
pub async fn set_playback_volume(state: State<'_, AppState>, volume: u8) -> Result<()> {
    state.player.lock().await.set_volume(volume).await
}

#[tauri::command]
pub async fn get_playback_position(state: State<'_, AppState>) -> Result<Option<f64>> {
    state.player.lock().await.position_seconds().await
}

#[tauri::command]
pub async fn get_playback_duration(state: State<'_, AppState>) -> Result<Option<f64>> {
    state.player.lock().await.duration_seconds().await
}
