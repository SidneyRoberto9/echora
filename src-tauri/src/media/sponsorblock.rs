use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::error::{EchoraError, Result};
use crate::state::AppState;

const API_BASE: &str = "https://sponsor.ajay.app/api/skipSegments";
const USER_AGENT: &str = "Echora/0.1 (+https://github.com/SidneyRoberto9/echora)";

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub category: String,
}

#[derive(Debug, Deserialize)]
struct VideoEntry {
    #[serde(rename = "videoID")]
    video_id: String,
    segments: Vec<RawSegment>,
}

#[derive(Debug, Deserialize)]
struct RawSegment {
    segment: [f64; 2],
    category: String,
    #[serde(rename = "actionType")]
    action_type: String,
}

/// Parses a raw SponsorBlock hash-prefix response body, keeping only the
/// entry matching `video_id` exactly (the API returns every video sharing
/// the K-Anonymity hash prefix) and only its skip-type segments (`"mute"`
/// and highlight-only markers don't apply to a plain seek-past skip).
fn parse_segments(body: &str, video_id: &str) -> Result<Vec<Segment>> {
    let entries: Vec<VideoEntry> =
        serde_json::from_str(body).map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    Ok(entries
        .into_iter()
        .find(|entry| entry.video_id == video_id)
        .map(|entry| {
            entry
                .segments
                .into_iter()
                .filter(|s| s.action_type == "skip")
                .map(|s| Segment {
                    start: s.segment[0],
                    end: s.segment[1],
                    category: s.category,
                })
                .collect()
        })
        .unwrap_or_default())
}

/// Fetches SponsorBlock's segment data for exactly `video_id`, filtered to
/// `categories`. Ephemeral: nothing here is cached or written to disk (see
/// ADR 0009's licensing mitigation) — the caller holds the result only in
/// memory for the current playback session. Any failure (network, parse,
/// unexpected status) is a normal `Err` here — callers treat it as
/// best-effort and discard it rather than surfacing it to the user.
pub async fn fetch_segments(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    if categories.is_empty() {
        return Ok(Vec::new());
    }
    let video_id_owned = video_id.to_string();
    let categories_owned = categories.to_vec();
    tokio::task::spawn_blocking(move || fetch_segments_blocking(&video_id_owned, &categories_owned))
        .await
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?
}

fn fetch_segments_blocking(video_id: &str, categories: &[String]) -> Result<Vec<Segment>> {
    let hash = Sha256::digest(video_id.as_bytes());
    let prefix: String = hash.as_slice()[..2]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let categories_json = serde_json::to_string(categories)
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    let url = format!("{API_BASE}/{prefix}");
    let response = attohttpc::get(&url)
        .param("categories", categories_json)
        .header("User-Agent", USER_AGENT)
        .timeout(Duration::from_secs(5))
        .send()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;

    if response.status() == attohttpc::StatusCode::NOT_FOUND {
        // No submissions for any video sharing this hash prefix — normal,
        // not a failure.
        return Ok(Vec::new());
    }
    if !response.is_success() {
        return Err(EchoraError::SponsorBlock(format!(
            "unexpected status {}",
            response.status()
        )));
    }

    let body = response
        .text()
        .map_err(|err| EchoraError::SponsorBlock(err.to_string()))?;
    parse_segments(&body, video_id)
}

/// Runs forever: once a second, detects whether the current track changed
/// (comparing `queue.current()`'s id against what this loop last saw) and,
/// on change, clears any stale segments immediately and kicks off a fetch
/// for the new track — then, every tick regardless, checks the player's
/// position against whatever segments are currently loaded and seeks past
/// a match. One task does both jobs so nothing needs to reach back into
/// `commands::resolve_and_load` or thread an `AppHandle` through the
/// existing command call chain — `queue.current()` already reflects every
/// playback entry point (mood session, scene, single favorited track)
/// uniformly.
pub async fn watch(app: AppHandle) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    let mut last_track_id: Option<String> = None;

    loop {
        interval.tick().await;
        let state = app.state::<AppState>();

        let current_track = state.queue.lock().unwrap().current().cloned();
        let current_id = current_track.as_ref().map(|t| t.id.clone());

        if current_id != last_track_id {
            last_track_id = current_id;
            *state.sponsorblock_segments.lock().unwrap() = Vec::new();

            if let Some(track) = current_track {
                let categories = state
                    .db
                    .lock()
                    .unwrap()
                    .get_settings()
                    .map(|s| s.sponsorblock_categories)
                    .unwrap_or_default();

                if !categories.is_empty()
                    && let Ok(segments) = fetch_segments(&track.id, &categories).await
                {
                    let still_current = state
                        .queue
                        .lock()
                        .unwrap()
                        .current()
                        .is_some_and(|t| t.id == track.id);
                    if still_current {
                        *state.sponsorblock_segments.lock().unwrap() = segments;
                    }
                }
            }
            continue;
        }

        let segments = state.sponsorblock_segments.lock().unwrap().clone();
        if segments.is_empty() {
            continue;
        }

        let player = state.player.lock().await;
        let Ok(Some(position)) = player.position_seconds().await else {
            continue;
        };
        if let Some(hit) = segments
            .iter()
            .find(|s| position >= s.start && position < s.end)
        {
            let _ = player.seek_to(hit.end).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_segments_keeps_only_the_matching_video_and_skip_segments() {
        let body = include_str!("fixtures/sponsorblock_response.json");
        let segments = parse_segments(body, "dQw4w9WgXcQ").unwrap();

        assert_eq!(
            segments,
            vec![Segment {
                start: 10.5,
                end: 45.2,
                category: "sponsor".into(),
            }]
        );
    }

    #[test]
    fn parse_segments_returns_empty_for_a_video_id_not_in_the_response() {
        let body = include_str!("fixtures/sponsorblock_response.json");
        let segments = parse_segments(body, "not-in-the-fixture").unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn parse_segments_errors_on_malformed_json() {
        let err = parse_segments("not json", "dQw4w9WgXcQ").unwrap_err();
        assert!(matches!(err, EchoraError::SponsorBlock(_)));
    }

    #[tokio::test]
    #[ignore]
    async fn fetching_segments_for_a_real_video_does_not_error() {
        // Not asserting non-empty results — which videos have live
        // SponsorBlock submissions changes over time as community data
        // drifts (verified: this specific video currently has none for
        // these categories). The fixture-based unit tests above already
        // prove the parsing/filtering logic; this only proves the real
        // hash-prefix -> network -> status-handling -> JSON-parse round
        // trip doesn't error.
        let result = fetch_segments(
            "dQw4w9WgXcQ",
            &["sponsor".to_string(), "selfpromo".to_string()],
        )
        .await;
        assert!(
            result.is_ok(),
            "real SponsorBlock request should not error: {result:?}"
        );
    }
}
