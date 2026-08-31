mod commands;
mod db;
mod error;
mod models;
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

            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::mood::list_moods,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::session::start_session,
            commands::session::end_session,
            commands::session::get_current_session,
            commands::session::list_history,
            commands::queue::get_queue,
            commands::queue::queue_next,
            commands::queue::queue_previous,
            commands::queue::queue_skip_to,
            commands::queue::queue_remove,
            commands::library::favorite_track,
            commands::library::unfavorite_track,
            commands::library::is_track_favorited,
            commands::library::favorite_mood,
            commands::library::unfavorite_mood,
            commands::library::list_favorite_moods,
            commands::library::set_track_feedback,
            commands::library::get_track_feedback,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
