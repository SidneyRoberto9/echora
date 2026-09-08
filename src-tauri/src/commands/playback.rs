use tauri::{Emitter, State};

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

/// `persist` is `false` for every tick of a volume slider drag and `true`
/// only for the trailing call once the user stops moving it (debounced on
/// the frontend, see `usePlayback.setVolume` — P1-4). The live mpv volume
/// always applies immediately either way; only the SQLite write is
/// conditional, so a drag doesn't turn into ~100 writes.
#[tauri::command]
pub async fn set_playback_volume(
    state: State<'_, AppState>,
    volume: u8,
    persist: bool,
) -> Result<()> {
    set_playback_volume_impl(&state, volume, persist).await
}

pub(crate) async fn set_playback_volume_impl(
    state: &AppState,
    volume: u8,
    persist: bool,
) -> Result<()> {
    if persist {
        // Single lock acquisition for the read-modify-write -- two
        // separate `db.lock()` calls here would let a concurrent settings
        // write interleave between the read and the write and get
        // clobbered, the same class of bug already fixed in
        // `record_current_completion`.
        let db = state.db.lock().unwrap();
        let mut settings = db.get_settings()?;
        settings.volume = volume;
        db.save_settings(&settings)?;
    }
    let result = state.player.lock().await.set_volume(volume).await;
    if let Some(app) = crate::platform::mpris::APP_HANDLE.get() {
        let _ = app.emit("volume-changed", volume);
    }
    result
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
            prefetch: crate::media::prefetch::Prefetch::new(),
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
    async fn set_playback_volume_does_not_panic_without_an_app_handle() {
        // In this test binary no real Tauri app is ever built, so
        // `platform::mpris::APP_HANDLE` is never `.set()`. This just proves
        // `set_playback_volume_impl` doesn't panic when the handle is absent
        // (the `if let Some(app) = ...` guard is skipped entirely), matching
        // how `mpris::notify()` already degrades. It still returns `Err` here
        // because `test_state()`'s `Player` is never started — same reason
        // the sibling test `set_playback_volume_persists_even_when_the_live_apply_fails`
        // below asserts `is_err()`, not because of anything to do with the
        // event emit.
        let state = test_state();
        let result = set_playback_volume_impl(&state, 55, false).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn set_playback_volume_persists_even_when_the_live_apply_fails() {
        let state = test_state();
        // The player isn't started -- no mpv socket to connect to -- so the
        // live apply fails. Volume should still be remembered for the next
        // session, matching a preference the user set independent of
        // whatever happens to be playing right now.
        let result = set_playback_volume_impl(&state, 42, true).await;

        assert!(result.is_err());
        assert_eq!(state.db.lock().unwrap().get_settings().unwrap().volume, 42);
    }

    #[tokio::test]
    async fn set_playback_volume_does_not_persist_when_not_asked_to() {
        // The debounced path (P1-4): every tick of a slider drag calls this
        // with `persist: false` so the live mpv volume still applies
        // immediately, without writing to SQLite on every single tick.
        let state = test_state();
        let before = state.db.lock().unwrap().get_settings().unwrap().volume;

        let _ = set_playback_volume_impl(&state, before.wrapping_add(1), false).await;

        assert_eq!(
            state.db.lock().unwrap().get_settings().unwrap().volume,
            before
        );
    }
}
