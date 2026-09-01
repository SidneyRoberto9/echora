//! System tray icon/menu. Also rewires the main window's close button here,
//! since "closing minimizes to tray" (REQUIREMENTS_FREEZE) only makes sense
//! once a tray exists to minimize to.

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{App, Manager, WindowEvent};

use crate::commands;
use crate::state::AppState;

/// Builds the tray icon + menu, and makes the window's close button hide it
/// instead of quitting — only the tray's own "Quit Echora" item exits the
/// app (REQUIREMENTS_FREEZE: closing the window minimizes to tray).
pub fn setup(app: &App) -> tauri::Result<()> {
    let window = app
        .get_webview_window("main")
        .expect("the main window is declared in tauri.conf.json");

    let close_target = window.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            let _ = close_target.hide();
        }
    });

    let show = MenuItemBuilder::with_id("show", "Show Echora").build(app)?;
    let previous = MenuItemBuilder::with_id("previous", "Previous").build(app)?;
    let play_pause = MenuItemBuilder::with_id("play_pause", "Play / Pause").build(app)?;
    let next = MenuItemBuilder::with_id("next", "Next").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit Echora").build(app)?;
    let menu = MenuBuilder::new(app)
        .items(&[&show, &previous, &play_pause, &next])
        .separator()
        .item(&quit)
        .build()?;

    let mut builder = TrayIconBuilder::new()
        .tooltip("Echora")
        .menu(&menu)
        .on_menu_event(|app, event| {
            let app = app.clone();
            match event.id().0.as_str() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => app.exit(0),
                "play_pause" => {
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        let _ = commands::toggle_play_pause(&state).await;
                    });
                }
                "next" => {
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        let _ = commands::queue::queue_next(app.clone(), state).await;
                    });
                }
                "previous" => {
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<AppState>();
                        let _ = commands::queue::queue_previous(app.clone(), state).await;
                    });
                }
                _ => {}
            }
        });

    // Linux tray backends don't emit click events at all (only the context
    // menu), so there's no separate `on_tray_icon_event` click-to-toggle
    // here — "Show Echora" in the menu is the only, and sufficient, way back.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;

    Ok(())
}
