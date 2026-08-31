use std::sync::Mutex;

use crate::db::Db;
use crate::moods::MoodCatalog;
use crate::queue::Queue;

/// Rust's source of truth for product state. One struct, but each field
/// keeps its own lock so, e.g., a queue read never waits on a database
/// write. `moods` is read-only after startup (bundled data), so it needs
/// no lock at all.
pub struct AppState {
    pub db: Mutex<Db>,
    pub queue: Mutex<Queue>,
    pub moods: MoodCatalog,
}
