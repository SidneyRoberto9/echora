mod commands;
mod db;
mod error;
mod media;
mod models;
mod mood_engine;
mod moods;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
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

            let sidecar_paths = media::sidecar_paths::SidecarPaths::discover_dev();
            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                yt_dlp_path: sidecar_paths.yt_dlp,
                deno_path: sidecar_paths.deno,
                timeout: std::time::Duration::from_secs(30),
            });
            let player =
                media::player::Player::new(sidecar_paths.mpv, app_dir.join("mpv-ipc.sock"));

            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                player: tokio::sync::Mutex::new(player),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::mood::list_moods,
            commands::mood::surprise_me,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::session::start_session,
            commands::session::start_mood_session,
            commands::session::end_session,
            commands::session::get_current_session,
            commands::session::list_history,
            commands::session::clear_history,
            commands::queue::get_queue,
            commands::queue::queue_next,
            commands::queue::queue_previous,
            commands::queue::queue_skip_to,
            commands::queue::queue_remove,
            commands::queue::ensure_queue_topped_up,
            commands::library::favorite_track,
            commands::library::unfavorite_track,
            commands::library::is_track_favorited,
            commands::library::favorite_mood,
            commands::library::unfavorite_mood,
            commands::library::list_favorite_moods,
            commands::library::set_track_feedback,
            commands::library::get_track_feedback,
            commands::search::search_tracks,
            commands::playback::play_track,
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
