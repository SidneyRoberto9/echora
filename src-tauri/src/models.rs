use serde::{Deserialize, Serialize};

/// A normalized, trusted representation of an external track. Anything
/// coming from yt-dlp/search results must be converted into this shape
/// before it touches the queue, history, or persistence — never passed
/// through raw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub duration_seconds: Option<u32>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct MoodTraits {
    pub energy: u8,
    pub darkness: u8,
    pub romance: u8,
    pub sadness: u8,
    pub aggression: u8,
    pub focus: u8,
}

/// Full mood record, including the search queries used to build a session.
/// Loaded once from the bundled `resources/moods.json` — never a database
/// row (see docs/adr/0008-moods-as-bundled-resource.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mood {
    pub id: String,
    pub name: String,
    pub category: String,
    pub traits: MoodTraits,
    pub queries: Vec<String>,
}

/// What the frontend actually needs to list/browse moods. Deliberately
/// excludes `queries` — search query strategy is a backend implementation
/// detail, not a frontend contract.
#[derive(Debug, Clone, Serialize)]
pub struct MoodSummary {
    pub id: String,
    pub name: String,
    pub category: String,
    pub traits: MoodTraits,
}

impl From<&Mood> for MoodSummary {
    fn from(mood: &Mood) -> Self {
        MoodSummary {
            id: mood.id.clone(),
            name: mood.name.clone(),
            category: mood.category.clone(),
            traits: mood.traits,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: i64,
    pub mood_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: i64,
    pub mood_id: String,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub track_count: u32,
}

/// A resolved, playable stream. Deliberately never persisted — YouTube
/// stream URLs expire, so this only ever lives in memory for the duration
/// of a single playback (see docs/adr — never save stream URLs as durable).
#[derive(Debug, Clone, Serialize)]
pub struct ResolvedStream {
    pub track_id: String,
    pub stream_url: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueView {
    pub current: Option<Track>,
    pub upcoming: Vec<Track>,
}

/// User-configurable settings, persisted as a single JSON blob (see
/// docs/adr — settings table is a one-row KV store, not a rigid schema, so
/// adding a field never needs a migration).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub cache_limit_mb: u32,
    pub history_enabled: bool,
    pub crash_report_enabled: bool,
    pub autostart_enabled: bool,
    pub sponsorblock_categories: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            cache_limit_mb: 500,
            history_enabled: true,
            crash_report_enabled: false,
            autostart_enabled: false,
            sponsorblock_categories: vec!["sponsor".into(), "selfpromo".into(), "intro".into()],
        }
    }
}
