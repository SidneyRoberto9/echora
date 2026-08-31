//! Real OS-level autostart registration (`tauri-plugin-autostart`), backing
//! the `autostart_enabled` field that `Settings` already persisted before
//! this existed.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::error::{EchoraError, Result};

/// Makes the OS-level autostart entry match `enabled`. Called both at
/// startup (in case the saved setting and the actual OS entry drifted — a
/// manual removal, a fresh profile) and whenever the setting is changed.
/// Both directions are idempotent on Linux, so this is safe to call
/// unconditionally rather than only on an actual change.
pub fn sync(app: &AppHandle, enabled: bool) -> Result<()> {
    let autolaunch = app.autolaunch();
    let result = if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    };
    result.map_err(|err| EchoraError::Autostart(err.to_string()))
}
