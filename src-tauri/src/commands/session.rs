use tauri::State;

use crate::error::{EchoraError, Result};
use crate::models::{
    CategoryBreakdown, ListeningStats, MoodPlayCount, SessionInfo, SessionMood, SessionSummary,
};
use crate::state::AppState;

/// Plain function (not `#[tauri::command]`) so it's testable without a real
/// Tauri `App` — the commands below are thin wrappers around it.
pub(crate) fn start_session_impl(state: &AppState, moods: &[(String, u8)]) -> Result<SessionInfo> {
    for (mood_id, _) in moods {
        state.moods.get(mood_id)?;
    }
    let session = state.db.lock().unwrap().start_session(moods)?;
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
    start_session_impl(&state, &[(mood_id, 100)])
}

/// The real "choose a mood, get music" entry point: creates the session,
/// fetches its first batch of candidates, and starts playing the first
/// one — `start_session` alone only does the first of those three.
#[tauri::command]
pub async fn start_mood_session(
    state: State<'_, AppState>,
    mood_id: String,
) -> Result<SessionInfo> {
    super::start_session_and_play(&state, &[(mood_id, 100)]).await
}

/// Starts a session for a weighted mix of 1-3 moods (see
/// `Db::start_session` for the validation rules) — the mood-mixing
/// counterpart to `start_mood_session`.
#[tauri::command]
pub async fn start_mixed_session(
    state: State<'_, AppState>,
    moods: Vec<SessionMood>,
) -> Result<SessionInfo> {
    let pairs: Vec<(String, u8)> = moods.into_iter().map(|m| (m.mood_id, m.weight)).collect();
    super::start_session_and_play(&state, &pairs).await
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

#[tauri::command]
pub fn list_most_played_moods(state: State<AppState>) -> Result<Vec<MoodPlayCount>> {
    state.db.lock().unwrap().mood_play_counts()
}

/// Plain function (not `#[tauri::command]`) so the category-grouping logic
/// is testable without a real Tauri `App` — mirrors `start_session_impl`.
/// `Db::listening_stats` can't do this grouping itself: it never
/// references `MoodCatalog` (moods are bundled static data, not a DB
/// concern — see ADR 0008), so this command layer maps each `mood_id` in
/// the ranking to its category, silently skipping any id no longer in the
/// loaded catalog (a mood renamed/removed across an app update) rather
/// than failing the whole stats view over one stale id.
pub(crate) fn build_listening_stats(state: &AppState) -> Result<ListeningStats> {
    let mut stats = state.db.lock().unwrap().listening_stats()?;
    let ranking = state.db.lock().unwrap().mood_play_counts()?;

    let mut by_category: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for entry in ranking {
        if let Ok(mood) = state.moods.get(&entry.mood_id) {
            *by_category.entry(mood.category.clone()).or_insert(0.0) += entry.play_count;
        }
    }
    let mut breakdown: Vec<CategoryBreakdown> = by_category
        .into_iter()
        .map(|(category, session_count)| CategoryBreakdown {
            category,
            session_count,
        })
        .collect();
    breakdown.sort_by(|a, b| b.session_count.partial_cmp(&a.session_count).unwrap());

    stats.category_breakdown = breakdown;
    Ok(stats)
}

#[tauri::command]
pub fn get_listening_stats(state: State<AppState>) -> Result<ListeningStats> {
    build_listening_stats(&state)
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
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    #[test]
    fn starting_a_session_with_an_unknown_mood_errors() {
        let state = test_state();
        let err = start_session_impl(&state, &[("not-a-real-mood".to_string(), 100)]).unwrap_err();
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

        let session = start_session_impl(&state, &[(mood_id.clone(), 100)]).unwrap();

        assert_eq!(session.moods.len(), 1);
        assert_eq!(session.moods[0].mood_id, mood_id);
        assert!(state.queue.lock().unwrap().current().is_none());
    }

    #[test]
    fn ending_a_session_with_none_active_errors() {
        let state = test_state();
        let err = end_session_impl(&state).unwrap_err();
        assert!(matches!(err, EchoraError::NoActiveSession));
    }

    #[test]
    fn build_listening_stats_groups_play_counts_by_category_skipping_unknown_moods() {
        let state = test_state();
        let mood = state.moods.list()[0].clone();

        state
            .db
            .lock()
            .unwrap()
            .start_session(&[(mood.id.clone(), 100)])
            .unwrap();
        state
            .db
            .lock()
            .unwrap()
            .start_session(&[("not-a-real-mood".to_string(), 100)])
            .unwrap();

        let stats = build_listening_stats(&state).unwrap();

        assert_eq!(stats.total_sessions, 2);
        assert_eq!(stats.category_breakdown.len(), 1);
        assert_eq!(stats.category_breakdown[0].category, mood.category);
        assert_eq!(stats.category_breakdown[0].session_count, 1.0);
    }

    #[test]
    fn ending_the_active_session_clears_the_queue() {
        let state = test_state();
        let mood_id = state.moods.list()[0].id.clone();
        start_session_impl(&state, &[(mood_id, 100)]).unwrap();
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
