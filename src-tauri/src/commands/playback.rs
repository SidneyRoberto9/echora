use tauri::State;

use crate::error::Result;
use crate::state::AppState;

/// Resolves the track to a playable stream and hands it to the mpv
/// sidecar, starting mpv on first use (not at app launch — no point
/// paying for the process before there's anything to play).
#[tauri::command]
pub async fn play_track(state: State<'_, AppState>, track_id: String) -> Result<()> {
    let resolved = state.resolver.resolve_with_retry(&track_id).await?;
    let mut player = state.player.lock().await;
    if !player.is_started() {
        player.start().await?;
    }
    player.load(&resolved.stream_url).await?;
    Ok(())
}

#[tauri::command]
pub async fn pause_playback(state: State<'_, AppState>) -> Result<()> {
    state.player.lock().await.set_paused(true).await
}

#[tauri::command]
pub async fn resume_playback(state: State<'_, AppState>) -> Result<()> {
    state.player.lock().await.set_paused(false).await
}

#[tauri::command]
pub async fn seek_playback(state: State<'_, AppState>, seconds: f64) -> Result<()> {
    state.player.lock().await.seek_to(seconds).await
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
