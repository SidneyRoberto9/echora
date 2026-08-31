use crate::error::{EchoraError, Result};
use crate::models::{ResolvedStream, Track};

/// One line of `yt-dlp --flat-playlist --dump-json` output -> a `Track`.
/// Everything from yt-dlp is untrusted input — normalize and validate here,
/// never pass it through raw.
pub fn parse_search_result(line: &str) -> Result<Track> {
    let value: serde_json::Value = serde_json::from_str(line)?;
    let id = value
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EchoraError::Metadata("search result missing id".into()))?
        .to_string();
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled")
        .to_string();
    let artist = value
        .get("channel")
        .or_else(|| value.get("uploader"))
        .and_then(|v| v.as_str())
        .map(String::from);
    let duration_seconds = value
        .get("duration")
        .and_then(|v| v.as_f64())
        .map(|d| d.round() as u32);
    let thumbnail_url = best_thumbnail(&value);

    Ok(Track {
        id,
        title,
        artist,
        duration_seconds,
        thumbnail_url,
    })
}

/// The single JSON object from `yt-dlp -f bestaudio --dump-json <url>`.
/// `track_id` is passed in (not re-read from the JSON) since it's already
/// known and validated by the caller.
pub fn parse_resolved(json: &str, track_id: &str) -> Result<ResolvedStream> {
    let value: serde_json::Value = serde_json::from_str(json)?;
    let stream_url = value
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| EchoraError::Metadata("resolved output missing a stream url".into()))?
        .to_string();

    Ok(ResolvedStream {
        track_id: track_id.to_string(),
        stream_url,
    })
}

/// yt-dlp reports failures on stderr with a human-readable message and a
/// non-zero exit code, not structured JSON. Map the known, common cases to
/// a specific unavailability reason; anything unrecognized still resolves
/// to a generic "unavailable" rather than a hard crash — the session should
/// skip the track and continue.
pub fn classify_ytdlp_failure(stderr: &str) -> EchoraError {
    let lower = stderr.to_lowercase();
    let reason = if lower.contains("private video") {
        "private"
    } else if lower.contains("not available in your country")
        || lower.contains("not made this video available")
    {
        "region_blocked"
    } else if lower.contains("video unavailable") || lower.contains("removed") {
        "removed"
    } else {
        "unknown"
    };
    EchoraError::TrackUnavailable(reason.into())
}

/// yt-dlp gives a top-level `thumbnail` convenience field on full (non-flat)
/// output, but flat-playlist search results only have a `thumbnails` array
/// — the last entry is conventionally the highest-resolution one.
fn best_thumbnail(value: &serde_json::Value) -> Option<String> {
    if let Some(url) = value.get("thumbnail").and_then(|v| v.as_str()) {
        return Some(url.to_string());
    }
    value
        .get("thumbnails")?
        .as_array()?
        .last()?
        .get("url")?
        .as_str()
        .map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_FIXTURE: &str = include_str!("fixtures/search_flat_playlist.jsonl");
    const RESOLVE_FIXTURE: &str = include_str!("fixtures/resolve_bestaudio.json");
    const ERR_PRIVATE: &str = include_str!("fixtures/error_private.txt");
    const ERR_UNAVAILABLE: &str = include_str!("fixtures/error_unavailable.txt");
    const ERR_REGION_BLOCKED: &str = include_str!("fixtures/error_region_blocked.txt");

    fn first_search_line() -> &'static str {
        SEARCH_FIXTURE.lines().next().unwrap()
    }

    #[test]
    fn parses_a_real_search_result_line() {
        let track = parse_search_result(first_search_line()).unwrap();
        assert_eq!(track.id, "rFZHOHl-L8A");
        assert_eq!(track.title, "lofi hip hop radio 📚 beats to relax/study to");
        assert_eq!(track.artist.as_deref(), Some("Lofi Girl"));
        assert!(track.thumbnail_url.unwrap().starts_with("https://"));
    }

    #[test]
    fn search_result_missing_duration_stays_none_not_zero() {
        // The fixture's first entry is a livestream — yt-dlp reports no duration for it.
        let track = parse_search_result(first_search_line()).unwrap();
        assert_eq!(track.duration_seconds, None);
    }

    #[test]
    fn search_result_without_an_id_errors() {
        let err = parse_search_result(r#"{"title": "no id here"}"#).unwrap_err();
        assert!(matches!(err, EchoraError::Metadata(_)));
    }

    #[test]
    fn search_result_is_not_valid_json_errors() {
        let err = parse_search_result("not json at all").unwrap_err();
        assert!(matches!(err, EchoraError::Serde(_)));
    }

    #[test]
    fn parses_a_real_resolved_stream() {
        let resolved = parse_resolved(RESOLVE_FIXTURE, "qKVBgH6k2oU").unwrap();
        assert_eq!(resolved.track_id, "qKVBgH6k2oU");
        assert!(resolved.stream_url.starts_with("https://"));
    }

    #[test]
    fn resolved_without_a_url_errors() {
        let err = parse_resolved(r#"{"id": "x", "title": "no url here"}"#, "x").unwrap_err();
        assert!(matches!(err, EchoraError::Metadata(_)));
    }

    #[test]
    fn classifies_private_video() {
        let err = classify_ytdlp_failure(ERR_PRIVATE);
        assert!(matches!(err, EchoraError::TrackUnavailable(r) if r == "private"));
    }

    #[test]
    fn classifies_removed_video() {
        let err = classify_ytdlp_failure(ERR_UNAVAILABLE);
        assert!(matches!(err, EchoraError::TrackUnavailable(r) if r == "removed"));
    }

    #[test]
    fn classifies_region_blocked_video() {
        let err = classify_ytdlp_failure(ERR_REGION_BLOCKED);
        assert!(matches!(err, EchoraError::TrackUnavailable(r) if r == "region_blocked"));
    }

    #[test]
    fn unrecognized_failure_text_still_yields_a_generic_unavailable() {
        let err = classify_ytdlp_failure("ERROR: something completely unexpected happened");
        assert!(matches!(err, EchoraError::TrackUnavailable(r) if r == "unknown"));
    }
}
