use rusqlite::OptionalExtension;

use super::Db;
use crate::error::Result;
use crate::models::Settings;

impl Db {
    pub fn get_settings(&self) -> Result<Settings> {
        let row: Option<String> = self
            .conn
            .query_row("SELECT data FROM settings WHERE id = 1", [], |r| r.get(0))
            .optional()?;
        match row {
            Some(json) => Ok(serde_json::from_str(&json)?),
            None => Ok(Settings::default()),
        }
    }

    pub fn save_settings(&self, settings: &Settings) -> Result<()> {
        let json = serde_json::to_string(settings)?;
        self.conn.execute(
            "INSERT INTO settings (id, data) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            [json],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_settings_returns_defaults_when_nothing_saved() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.get_settings().unwrap(), Settings::default());
    }

    #[test]
    fn save_then_get_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let settings = Settings {
            cache_limit_mb: 1000,
            history_enabled: false,
            ..Default::default()
        };

        db.save_settings(&settings).unwrap();

        assert_eq!(db.get_settings().unwrap(), settings);
    }

    #[test]
    fn saving_twice_overwrites_not_duplicates() {
        let db = Db::open_in_memory().unwrap();
        let mut settings = Settings::default();
        db.save_settings(&settings).unwrap();
        settings.cache_limit_mb = 2000;
        db.save_settings(&settings).unwrap();

        assert_eq!(db.get_settings().unwrap().cache_limit_mb, 2000);
    }
}
