use std::collections::HashMap;

use crate::error::{EchoraError, Result};
use crate::models::{Mood, MoodSummary};

const MOODS_JSON: &str = include_str!("../resources/moods.json");

/// The mood catalog is bundled app data (see docs/adr/0008), loaded once at
/// startup — never a database table.
pub struct MoodCatalog {
    by_id: HashMap<String, Mood>,
    order: Vec<String>,
}

impl MoodCatalog {
    pub fn load() -> Result<Self> {
        let moods: Vec<Mood> = serde_json::from_str(MOODS_JSON)?;
        let mut by_id = HashMap::with_capacity(moods.len());
        let mut order = Vec::with_capacity(moods.len());
        for mood in moods {
            order.push(mood.id.clone());
            by_id.insert(mood.id.clone(), mood);
        }
        Ok(MoodCatalog { by_id, order })
    }

    /// Summaries in the catalog's declared order (not alphabetical/random),
    /// so category grouping in `resources/moods.json` is preserved.
    pub fn list(&self) -> Vec<MoodSummary> {
        self.order
            .iter()
            .filter_map(|id| self.by_id.get(id))
            .map(MoodSummary::from)
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<&Mood> {
        self.by_id
            .get(id)
            .ok_or_else(|| EchoraError::UnknownMood(id.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_the_bundled_catalog_succeeds() {
        MoodCatalog::load().expect("bundled moods.json should parse");
    }

    #[test]
    fn catalog_has_at_least_fifty_moods() {
        let catalog = MoodCatalog::load().unwrap();
        assert!(
            catalog.list().len() >= 50,
            "expected at least 50 moods, got {}",
            catalog.list().len()
        );
    }

    #[test]
    fn no_mood_ids_collide() {
        let raw: Vec<Mood> = serde_json::from_str(MOODS_JSON).unwrap();
        let catalog = MoodCatalog::load().unwrap();
        assert_eq!(
            raw.len(),
            catalog.list().len(),
            "a duplicate id silently dropped a mood from the catalog"
        );
    }

    #[test]
    fn get_returns_the_matching_mood() {
        let catalog = MoodCatalog::load().unwrap();
        let first_id = catalog.list()[0].id.clone();
        let mood = catalog.get(&first_id).unwrap();
        assert_eq!(mood.id, first_id);
    }

    #[test]
    fn get_unknown_mood_errors() {
        let catalog = MoodCatalog::load().unwrap();
        let err = catalog.get("not-a-real-mood").unwrap_err();
        assert!(matches!(err, EchoraError::UnknownMood(id) if id == "not-a-real-mood"));
    }
}
