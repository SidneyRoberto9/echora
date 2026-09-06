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
        M::up(include_str!("../../migrations/0003_mixed_sessions.sql")),
        M::up(include_str!("../../migrations/0004_group_by_indexes.sql")),
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

/// `journal_mode` is one of the few PRAGMAs that always returns the
/// resulting mode as a row, even in its "set" form (`PRAGMA journal_mode =
/// WAL`) — a bare `conn.execute(...)` errors with `ExecuteReturnedResults`
/// wherever the `extra_check` feature is on. `pragma_update_and_check` is
/// rusqlite's purpose-built method for this: it sets the value and reads
/// the row back via `query_row` in one call, so we can also confirm what
/// mode SQLite actually landed on.
///
/// WAL is persisted in the database file itself, not per-connection, and
/// cuts fsyncs per write — `record_play` runs on every track and settings
/// writes can burst (e.g. a debounced volume slider), so this matters for
/// the "lightness" priority. `synchronous = NORMAL` is WAL's recommended
/// pairing (safe against app crashes, only loses durability on an OS
/// crash/power loss, which this desktop player doesn't need to guard).
///
/// `:memory:` connections (test-only) can't use WAL — SQLite has nowhere
/// to put a WAL file — and silently stay on `"memory"` instead of
/// erroring, so we don't assert the returned mode, only propagate a real
/// failure.
fn configure_pragmas(conn: &Connection) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    let _journal_mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

impl Db {
    pub fn open(path: &str) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        configure_pragmas(&conn)?;
        migrations().to_latest(&mut conn)?;
        Ok(Db { conn })
    }

    /// Test-only: an ephemeral database for unit tests, so they never touch
    /// disk or share state with each other.
    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let mut conn = Connection::open_in_memory()?;
        configure_pragmas(&conn)?;
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

    #[test]
    fn open_enables_wal_journal_mode_on_a_real_file() {
        // P2-14: a file-backed database must actually land on WAL, not just
        // attempt it — `pragma_update` alone would silently accept whatever
        // mode SQLite picked.
        let dir = std::env::temp_dir().join(format!("echora-wal-test-{}", now()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("echora.sqlite");
        let path = path.to_str().unwrap();

        let db = Db::open(path).unwrap();
        let journal_mode: String = db
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");

        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn open_in_memory_does_not_fail_even_though_it_cannot_use_wal() {
        // SQLite keeps `:memory:` connections on the "memory" journal mode
        // regardless of what we ask for; `configure_pragmas` must treat
        // that as success, not an error to propagate or panic on.
        let db = Db::open_in_memory().expect("in-memory open must not fail over WAL");
        let journal_mode: String = db
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode, "memory");
    }
}
