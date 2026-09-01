//! MPRIS (org.mpris.MediaPlayer2) integration (ADR 0005). This is how Linux
//! desktop environments route hardware media keys and lock-screen/"now
//! playing" widgets to Echora — no separate media-keys plugin needed.
//!
//! Implemented against the low-level `Server<T>` API (not the crate's
//! higher-level `Player` builder) because that builder is `Rc`-based and not
//! `Send` — it can't live in Tauri's shared, multi-threaded `AppState`. Our
//! `MprisHandler` instead holds an `AppHandle` and reaches back into the same
//! command functions the frontend calls, so playback logic is never
//! duplicated.

use std::sync::Arc;

use mpris_server::{
    LoopStatus, Metadata, PlaybackRate, PlaybackStatus, PlayerInterface, Property, RootInterface,
    Server, Signal, Time, TrackId, Volume,
    zbus::{self, fdo},
};
use tauri::{AppHandle, Manager};

use crate::commands;
use crate::models::Track;
use crate::state::AppState;

pub type Handle = Arc<Server<MprisHandler>>;

const BUS_NAME_SUFFIX: &str = "echora";

pub struct MprisHandler {
    app: AppHandle,
}

/// Builds and registers the MPRIS D-Bus server. Returns `None` (logging a
/// warning) if the session bus isn't reachable — a missing/misconfigured
/// D-Bus session must not stop Echora from starting (stability over a
/// nice-to-have desktop integration).
pub async fn build(app: AppHandle) -> Option<Handle> {
    match Server::new(BUS_NAME_SUFFIX, MprisHandler { app }).await {
        Ok(server) => Some(Arc::new(server)),
        Err(err) => {
            eprintln!(
                "mpris: session bus unavailable, media keys/lock-screen controls disabled: {err}"
            );
            None
        }
    }
}

/// Recomputes and pushes the properties that can change as a result of a
/// playback/queue mutation, plus a `Seeked` signal — cheap and always safe to
/// emit, and it's what tells desktop "now playing" widgets to resync their
/// position instead of looking frozen. Best-effort: a failure here must never
/// fail the playback command that triggered it.
pub async fn notify(state: &AppState) {
    let Some(server) = state.mpris.as_ref() else {
        return;
    };

    let (track, has_next, has_previous) = {
        let queue = state.queue.lock().unwrap();
        (
            queue.current().cloned(),
            !queue.upcoming().is_empty(),
            queue.position().is_some_and(|p| p > 0),
        )
    };
    let has_track = track.is_some();

    let position = state
        .player
        .lock()
        .await
        .position_seconds()
        .await
        .ok()
        .flatten()
        .map(|secs| Time::from_secs(secs as i64))
        .unwrap_or(Time::ZERO);

    let _ = server
        .properties_changed([
            Property::PlaybackStatus(playback_status_for(state).await),
            Property::Metadata(track.as_ref().map(metadata_for).unwrap_or_default()),
            Property::CanGoNext(has_next),
            Property::CanGoPrevious(has_previous),
            Property::CanPlay(has_track),
            Property::CanPause(has_track),
        ])
        .await;
    let _ = server.emit(Signal::Seeked { position }).await;
}

async fn playback_status_for(state: &AppState) -> PlaybackStatus {
    let mut player = state.player.lock().await;
    if !player.is_started() {
        return PlaybackStatus::Stopped;
    }
    match player.is_paused().await {
        Ok(Some(true)) => PlaybackStatus::Paused,
        Ok(Some(false)) => PlaybackStatus::Playing,
        _ => PlaybackStatus::Stopped,
    }
}

fn metadata_for(track: &Track) -> Metadata {
    let mut builder = Metadata::builder()
        .trackid(track_id_for(&track.id))
        .title(&track.title);
    if let Some(artist) = &track.artist {
        builder = builder.artist([artist.clone()]);
    }
    if let Some(duration) = track.duration_seconds {
        builder = builder.length(Time::from_secs(duration as i64));
    }
    if let Some(art_url) = &track.thumbnail_url {
        builder = builder.art_url(art_url.clone());
    }
    builder.build()
}

/// YouTube video IDs can contain `-`/`_`, which aren't valid D-Bus object
/// path characters — hex-encode to guarantee a valid `TrackId` instead of
/// assuming the external ID is already path-safe.
fn track_id_for(id: &str) -> TrackId {
    let hex: String = id.bytes().map(|b| format!("{b:02x}")).collect();
    TrackId::try_from(format!("/io/github/sidneyroberto9/echora/track/{hex}"))
        .unwrap_or(TrackId::NO_TRACK)
}

