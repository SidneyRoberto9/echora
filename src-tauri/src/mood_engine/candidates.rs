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

/// Drops tracks `is_unavailable` reports as already known-bad (see
/// `Db::mark_track_unavailable`) — so a track that failed to resolve once
/// doesn't keep resurfacing as a fresh candidate every round. Takes a
/// predicate rather than a `Db` reference directly, same as the rest of
/// this module: keeps it trivially unit-testable and leaves the caller
/// free to back it with a single bulk lookup instead of one query per
/// track.
///
/// Called from `mood_engine::rank_candidates`, which passes the set
/// `Db::all_unavailable_track_ids` loaded once per top-up — the bulk
/// lookup won over a per-track query.
pub fn filter_out_unavailable(
    tracks: Vec<Track>,
    is_unavailable: impl Fn(&str) -> bool,
) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|track| !is_unavailable(&track.id))
        .collect()
}

/// Below this, a "vibe" search result is almost always a meme clip or
/// Short, not a song — measured junk against real mood queries topped out
/// at 43s, and the shortest real song/music-video seen survive at 151s and
/// 178s. Never raise this much further without re-measuring: it's already
/// close to cutting real short tracks. A `None` duration (most commonly a
/// 24/7 live radio stream) is let through unfiltered — this floor doesn't
/// attempt to classify live content.
const MIN_TRACK_DURATION_SECONDS: u32 = 60;

/// Title/channel substrings strongly associated with non-music content that
/// still matches mood "vibe" queries (trailers, reactions, gameplay, vlogs).
/// Case-insensitive substring match, not a classifier.
///
/// ponytail: this list is tuned data, not a solved problem — it will leak
/// (a vlog with none of these words, a song that happens to contain one).
/// Extend it from real leakage instead of trying to enumerate every
/// non-music genre upfront. Deliberately excludes generic words a real
/// song title collides with — e.g. NOT "habits" (Tove Lo's "Habits (Stay
/// High)") and NOT "compilation" (measured: YouTube itself categorizes
/// long fan-curated DJ mixes titled "... mix compilation" as `Music`,
/// unlike the entries below, which were confirmed non-music results
/// against real mood queries — see mood_engine research).
const NON_MUSIC_KEYWORDS: &[&str] = &[
    "trailer",
    "movie clip",
    "full episode",
    "interview",
    "reaction",
    "react to",
    "vlog",
    "gameplay",
    "walkthrough",
    "let's play",
    "tutorial",
    "explained",
    "review",
    "unboxing",
    "asmr",
    "podcast",
    "prank",
    "shorts",
    "days in my life",
    "get ready with me",
    "grwm",
    "tips to",
    "edit audio",
    "subliminal",
    "to become",
    "guide to",
];

