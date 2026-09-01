use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use crate::db::Db;
use crate::media;
use crate::media::player::Player;
use crate::media::resolver::Resolver;
use crate::moods::MoodCatalog;
use crate::platform::mpris;
use crate::queue::Queue;

/// Rust's source of truth for product state. Each field keeps its own
/// lock so, e.g., a queue read never waits on a database write. `moods` is
/// read-only after startup (bundled data), so it needs no lock at all.
///
/// `player` uses an async-aware `tokio::sync::Mutex` (not `std::sync::Mutex`
/// like the others) because playback commands hold the lock across `.await`
/// points while talking to the mpv sidecar over IPC.
pub struct AppState {
    pub db: Mutex<Db>,
    pub queue: Mutex<Queue>,
    pub moods: MoodCatalog,
    pub resolver: Resolver,
    pub player: tokio::sync::Mutex<Player>,
    /// `None` when the D-Bus session bus wasn't reachable at startup — MPRIS
    /// is a nice-to-have desktop integration, never a startup requirement.
    pub mpris: Option<mpris::Handle>,
    /// Skip-segment data for whatever track is currently playing, kept only
    /// in memory for the current playback session (see ADR 0009) —
    /// populated/cleared by `media::sponsorblock::watch`.
    pub sponsorblock_segments: Mutex<Vec<media::sponsorblock::Segment>>,
    /// The resolved app data directory — stored here so commands (e.g.
    /// crash reporting) can reach it without threading an `AppHandle`
    /// through every call.
    pub app_dir: PathBuf,
    /// Mirrors `Settings.crash_report_enabled`, kept in sync by
    /// `update_settings`. An `Arc` because the panic hook and the mpv
    /// `Player` each need their own clone to check it without touching
    /// `AppState` (the panic hook in particular must not lock anything).
    pub crash_reporting_enabled: Arc<AtomicBool>,
}
