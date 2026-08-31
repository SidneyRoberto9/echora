use tauri::State;

use crate::error::{EchoraError, Result};
use crate::models::{SessionInfo, SessionSummary};
use crate::state::AppState;

/// Plain function (not `#[tauri::command]`) so it's testable without a real
/// Tauri `App` — the command below is a one-line wrapper around it.
pub(crate) fn start_session_impl(state: &AppState, mood_id: &str) -> Result<SessionInfo> {
    state.moods.get(mood_id)?;
    let session = state.db.lock().unwrap().start_session(mood_id)?;
    state.queue.lock().unwrap().clear();
    Ok(session)
}

pub(crate) fn end_session_impl(state: &AppState) -> Result<()> {
    let current = state.db.lock().unwrap().current_session()?;
    let session = current.ok_or(EchoraError::NoActiveSession)?;
    state.db.lock().unwrap().end_session(session.id)?;
    state.queue.lock().unwrap().clear();
    Ok(())
}

#[tauri::command]
pub fn start_session(state: State<AppState>, mood_id: String) -> Result<SessionInfo> {
    start_session_impl(&state, &mood_id)
}

/// The real "choose a mood, get music" entry point: creates the session,
/// fetches its first batch of candidates, and starts playing the first
/// one — `start_session` alone only does the first of those three.
#[tauri::command]
pub async fn start_mood_session(
    state: State<'_, AppState>,
    mood_id: String,
) -> Result<SessionInfo> {
    super::start_session_and_play(&state, &mood_id).await
}

#[tauri::command]
pub fn end_session(state: State<AppState>) -> Result<()> {
    end_session_impl(&state)
}

#[tauri::command]
pub fn get_current_session(state: State<AppState>) -> Result<Option<SessionInfo>> {
    state.db.lock().unwrap().current_session()
}

#[tauri::command]
pub fn list_history(
    state: State<AppState>,
    limit: i64,
    offset: i64,
) -> Result<Vec<SessionSummary>> {
    state.db.lock().unwrap().list_sessions(limit, offset)
}

#[tauri::command]
pub fn clear_history(state: State<AppState>) -> Result<()> {
    state.db.lock().unwrap().clear_history()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::models::Track;
    use crate::moods::MoodCatalog;
    use crate::queue::Queue;
    use std::sync::Mutex;

    fn test_state() -> AppState {
        use crate::media::player::Player;
        use crate::media::resolver::{Resolver, ResolverConfig};
        use std::path::PathBuf;
        use std::time::Duration;

        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                yt_dlp_path: PathBuf::from("yt-dlp"),
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-unused.sock"),
            )),
        }
    }

    #[test]
    fn starting_a_session_with_an_unknown_mood_errors() {
        let state = test_state();
        let err = start_session_impl(&state, "not-a-real-mood").unwrap_err();
        assert!(matches!(err, EchoraError::UnknownMood(_)));
    }

    #[test]
    fn starting_a_session_with_a_known_mood_clears_the_queue() {
        let state = test_state();
        let mood_id = state.moods.list()[0].id.clone();
        state.queue.lock().unwrap().add_candidates([Track {
            id: "leftover".into(),
            title: "leftover".into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }]);

        let session = start_session_impl(&state, &mood_id).unwrap();

        assert_eq!(session.mood_id, mood_id);
        assert!(state.queue.lock().unwrap().current().is_none());
    }

    #[test]
    fn ending_a_session_with_none_active_errors() {
        let state = test_state();
        let err = end_session_impl(&state).unwrap_err();
        assert!(matches!(err, EchoraError::NoActiveSession));
    }

    #[test]
    fn ending_the_active_session_clears_the_queue() {
        let state = test_state();
        let mood_id = state.moods.list()[0].id.clone();
        start_session_impl(&state, &mood_id).unwrap();
        state.queue.lock().unwrap().add_candidates([Track {
            id: "a".into(),
            title: "a".into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }]);

        end_session_impl(&state).unwrap();

        assert!(state.queue.lock().unwrap().current().is_none());
        assert!(
            state
                .db
                .lock()
                .unwrap()
                .current_session()
                .unwrap()
                .is_none()
        );
    }
}
