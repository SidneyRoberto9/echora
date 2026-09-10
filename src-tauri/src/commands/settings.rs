use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::error::Result;
use crate::models::Settings;
use crate::platform::autostart;
use crate::state::AppState;

/// SponsorBlock categories Echora actually knows how to skip. Anything
/// else arriving in a patch is untrusted frontend input and gets dropped
/// rather than persisted (CLAUDE.md: validate external/frontend input
/// before persistence) -- mirrors `SPONSORBLOCK_CATEGORIES` in
/// `SettingsView.tsx`, but the backend is the one that actually enforces
/// it.
const SPONSORBLOCK_CATEGORY_ALLOWLIST: &[&str] = &["sponsor", "selfpromo", "intro", "outro"];

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings> {
    state.db.lock().unwrap().get_settings()
}

/// Every field optional -- only what's actually sent gets changed. Needs
/// `#[serde(default)]`: without it, serde treats a JSON object that's
/// missing a key as a deserialization error even for an `Option<T>`
/// field, which would defeat the entire point of a *partial* patch.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct SettingsPatch {
    pub history_enabled: Option<bool>,
    pub crash_report_enabled: Option<bool>,
    pub autostart_enabled: Option<bool>,
    pub sponsorblock_categories: Option<Vec<String>>,
    pub volume: Option<u8>,
    pub discord_presence_enabled: Option<bool>,
}

/// Merges `patch` onto `current`, validating as it merges: `volume` is
/// clamped to the 0-100 range the player/UI actually use, and SponsorBlock
/// categories are filtered against the allowlist instead of trusted
/// verbatim. Pure and DB-free on purpose -- this is the one part of the
/// patch flow with actual logic in it, so it's the one part worth unit
/// testing directly.
fn apply_patch(current: Settings, patch: SettingsPatch) -> Settings {
    let mut next = current;
    if let Some(volume) = patch.volume {
        next.volume = volume.min(100);
    }
    if let Some(categories) = patch.sponsorblock_categories {
        next.sponsorblock_categories = categories
            .into_iter()
            .filter(|c| SPONSORBLOCK_CATEGORY_ALLOWLIST.contains(&c.as_str()))
            .collect();
    }
    if let Some(v) = patch.history_enabled {
        next.history_enabled = v;
    }
    if let Some(v) = patch.crash_report_enabled {
        next.crash_report_enabled = v;
    }
    if let Some(v) = patch.autostart_enabled {
        next.autostart_enabled = v;
    }
    if let Some(v) = patch.discord_presence_enabled {
        next.discord_presence_enabled = v;
    }
    next
}

/// Partial settings update: patches only the fields the frontend actually
/// sends, merged onto the settings row as it is *right now* rather than
/// whatever the frontend happened to load at mount time. Fixes the
/// lost-update bug a full-object replace had (P1-3: change the volume via
/// the player slider, then flip an unrelated toggle in Settings, and the
/// volume used to revert to whatever Settings loaded with). Read, merge,
/// and write happen under one `db` lock acquisition, so this also can't
/// race a second concurrent patch. Returns the merged `Settings` so the
/// frontend can adopt it directly instead of re-deriving its own copy of
/// the merge.
#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<Settings> {
    // Scoped rather than `drop`ped: this is an async command now, and a
    // `std::sync::MutexGuard` merely *reachable* at an await point makes
    // the whole future non-`Send`, which Tauri's handler rejects.
    let next = {
        let db = state.db.lock().unwrap();
        let next = apply_patch(db.get_settings()?, patch);
        db.save_settings(&next)?;
        next
    };

    state.crash_reporting_enabled.store(
        next.crash_report_enabled,
        std::sync::atomic::Ordering::Relaxed,
    );
    autostart::sync(&app, next.autostart_enabled)?;
    if let Some(handle) = state.discord.as_ref() {
        crate::platform::discord::set_enabled(handle, next.discord_presence_enabled);
        // `notify_from_state` is inert while the feature is off, so the
        // presence channel still holds whatever was true when it was last
        // on (usually nothing). Push the live state once at the moment
        // it's switched on, instead of showing nothing until the next
        // playback event happens to fire.
        if next.discord_presence_enabled {
            crate::platform::discord::notify_from_state(handle, &state).await;
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_only_changes_fields_that_were_sent() {
        let current = Settings {
            volume: 42,
            history_enabled: false,
            ..Settings::default()
        };

        let next = apply_patch(
            current,
            SettingsPatch {
                crash_report_enabled: Some(true),
                ..Default::default()
            },
        );

        assert_eq!(next.volume, 42, "volume untouched by an unrelated patch");
        assert!(!next.history_enabled, "history_enabled untouched");
        assert!(next.crash_report_enabled, "the field that was sent changed");
    }

    #[test]
    fn volume_above_100_is_clamped_not_rejected() {
        let next = apply_patch(
            Settings::default(),
            SettingsPatch {
                volume: Some(255),
                ..Default::default()
            },
        );
        assert_eq!(next.volume, 100);
    }

    /// Exercises the actual wire format the frontend sends -- a JSON
    /// object with only some of `SettingsPatch`'s keys -- rather than a
    /// `SettingsPatch` built directly in Rust, which wouldn't catch a
    /// missing `#[serde(default)]`.
    #[test]
    fn partial_json_deserializes_with_other_fields_as_none() {
        let patch: SettingsPatch = serde_json::from_str(r#"{"volume":77}"#).unwrap();
        assert_eq!(patch.volume, Some(77));
        assert_eq!(patch.history_enabled, None);
        assert_eq!(patch.sponsorblock_categories, None);
    }

    #[test]
    fn unknown_sponsorblock_category_is_dropped_not_persisted() {
        let next = apply_patch(
            Settings::default(),
            SettingsPatch {
                sponsorblock_categories: Some(vec!["sponsor".into(), "evil_script".into()]),
                ..Default::default()
            },
        );
        assert_eq!(next.sponsorblock_categories, vec!["sponsor".to_string()]);
    }

    #[test]
    fn patch_can_enable_discord_presence() {
        let next = apply_patch(
            Settings::default(),
            SettingsPatch {
                discord_presence_enabled: Some(true),
                ..Default::default()
            },
        );
        assert!(next.discord_presence_enabled);
    }
}
