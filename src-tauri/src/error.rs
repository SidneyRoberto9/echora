use serde::Serialize;

/// Every failure mode the Rust core can surface to the frontend.
///
/// Kept as one enum (not per-module errors) because Tauri commands need a
/// single error type to convert into, and the frontend needs one stable set
/// of codes to switch on.
#[derive(Debug, thiserror::Error)]
pub enum EchoraError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("queue is empty")]
    QueueEmpty,

    #[error("queue index {0} is out of bounds")]
    QueueIndexOutOfBounds(usize),

    #[error("no active session")]
    NoActiveSession,

    #[error("unknown mood: {0}")]
    UnknownMood(String),

    #[error("invalid mood mix: {0}")]
    InvalidMoodMix(String),

    #[error("malformed sidecar output: {0}")]
    Metadata(String),

    #[error("track unavailable: {0}")]
    TrackUnavailable(String),

    #[error("{0} did not respond in time")]
    SidecarTimeout(String),

    #[error("autostart error: {0}")]
    Autostart(String),

    #[error("sponsorblock error: {0}")]
    SponsorBlock(String),
}

pub type Result<T> = std::result::Result<T, EchoraError>;

/// Stable `{ code, message }` shape sent to the frontend, instead of a bare
/// string, so the UI can switch on `code` without parsing English text.
#[derive(Debug, Serialize)]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: String,
}

impl EchoraError {
    pub fn code(&self) -> &'static str {
        match self {
            EchoraError::Database(_) => "database_error",
            EchoraError::Migration(_) => "migration_error",
            EchoraError::Serde(_) => "serialization_error",
            EchoraError::Io(_) => "io_error",
            EchoraError::QueueEmpty => "queue_empty",
            EchoraError::QueueIndexOutOfBounds(_) => "queue_index_out_of_bounds",
            EchoraError::NoActiveSession => "no_active_session",
            EchoraError::UnknownMood(_) => "unknown_mood",
            EchoraError::InvalidMoodMix(_) => "invalid_mood_mix",
            EchoraError::Metadata(_) => "metadata_error",
            EchoraError::TrackUnavailable(_) => "track_unavailable",
            EchoraError::SidecarTimeout(_) => "sidecar_timeout",
            EchoraError::Autostart(_) => "autostart_error",
            EchoraError::SponsorBlock(_) => "sponsorblock_error",
        }
    }
}

impl Serialize for EchoraError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorPayload {
            code: self.code(),
            message: self.to_string(),
        }
        .serialize(serializer)
    }
}
