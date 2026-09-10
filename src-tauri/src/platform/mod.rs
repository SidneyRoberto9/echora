pub mod autostart;
pub mod discord;
pub mod mpris;
pub mod tray;

use crate::state::AppState;

/// Fans a playback/queue change out to every desktop integration that
/// needs to know about it -- the single point every playback-changing
/// command calls, instead of reaching into `mpris`/`discord`
/// individually. See docs/superpowers/specs/
/// 2026-09-09-discord-rich-presence-design.md.
pub async fn notify_playback_changed(state: &AppState) {
    mpris::notify(state).await;
    if let Some(handle) = state.discord.as_ref() {
        discord::notify_from_state(handle, state).await;
    }
}
