//! Link radio: turning a YouTube Mix into queue candidates. Pure and
//! network-free -- the yt-dlp call lives in `Resolver::radio`, the
//! orchestration in `commands::link`.

use std::collections::HashSet;

use crate::models::Track;
use crate::mood_engine::candidates;
use crate::mood_engine::scoring::ScoringContext;

/// Enough for ~an hour of music per fetch while staying one quick yt-dlp
/// call; top-up runs again long before it's exhausted.
#[allow(dead_code)]
// Wired in by commands::link (link radio Task 5); remove this allow then.
pub const RADIO_FETCH_LIMIT: u32 = 30;

/// Same filters mood candidates get (dedup, non-music, known-unavailable)
/// plus disliked tracks and anything already in this session -- but no
/// score shuffle: Mix order is what carries the similarity.
#[allow(dead_code)]
// Wired in by commands::link (link radio Task 5); remove this allow then.
pub fn filter_radio(
    raw: Vec<Track>,
    exclude_ids: &HashSet<String>,
    ctx: &ScoringContext,
) -> Vec<Track> {
    let musical = candidates::filter_non_music(candidates::dedup(raw));
    candidates::filter_out_unavailable(musical, |id| ctx.unavailable_tracks.contains(id))
        .into_iter()
        .filter(|t| !exclude_ids.contains(&t.id))
        .filter(|t| ctx.feedback.get(&t.id) != Some(&false))
        .collect()
}

/// The Mix to fetch next continues from the newest track in the queue,
/// so the radio drifts naturally; with an empty queue, the original seed.
#[allow(dead_code)]
// Wired in by commands::link (link radio Task 5); remove this allow then.
pub fn next_seed(queue: &[Track], original_seed_id: &str) -> String {
    queue
        .last()
        .map(|t| t.id.clone())
        .unwrap_or_else(|| original_seed_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: format!("Song {id}"),
            artist: Some("Artist".into()),
            duration_seconds: Some(200),
            thumbnail_url: None,
        }
    }

    fn ids(tracks: &[Track]) -> Vec<&str> {
        tracks.iter().map(|t| t.id.as_str()).collect()
    }

    #[test]
    fn filter_radio_keeps_mix_order() {
        let out = filter_radio(
            vec![track("c"), track("a"), track("b")],
            &HashSet::new(),
            &ScoringContext::default(),
        );
        assert_eq!(ids(&out), ["c", "a", "b"]);
    }

    #[test]
    fn filter_radio_drops_session_tracks_disliked_unavailable_and_dupes() {
        let mut ctx = ScoringContext::default();
        ctx.feedback.insert("disliked".into(), false);
        ctx.feedback.insert("liked".into(), true);
        ctx.unavailable_tracks.insert("gone".into());
        let exclude: HashSet<String> = ["played".to_string()].into();
        let out = filter_radio(
            vec![
                track("played"),
                track("disliked"),
                track("gone"),
                track("liked"),
                track("new"),
                track("new"),
            ],
            &exclude,
            &ctx,
        );
        assert_eq!(ids(&out), ["liked", "new"]);
    }

    #[test]
    fn filter_radio_of_all_seen_tracks_is_empty() {
        let exclude: HashSet<String> = ["a".to_string(), "b".to_string()].into();
        let out = filter_radio(
            vec![track("a"), track("b")],
            &exclude,
            &ScoringContext::default(),
        );
        assert!(out.is_empty());
    }

    #[test]
    fn next_seed_is_the_last_queued_track() {
        assert_eq!(next_seed(&[track("a"), track("b")], "seed"), "b");
    }

    #[test]
    fn next_seed_falls_back_to_the_original_seed_on_an_empty_queue() {
        assert_eq!(next_seed(&[], "seed"), "seed");
    }
}
