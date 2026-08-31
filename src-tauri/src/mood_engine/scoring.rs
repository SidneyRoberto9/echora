use std::collections::{HashMap, HashSet};

use rand::{Rng, RngExt};

use crate::models::Track;

/// Everything the scorer needs about the listener's history, pre-fetched
/// once per candidate round (see `db/scoring_signals.rs`) rather than
/// queried per-track — a handful of small in-memory lookups instead of N
/// database round-trips per candidate.
#[derive(Debug, Default, Clone)]
pub struct ScoringContext {
    /// track_id -> true (liked) / false (disliked)
    pub feedback: HashMap<String, bool>,
    pub favorited_tracks: HashSet<String>,
    /// tracks played recently enough that resurfacing them feels repetitive
    pub recently_played: HashSet<String>,
    /// track_id -> average completion percentage (0.0-1.0) across past plays
    pub avg_completion: HashMap<String, f64>,
    /// artist name -> how many of that artist's tracks the listener liked
    pub liked_artist_counts: HashMap<String, u32>,
}

/// The recommendation formula: rewards liked/favorited/finished/
/// artist-affine tracks, penalizes disliked or recently-repeated ones.
/// Deliberately simple heuristics, not machine learning — see the
/// product brief's "prefer heuristics over ML when they're enough."
pub fn score(track: &Track, ctx: &ScoringContext) -> i32 {
    let mut total = 0;

    if let Some(&liked) = ctx.feedback.get(&track.id) {
        total += if liked { 30 } else { -50 };
    }

    if ctx.favorited_tracks.contains(&track.id) {
        total += 20;
    }

    if let Some(artist) = &track.artist
        && let Some(&count) = ctx.liked_artist_counts.get(artist)
    {
        total += (count.min(5) as i32) * 4;
    }

    if let Some(&completion) = ctx.avg_completion.get(&track.id) {
        // Centered on 0.5: finishing most of a track nudges the score up,
        // skipping it early nudges it down.
        total += ((completion - 0.5) * 40.0) as i32;
    }

    if ctx.recently_played.contains(&track.id) {
        total -= 25;
    }

    total
}

/// Orders candidates mostly-best-first without being perfectly
/// deterministic — jitters each score by a small random amount before
/// sorting, so the same top candidate doesn't always lead every session
/// for a given mood.
pub fn shuffle_by_score(
    candidates: Vec<Track>,
    ctx: &ScoringContext,
    rng: &mut impl Rng,
) -> Vec<Track> {
    let mut scored: Vec<(Track, i32)> = candidates
        .into_iter()
        .map(|track| {
            let jitter = rng.random_range(-10..=10);
            let base = score(&track, ctx);
            (track, base + jitter)
        })
        .collect();
    scored.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
    scored.into_iter().map(|(track, _)| track).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn track(id: &str, artist: Option<&str>) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: artist.map(String::from),
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn liked_track_scores_higher_than_neutral() {
        let mut ctx = ScoringContext::default();
        ctx.feedback.insert("liked".into(), true);
        assert!(score(&track("liked", None), &ctx) > score(&track("neutral", None), &ctx));
    }

    #[test]
    fn disliked_track_scores_lower_than_neutral() {
        let mut ctx = ScoringContext::default();
        ctx.feedback.insert("disliked".into(), false);
        assert!(score(&track("disliked", None), &ctx) < score(&track("neutral", None), &ctx));
    }

    #[test]
    fn favorited_track_scores_higher_than_neutral() {
        let mut ctx = ScoringContext::default();
        ctx.favorited_tracks.insert("fav".into());
        assert!(score(&track("fav", None), &ctx) > score(&track("neutral", None), &ctx));
    }

    #[test]
    fn recently_played_track_scores_lower_than_neutral() {
        let mut ctx = ScoringContext::default();
        ctx.recently_played.insert("repeat".into());
        assert!(score(&track("repeat", None), &ctx) < score(&track("neutral", None), &ctx));
    }

    #[test]
    fn high_completion_scores_higher_than_low_completion() {
        let mut ctx = ScoringContext::default();
        ctx.avg_completion.insert("finished".into(), 0.95);
        ctx.avg_completion.insert("skipped".into(), 0.05);
        assert!(score(&track("finished", None), &ctx) > score(&track("skipped", None), &ctx));
    }

    #[test]
    fn artist_with_more_liked_tracks_scores_higher() {
        let mut ctx = ScoringContext::default();
        ctx.liked_artist_counts.insert("Favorite Artist".into(), 4);
        let scored = score(&track("x", Some("Favorite Artist")), &ctx);
        let unscored = score(&track("y", Some("Unknown Artist")), &ctx);
        assert!(scored > unscored);
    }

    #[test]
    fn shuffle_by_score_keeps_every_candidate_exactly_once() {
        let ctx = ScoringContext::default();
        let candidates = vec![track("a", None), track("b", None), track("c", None)];
        let mut rng = StdRng::seed_from_u64(7);
        let result = shuffle_by_score(candidates, &ctx, &mut rng);
        let mut ids: Vec<_> = result.iter().map(|t| t.id.clone()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }

    #[test]
    fn shuffle_by_score_puts_a_strongly_liked_track_near_the_front() {
        let mut ctx = ScoringContext::default();
        ctx.feedback.insert("best".into(), true);
        ctx.favorited_tracks.insert("best".into());
        let candidates = vec![
            track("meh1", None),
            track("meh2", None),
            track("best", None),
            track("meh3", None),
        ];
        let mut rng = StdRng::seed_from_u64(7);
        let result = shuffle_by_score(candidates, &ctx, &mut rng);
        // +/-10 jitter can't overcome a +50 base score advantage.
        assert_eq!(result[0].id, "best");
    }
}