/// Drops candidates that a mood's "vibe" search phrase pulled in but that
/// aren't actually music — a duration floor for meme clips/Shorts, plus a
/// title/channel keyword denylist for trailers, reactions, vlogs, etc. (see
/// the constants above). This is the content-type gate the pipeline never
/// had: everything upstream (yt-dlp `ytsearch`) returns whatever YouTube's
/// search ranks for the query, music or not.
pub fn filter_non_music(tracks: Vec<Track>) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|track| {
            if let Some(duration) = track.duration_seconds
                && duration < MIN_TRACK_DURATION_SECONDS
            {
                return false;
            }
            let haystack = format!(
                "{} {}",
                track.title.to_lowercase(),
                track.artist.as_deref().unwrap_or("").to_lowercase()
            );
            !NON_MUSIC_KEYWORDS.iter().any(|kw| haystack.contains(kw))
        })
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

    fn track_with(
        id: &str,
        title: &str,
        artist: Option<&str>,
        duration_seconds: Option<u32>,
    ) -> Track {
        Track {
            id: id.into(),
            title: title.into(),
            artist: artist.map(String::from),
            duration_seconds,
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
    fn filter_out_unavailable_drops_only_the_marked_tracks() {
        let tracks = vec![track("a"), track("b"), track("c")];
        let result = filter_out_unavailable(tracks, |id| id == "b");
        assert_eq!(
            result.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "c"]
        );
    }

    #[test]
    fn filter_out_unavailable_keeps_everything_when_nothing_is_marked() {
        let tracks = vec![track("a"), track("b")];
        let result = filter_out_unavailable(tracks, |_| false);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn filter_out_unavailable_of_empty_input_is_empty() {
        assert!(filter_out_unavailable(vec![], |_| true).is_empty());
    }

    #[test]
    fn filter_non_music_drops_tracks_shorter_than_the_floor() {
        let tracks = vec![track_with("short", "some meme clip", None, Some(15))];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_keeps_tracks_at_or_above_the_floor() {
        let tracks = vec![track_with("song", "a real song", None, Some(60))];
        assert_eq!(filter_non_music(tracks).len(), 1);
    }

    #[test]
    fn filter_non_music_keeps_unknown_duration_tracks() {
        // Most commonly a 24/7 live radio stream -- not classified here.
        let tracks = vec![track_with("live", "lofi radio", None, None)];
        assert_eq!(filter_non_music(tracks).len(), 1);
    }

    #[test]
    fn filter_non_music_drops_tracks_matching_a_denylist_keyword_in_the_title() {
        let tracks = vec![track_with(
            "trailer",
            "Movie Official Trailer",
            None,
            Some(120),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_denylist_match_is_case_insensitive() {
        let tracks = vec![track_with(
            "react",
            "REACTION to this song",
            None,
            Some(300),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_drops_tracks_matching_a_denylist_keyword_in_the_channel() {
        let tracks = vec![track_with(
            "vlog",
            "a day in the city",
            Some("Some Vlog Channel"),
            Some(600),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_keeps_real_songs() {
        let tracks = vec![track_with(
            "keep",
            "Actual Song Title",
            Some("Real Artist"),
            Some(210),
        )];
        assert_eq!(filter_non_music(tracks).len(), 1);
    }

    #[test]
    fn filter_non_music_drops_tiktok_style_edit_audio_compilations() {
        // Real measured leak: "edit audios for your villain arc" -- short
        // decontextualized audio snippets, not full songs (categories:
        // Entertainment, not Music).
        let tracks = vec![track_with(
            "edit",
            "Edit audios for your villain arc + timestamps",
            None,
            Some(1552),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_drops_subliminal_audio() {
        let tracks = vec![track_with(
            "sub",
            "I AM POWER Divine Authority Unstoppable Force (subliminal)",
            None,
            Some(768),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_drops_self_help_vlogs_phrased_as_becoming_something() {
        // Real measured leak: "7 Simple Habits to Become THAT GIRL".
        let tracks = vec![track_with(
            "vlog2",
            "7 Simple Habits to Become THAT GIRL",
            None,
            Some(1037),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_drops_guide_style_vlogs() {
        let tracks = vec![track_with(
            "vlog3",
            "A guide to classy woman energy",
            None,
            Some(1258),
        )];
        assert!(filter_non_music(tracks).is_empty());
    }

    #[test]
    fn filter_non_music_keeps_a_song_whose_title_contains_habits() {
        // Tove Lo's "Habits (Stay High)" is exactly why "habits" alone is
        // NOT in the denylist -- only the more specific vlog phrasing is.
        let tracks = vec![track_with(
            "tovelo",
            "Tove Lo - Habits (Stay High)",
            Some("Tove Lo"),
            Some(230),
        )];
        assert_eq!(filter_non_music(tracks).len(), 1);
    }

    #[test]
    fn filter_non_music_keeps_a_long_fan_curated_mix_labeled_compilation() {
        // Measured: YouTube itself categorizes this as Music -- "compilation"
        // is deliberately NOT in the denylist.
        let tracks = vec![track_with(
            "mix",
            "BADASS GIRL energy (mix compilation) girl boss playlist",
            None,
            Some(6393),
        )];
        assert_eq!(filter_non_music(tracks).len(), 1);
    }

    #[test]
    fn filter_non_music_of_empty_input_is_empty() {
        assert!(filter_non_music(vec![]).is_empty());
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