impl RootInterface for MprisHandler {
    async fn raise(&self) -> fdo::Result<()> {
        if let Some(window) = self.app.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_focus();
        }
        Ok(())
    }

    async fn quit(&self) -> fdo::Result<()> {
        self.app.exit(0);
        Ok(())
    }

    async fn can_quit(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_fullscreen(&self, _fullscreen: bool) -> zbus::Result<()> {
        Ok(())
    }

    async fn can_set_fullscreen(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn can_raise(&self) -> fdo::Result<bool> {
        Ok(true)
    }

    async fn has_track_list(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn identity(&self) -> fdo::Result<String> {
        Ok("Echora".into())
    }

    async fn desktop_entry(&self) -> fdo::Result<String> {
        Ok(self.app.config().identifier.clone())
    }

    async fn supported_uri_schemes(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn supported_mime_types(&self) -> fdo::Result<Vec<String>> {
        Ok(Vec::new())
    }
}

impl PlayerInterface for MprisHandler {
    // Every action method below fires the effect on a spawned task instead
    // of awaiting it directly. `notify` (transitively, via `emit`/
    // `properties_changed`) awaits zbus's own signal-sending, whose future
    // isn't `Sync` — and this trait's methods are required to return
    // `Send + Sync` futures (`trait_variant::make(Send + Sync)` in
    // mpris-server). A spawned task is a new top-level future the runtime
    // drives independently, so it doesn't carry that bound back to the
    // caller. D-Bus callers don't need to block on the result either way.

    async fn next(&self) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = commands::queue::queue_next(app.clone(), app.state()).await;
        });
        Ok(())
    }

    async fn previous(&self) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = commands::queue::queue_previous(app.clone(), app.state()).await;
        });
        Ok(())
    }

    async fn pause(&self) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _ = state.player.lock().await.set_paused(true).await;
            notify(&state).await;
        });
        Ok(())
    }

    async fn play_pause(&self) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let _ = commands::toggle_play_pause(&app.state::<AppState>()).await;
        });
        Ok(())
    }

    async fn stop(&self) -> fdo::Result<()> {
        // ponytail: Stop maps to Pause — Echora has no separate "stopped and
        // unloaded" playback state. Upgrade to a real mpv unload if an MPRIS
        // client ever needs the distinction.
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _ = state.player.lock().await.set_paused(true).await;
            notify(&state).await;
        });
        Ok(())
    }

    async fn play(&self) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _ = state.player.lock().await.set_paused(false).await;
            notify(&state).await;
        });
        Ok(())
    }

    async fn seek(&self, offset: Time) -> fdo::Result<()> {
        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let mut player = state.player.lock().await;
            let current = player
                .position_seconds()
                .await
                .ok()
                .flatten()
                .unwrap_or(0.0);
            let target = (current + offset.as_micros() as f64 / 1_000_000.0).max(0.0);
            let _ = player.seek_to(target).await;
            drop(player);
            notify(&state).await;
        });
        Ok(())
    }

    async fn set_position(&self, track_id: TrackId, position: Time) -> fdo::Result<()> {
        let current_id = self
            .app
            .state::<AppState>()
            .queue
            .lock()
            .unwrap()
            .current()
            .map(|track| track_id_for(&track.id));
        if current_id != Some(track_id) {
            return Ok(()); // stale request against a track that's no longer current — ignore per spec
        }

        let app = self.app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let seconds = (position.as_micros() as f64 / 1_000_000.0).max(0.0);
            let _ = state.player.lock().await.seek_to(seconds).await;
            notify(&state).await;
        });
        Ok(())
    }

    async fn open_uri(&self, _uri: String) -> fdo::Result<()> {
        Err(fdo::Error::NotSupported(
            "Echora only plays tracks resolved through its own mood/search flow".into(),
        ))
    }

    async fn playback_status(&self) -> fdo::Result<PlaybackStatus> {
        Ok(playback_status_for(&self.app.state::<AppState>()).await)
    }

    async fn loop_status(&self) -> fdo::Result<LoopStatus> {
        Ok(LoopStatus::None)
    }

    async fn set_loop_status(&self, _loop_status: LoopStatus) -> zbus::Result<()> {
        Ok(())
    }

    async fn rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn set_rate(&self, _rate: PlaybackRate) -> zbus::Result<()> {
        Ok(())
    }

    async fn shuffle(&self) -> fdo::Result<bool> {
        Ok(false)
    }

    async fn set_shuffle(&self, _shuffle: bool) -> zbus::Result<()> {
        Ok(())
    }

    async fn metadata(&self) -> fdo::Result<Metadata> {
        let state = self.app.state::<AppState>();
        let current = state.queue.lock().unwrap().current().cloned();
        Ok(current.as_ref().map(metadata_for).unwrap_or_default())
    }

    async fn volume(&self) -> fdo::Result<Volume> {
        let state = self.app.state::<AppState>();
        let mut player = state.player.lock().await;
        if !player.is_started() {
            return Ok(1.0);
        }
        let percent = player.volume_percent().await.ok().flatten().unwrap_or(100);
        Ok(percent as f64 / 100.0)
    }

    async fn set_volume(&self, volume: Volume) -> zbus::Result<()> {
        let state = self.app.state::<AppState>();
        let percent = (volume.max(0.0) * 100.0).round() as u8;
        let _ = state.player.lock().await.set_volume(percent).await;
        Ok(())
    }

    async fn position(&self) -> fdo::Result<Time> {
        let state = self.app.state::<AppState>();
        let secs = state
            .player
            .lock()
            .await
            .position_seconds()
            .await
            .ok()
            .flatten()
            .unwrap_or(0.0);
        Ok(Time::from_secs(secs as i64))
    }

    async fn minimum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn maximum_rate(&self) -> fdo::Result<PlaybackRate> {
        Ok(1.0)
    }

    async fn can_go_next(&self) -> fdo::Result<bool> {
        let state = self.app.state::<AppState>();
        Ok(!state.queue.lock().unwrap().upcoming().is_empty())
    }

    async fn can_go_previous(&self) -> fdo::Result<bool> {
        let state = self.app.state::<AppState>();
        Ok(state
            .queue
            .lock()
            .unwrap()
            .position()
            .is_some_and(|p| p > 0))
    }

    async fn can_play(&self) -> fdo::Result<bool> {
        let state = self.app.state::<AppState>();
        Ok(state.queue.lock().unwrap().current().is_some())
    }

    async fn can_pause(&self) -> fdo::Result<bool> {
        let state = self.app.state::<AppState>();
        Ok(state.queue.lock().unwrap().current().is_some())
    }

    async fn can_seek(&self) -> fdo::Result<bool> {
        let state = self.app.state::<AppState>();
        Ok(state.queue.lock().unwrap().current().is_some())
    }

    async fn can_control(&self) -> fdo::Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_id_for_sanitizes_dashes_and_underscores() {
        // Real YouTube IDs look like this — dashes/underscores aren't valid
        // D-Bus object path characters, so this must not panic or fall back
        // to `NO_TRACK`.
        let id = track_id_for("dQw4w9WgXcQ-_1");
        assert_ne!(id, TrackId::NO_TRACK);
        assert!(
            id.as_str()
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '/')
        );
    }

    #[test]
    fn track_id_for_is_stable_and_distinct() {
        assert_eq!(track_id_for("abc"), track_id_for("abc"));
        assert_ne!(track_id_for("abc"), track_id_for("xyz"));
    }

    #[test]
    fn metadata_for_maps_track_fields() {
        let track = Track {
            id: "abc-123".into(),
            title: "A Song".into(),
            artist: Some("An Artist".into()),
            duration_seconds: Some(180),
            thumbnail_url: Some("https://example.com/thumb.jpg".into()),
        };

        let metadata = metadata_for(&track);

        assert_eq!(metadata.trackid(), Some(track_id_for("abc-123")));
        assert_eq!(metadata.title(), Some("A Song"));
        assert_eq!(metadata.artist(), Some(vec!["An Artist".to_string()]));
        assert_eq!(metadata.length(), Some(Time::from_secs(180)));
        assert_eq!(
            metadata.art_url(),
            Some("https://example.com/thumb.jpg".to_string())
        );
    }

    #[test]
    fn metadata_for_omits_absent_optional_fields() {
        let track = Track {
            id: "abc".into(),
            title: "A Song".into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        };

        let metadata = metadata_for(&track);

        assert_eq!(metadata.artist(), None);
        assert_eq!(metadata.length(), None);
        assert_eq!(metadata.art_url(), None);
    }
}
