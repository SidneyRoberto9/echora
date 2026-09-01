use std::collections::HashSet;

use rand::Rng;
use rand::seq::SliceRandom;

use crate::models::{Mood, Track};

/// Picks which of the mood's queries to actually search this round —
/// shuffled and capped, so consecutive sessions for the same mood don't
/// always search in the same order (see the "don't repeat the same songs"
/// goal in the product brief). Never returns more queries than the mood
/// actually has.
pub fn select_queries(mood: &Mood, count: usize, rng: &mut impl Rng) -> Vec<String> {
    let mut queries = mood.queries.clone();
    queries.shuffle(rng);
    queries.truncate(count.max(1).min(queries.len().max(1)));
    queries
}

/// Splits `total_budget` search-query slots across moods proportional to
/// their weight, rounding each share and guaranteeing every mood gets at
/// least 1 — a small budget or a low-weight mood in a big mix must still get
/// a chance to contribute.
pub fn query_counts_for_weights(total_budget: usize, weights: &[u8]) -> Vec<usize> {
    weights
        .iter()
        .map(|&weight| {
            let share = ((total_budget as f32) * (weight as f32) / 100.0).round() as usize;
            share.max(1)
        })
        .collect()
}

/// Removes tracks already seen earlier in the same candidate batch (same
/// video surfacing from more than one search query), keeping the first
/// occurrence.
pub fn dedup(tracks: Vec<Track>) -> Vec<Track> {
    let mut seen = HashSet::with_capacity(tracks.len());
    tracks
        .into_iter()
        .filter(|track| seen.insert(track.id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

    fn mood_with_queries(queries: &[&str]) -> Mood {
        Mood {
            id: "test".into(),
            name: "Test".into(),
            category: "test".into(),
            traits: Default::default(),
            queries: queries.iter().map(|q| q.to_string()).collect(),
        }
    }

    fn track(id: &str) -> Track {
        Track {
            id: id.into(),
            title: id.into(),
            artist: None,
            duration_seconds: None,
            thumbnail_url: None,
        }
    }

    #[test]
    fn select_queries_never_returns_more_than_requested() {
        let mood = mood_with_queries(&["a", "b", "c", "d", "e"]);
        let mut rng = StdRng::seed_from_u64(1);
        let selected = select_queries(&mood, 2, &mut rng);
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn select_queries_never_returns_more_than_the_mood_has() {
        let mood = mood_with_queries(&["only-one"]);
        let mut rng = StdRng::seed_from_u64(1);
        let selected = select_queries(&mood, 5, &mut rng);
        assert_eq!(selected, vec!["only-one".to_string()]);
    }

    #[test]
    fn select_queries_only_returns_queries_the_mood_actually_has() {
        let mood = mood_with_queries(&["a", "b", "c"]);
        let mut rng = StdRng::seed_from_u64(42);
        let selected = select_queries(&mood, 2, &mut rng);
        for q in &selected {
            assert!(mood.queries.contains(q));
        }
    }

    #[test]
    fn dedup_keeps_first_occurrence_and_drops_repeats() {
        let tracks = vec![track("a"), track("b"), track("a"), track("c"), track("b")];
        let result = dedup(tracks);
        assert_eq!(
            result.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
    }

    #[test]
    fn dedup_of_empty_input_is_empty() {
        assert!(dedup(vec![]).is_empty());
    }

    #[test]
    fn query_counts_for_weights_never_leaves_a_mood_at_zero() {
        let counts = query_counts_for_weights(6, &[70, 20, 10]);
        assert_eq!(counts.len(), 3);
        assert!(counts.iter().all(|&c| c >= 1));
    }

    #[test]
    fn query_counts_for_weights_is_proportional_for_a_dominant_mood() {
        let counts = query_counts_for_weights(10, &[80, 20]);
        assert!(counts[0] > counts[1]);
    }

    #[test]
    fn query_counts_for_weights_floors_a_near_zero_share_at_one() {
        // Raw math: 2 * 1 / 100.0 = 0.02, rounds to 0 — only the `.max(1)`
        // floor saves this mood from getting zero search queries.
        let counts = query_counts_for_weights(2, &[1, 99]);
        assert_eq!(counts[0], 1);
    }
}
