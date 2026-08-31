use std::sync::Mutex;

use crate::db::Db;
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
}
