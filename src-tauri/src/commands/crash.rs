use tauri::State;

use crate::crash::{self, CrashSummary};
use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub fn list_crash_reports(state: State<AppState>) -> Result<Vec<CrashSummary>> {
    crash::list(&state.app_dir).map_err(crate::error::EchoraError::Io)
}

#[tauri::command]
pub fn get_crash_report_markdown(state: State<AppState>, id: String) -> Result<String> {
    crash::to_markdown(&state.app_dir, &id)
}

#[tauri::command]
pub fn clear_crash_reports(state: State<AppState>) -> Result<()> {
    crash::clear(&state.app_dir).map_err(crate::error::EchoraError::Io)
}

pub(crate) fn report_frontend_crash_impl(state: &AppState, message: String, stack: Option<String>) {
    if state
        .crash_reporting_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        let _ = crash::record(
            &state.app_dir,
            crash::CrashRecord::from_frontend(message, stack),
        );
    }
}

#[tauri::command]
pub fn report_frontend_crash(
    state: State<AppState>,
    message: String,
    stack: Option<String>,
) -> Result<()> {
    report_frontend_crash_impl(&state, message, stack);
    Ok(())
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
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("echora-crash-cmd-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn test_state(app_dir: PathBuf, crash_reporting_enabled: bool) -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("/tmp/echora-test-crash-cmd-unused.sock"),
                app_dir.clone(),
                Arc::new(AtomicBool::new(false)),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir,
            crash_reporting_enabled: Arc::new(AtomicBool::new(crash_reporting_enabled)),
        }
    }

    #[test]
    fn report_frontend_crash_is_a_no_op_when_disabled() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), false);

        report_frontend_crash_impl(&state, "should not persist".into(), None);

        assert!(crash::list(&dir).unwrap().is_empty());
    }

    #[test]
    fn report_frontend_crash_persists_when_enabled() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), true);

        report_frontend_crash_impl(&state, "persisted".into(), Some("stack trace".into()));

        let summaries = crash::list(&dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message, "persisted");
    }

    #[test]
    fn list_and_clear_round_trip_through_the_command_functions() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), true);
        report_frontend_crash_impl(&state, "one".into(), None);
        std::thread::sleep(std::time::Duration::from_millis(2));
        report_frontend_crash_impl(&state, "two".into(), None);

        let listed = crash::list(&state.app_dir).unwrap();
        assert_eq!(listed.len(), 2);

        crash::clear(&state.app_dir).unwrap();
        assert!(crash::list(&state.app_dir).unwrap().is_empty());
    }
}
