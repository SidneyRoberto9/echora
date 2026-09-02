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
    set_playback_volume_impl(&state, volume).await
}

pub(crate) async fn set_playback_volume_impl(state: &AppState, volume: u8) -> Result<()> {
    let mut settings = state.db.lock().unwrap().get_settings()?;
    settings.volume = volume;
    state.db.lock().unwrap().save_settings(&settings)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::media::player::Player;
    use crate::media::resolver::{Resolver, ResolverConfig};
    use crate::moods::MoodCatalog;
    use crate::queue::Queue;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::Duration;

    fn test_state() -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("/tmp/echora-test-playback-unused.sock"),
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    #[tokio::test]
    async fn set_playback_volume_persists_even_when_the_live_apply_fails() {
        let state = test_state();
        // The player isn't started -- no mpv socket to connect to -- so the
        // live apply fails. Volume should still be remembered for the next
        // session, matching a preference the user set independent of
        // whatever happens to be playing right now.
        let result = set_playback_volume_impl(&state, 42).await;

        assert!(result.is_err());
        assert_eq!(state.db.lock().unwrap().get_settings().unwrap().volume, 42);
    }
}
