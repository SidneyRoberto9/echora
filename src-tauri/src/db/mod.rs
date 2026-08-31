mod library;
mod scenes;
mod scoring_signals;
mod sessions;
mod settings;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

use crate::error::Result;

pub struct Db {
    pub(crate) conn: Connection,
}

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../../migrations/0001_init.sql")),
        M::up(include_str!("../../migrations/0002_scenes.sql")),
    ])
}

/// Seconds since the Unix epoch — every timestamp column in this schema
/// uses this, kept as a plain function so tests can't drift from
/// production behavior via a different clock source.
pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_secs() as i64
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        migrations().to_latest(&mut conn)?;
        Ok(Db { conn })
    }

    /// Test-only: an ephemeral database for unit tests, so they never touch
    /// disk or share state with each other.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        conn.execute("PRAGMA foreign_keys = ON", [])?;
        migrations().to_latest(&mut conn)?;
        Ok(Db { conn })
    }

    /// Inserts the track if unseen, otherwise just bumps `last_seen_at` and
    /// refreshes the metadata (titles/thumbnails can change on refetch).
    pub(crate) fn upsert_track(&self, track: &crate::models::Track) -> Result<()> {
        let now = now();
        self.conn.execute(
            "INSERT INTO tracks (id, title, artist, duration_seconds, thumbnail_url, first_seen_at, last_seen_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                artist = excluded.artist,
                duration_seconds = excluded.duration_seconds,
                thumbnail_url = excluded.thumbnail_url,
                last_seen_at = excluded.last_seen_at",
            rusqlite::params![
                track.id,
                track.title,
                track.artist,
                track.duration_seconds,
                track.thumbnail_url,
                now,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_in_memory_db_applies_migrations_cleanly() {
        Db::open_in_memory().expect("migrations should apply without error");
    }

    #[test]
    fn opening_twice_is_idempotent() {
        // Simulates restarting the app against the same file: migrations
        // must not fail or duplicate schema objects on a second run.
        let dir = std::env::temp_dir().join(format!("echora-test-{}", now()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("echora.sqlite");
        let path = path.to_str().unwrap();

        Db::open(path).expect("first open should succeed");
        Db::open(path).expect("second open should succeed");

        std::fs::remove_dir_all(&dir).ok();
    }
}
