mod commands;
mod crash;
mod db;
mod error;
mod licenses;
mod media;
mod models;
mod mood_engine;
mod moods;
mod platform;
mod queue;
mod state;

use std::sync::Mutex;

use tauri::Manager;

use state::AppState;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

/// Whether this process is running from an AppImage — set by the
/// AppImage runtime itself (`APPIMAGE` env var points at the mounted
/// image). Only the AppImage build supports in-place self-update; `.deb`
/// installs get a static "check the releases page" note in Settings
/// instead (see docs/superpowers/specs/2026-09-01-auto-update-design.md).
#[tauri::command]
fn is_appimage_build() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}

#[tauri::command]
fn get_third_party_licenses() -> Vec<licenses::LicenseEntry> {
    licenses::all()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir should resolve");
            std::fs::create_dir_all(&app_dir).expect("should be able to create the app data dir");
            let db_path = app_dir.join("echora.sqlite");
            let db = db::Db::open(
                db_path
                    .to_str()
                    .expect("app data path should be valid utf8"),
            )
            .expect("database should open and migrate");
            let moods = moods::MoodCatalog::load().expect("bundled moods.json should load");

            let initial_settings = db.get_settings()?;
            let crash_reporting_enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                initial_settings.crash_report_enabled,
            ));

            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                deno_path: media::sidecar_paths::resolve_deno_path(),
                timeout: std::time::Duration::from_secs(30),
            });
            let player = media::player::Player::new(
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );

            let mpris =
                tauri::async_runtime::block_on(platform::mpris::build(app.handle().clone()));

            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                prefetch: media::prefetch::Prefetch::new(),
                player: tokio::sync::Mutex::new(player),
                mpris,
                sponsorblock_segments: Mutex::new(Vec::new()),
                app_dir: app_dir.clone(),
                crash_reporting_enabled: crash_reporting_enabled.clone(),
            });

            platform::tray::setup(app)?;

            // Keep the OS-level autostart entry truthful to the saved
            // setting even if it drifted (manually removed, fresh profile).
            // Reuses `initial_settings` read above instead of a second
            // `get_settings()` query.
            platform::autostart::sync(app.handle(), initial_settings.autostart_enabled)?;

            // Chain onto Rust's default panic hook (keeps stderr output
            // for `cargo tauri dev`) and additionally persist a crash
            // record — best-effort, must never itself panic.
            crash::install_panic_hook(app_dir.clone(), crash_reporting_enabled.clone());

            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            is_appimage_build,
            get_third_party_licenses,
            commands::mood::list_moods,
            commands::mood::surprise_me,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::crash::list_crash_reports,
            commands::crash::get_crash_report_markdown,
            commands::crash::clear_crash_reports,
            commands::crash::report_frontend_crash,
            commands::session::start_session,
            commands::session::start_mood_session,
            commands::session::start_mixed_session,
            commands::session::end_session,
            commands::session::get_current_session,
            commands::session::list_history,
            commands::session::clear_history,
            commands::session::list_most_played_moods,
            commands::session::get_listening_stats,
            commands::queue::get_queue,
            commands::queue::queue_next,
            commands::queue::queue_previous,
            commands::queue::queue_skip_to,
            commands::queue::queue_remove,
            commands::queue::ensure_queue_topped_up,
            commands::queue::play_single_track,
            commands::queue::save_scene,
            commands::queue::list_scenes,
            commands::queue::play_scene,
            commands::queue::rename_scene,
            commands::queue::delete_scene,
            commands::library::favorite_track,
            commands::library::unfavorite_track,
            commands::library::is_track_favorited,
            commands::library::favorite_mood,
            commands::library::unfavorite_mood,
            commands::library::list_favorite_moods,
            commands::library::list_favorite_tracks,
            commands::library::set_track_feedback,
            commands::library::get_track_feedback,
            commands::playback::pause_playback,
            commands::playback::resume_playback,
            commands::playback::seek_playback,
            commands::playback::set_playback_volume,
            commands::playback::get_playback_position,
            commands::playback::get_playback_duration,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // No orphaned mpv process on quit: this runs once, right before
            // the process actually exits, regardless of which window/tray
            // action triggered it.
            if let tauri::RunEvent::Exit = event
                && let Some(state) = app_handle.try_state::<AppState>()
            {
                tauri::async_runtime::block_on(async {
                    let _ = state.player.lock().await.shutdown().await;
                });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_appimage_build_reflects_the_appimage_env_var() {
        // SAFETY: this test mutates process-global env state
        // (`APPIMAGE`); no other test in this crate reads or writes that
        // variable, and Rust's test harness doesn't run this specific
        // test concurrently with itself, so there's no cross-test race —
        // but do not add another test touching `APPIMAGE` without
        // giving both a `#[serial]`-style guard or merging them into one.
        unsafe {
            std::env::remove_var("APPIMAGE");
        }
        assert!(!is_appimage_build());

        unsafe {
            std::env::set_var("APPIMAGE", "/tmp/echora.AppImage");
        }
        assert!(is_appimage_build());

        unsafe {
            std::env::remove_var("APPIMAGE");
        }
    }
}
