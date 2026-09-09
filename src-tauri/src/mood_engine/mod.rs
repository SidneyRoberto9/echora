pub mod candidates;
pub mod scoring;
pub mod surprise;

use rand::Rng;

use crate::db::Db;
use crate::error::Result;
use crate::media::resolver::Resolver;
use crate::models::{Mood, Track};
use scoring::ScoringContext;

#[derive(Debug, Clone)]
pub struct GenerationConfig {
    /// How many of the mood's queries to search per round.
    pub queries_per_round: usize,
    /// Results requested per query — kept small on purpose (see the
    /// product brief's "don't load hundreds of results ahead of time").
    /// Padded above the actual target count to absorb the ~20-40% that
    /// `candidates::filter_non_music` measurably drops as non-music.
    pub results_per_query: u32,
    /// How many of the most recent sessions count as "recently played"
    /// for the repetition penalty.
    pub recent_session_window: i64,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        GenerationConfig {
            queries_per_round: 2,
            results_per_query: 12,
            recent_session_window: 5,
        }
    }
}

/// Assembles the scorer's view of listening history from the database.
/// Cheap: a handful of aggregate queries against small local tables, done
/// once per candidate round rather than once per candidate.
pub fn build_scoring_context(db: &Db, recent_session_window: i64) -> Result<ScoringContext> {
    Ok(ScoringContext {
        feedback: db.all_track_feedback()?,
        favorited_tracks: db.all_favorited_track_ids()?,
        recently_played: db.recently_played_track_ids(recent_session_window)?,
        avg_completion: db.avg_completion_by_track()?,
        liked_artist_counts: db.liked_artist_counts()?,
        unavailable_tracks: db.all_unavailable_track_ids()?,
    })
}

/// The core mood-engine flow, mix-aware: splits the round's query budget
/// across 1-3 moods proportional to weight (see
/// `candidates::query_counts_for_weights`), searches, dedups, drops
/// non-music results (see `candidates::filter_non_music`) and tracks
/// already known unavailable (see `ScoringContext::unavailable_tracks`),
/// scores, shuffles. Same partial-failure rule as before: the last error is
/// propagated only if every query across every mood in the mix failed —
/// a mood whose results are entirely filtered out as unavailable simply
/// contributes zero candidates, not an error.
pub async fn generate_mixed_candidates<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    moods: &[(&Mood, u8)],
    resolver: &Resolver,
    ctx: &ScoringContext,
    config: &GenerationConfig,
    rng: &mut impl Rng,
) -> Result<Vec<Track>> {
    let total_budget = config.queries_per_round * moods.len().max(1);
    let weights: Vec<u8> = moods.iter().map(|(_, weight)| *weight).collect();
    let counts = candidates::query_counts_for_weights(total_budget, &weights);

    let mut raw = Vec::new();
    let mut last_err = None;
    for ((mood, _), count) in moods.iter().copied().zip(counts) {
        for query in candidates::select_queries(mood, count, rng) {
            match resolver.search(app, &query, config.results_per_query).await {
                Ok(tracks) => raw.extend(tracks),
                Err(err) => last_err = Some(err),
            }
        }
    }

    if raw.is_empty()
        && let Some(err) = last_err
    {
        return Err(err);
    }

    Ok(rank_candidates(raw, ctx, rng))
}

/// The pure, network-free tail of `generate_mixed_candidates`: dedup, drop
/// non-music results the search terms pulled in anyway, drop
/// known-unavailable tracks, score and shuffle. Split out so this logic is
/// unit-testable without spawning the yt-dlp sidecar (see the `#[ignore]`d
/// smoke test below for the full-flow, real-network coverage).
fn rank_candidates(raw: Vec<Track>, ctx: &ScoringContext, rng: &mut impl Rng) -> Vec<Track> {
    let deduped = candidates::dedup(raw);
    let musical = candidates::filter_non_music(deduped);
    let available =
        candidates::filter_out_unavailable(musical, |id| ctx.unavailable_tracks.contains(id));
    scoring::shuffle_by_score(available, ctx, rng)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Track;
    use rand::SeedableRng;
    use rand::rngs::StdRng;

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
    fn build_scoring_context_reflects_persisted_signals() {
        let db = Db::open_in_memory().unwrap();
        db.set_track_feedback(&track("liked"), true).unwrap();
        db.favorite_track(&track("fav")).unwrap();
        db.mark_track_unavailable("dead", "region_blocked").unwrap();

        let ctx = build_scoring_context(&db, 5).unwrap();
        assert_eq!(ctx.feedback.get("liked"), Some(&true));
        assert!(ctx.favorited_tracks.contains("fav"));
        assert!(ctx.unavailable_tracks.contains("dead"));
    }

    #[test]
    fn build_scoring_context_on_a_fresh_database_is_all_empty() {
        let db = Db::open_in_memory().unwrap();
        let ctx = build_scoring_context(&db, 5).unwrap();
        assert!(ctx.feedback.is_empty());
        assert!(ctx.favorited_tracks.is_empty());
        assert!(ctx.recently_played.is_empty());
        assert!(ctx.unavailable_tracks.is_empty());
    }

    #[test]
    fn rank_candidates_drops_tracks_marked_unavailable() {
        let mut ctx = ScoringContext::default();
        ctx.unavailable_tracks.insert("dead".into());
        let raw = vec![track("alive"), track("dead")];
        let mut rng = StdRng::seed_from_u64(1);

        let result = rank_candidates(raw, &ctx, &mut rng);

        let ids: Vec<_> = result.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["alive"]);
    }

    #[test]
    fn rank_candidates_keeps_everything_when_nothing_is_unavailable() {
        let ctx = ScoringContext::default();
        let raw = vec![track("a"), track("b"), track("c")];
        let mut rng = StdRng::seed_from_u64(1);

        let result = rank_candidates(raw, &ctx, &mut rng);

        let mut ids: Vec<_> = result.iter().map(|t| t.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }
}

/// Real network smoke test — not run by default, see media/resolver.rs's
/// smoke tests for the dev binary setup this depends on.
#[cfg(test)]
mod smoke_tests {
    use super::*;
    use crate::media::resolver::{Resolver, ResolverConfig};
    use crate::moods::MoodCatalog;
    use rand::SeedableRng;
    use rand::rngs::StdRng;
    use std::path::Path;
    use std::time::Duration;

    /// A real (if fake-runtime) `AppHandle` with the shell plugin
    /// registered — see `media::resolver::smoke_tests::test_app_handle`
    /// for why the plugin registration matters.
    fn test_app_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_shell::init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock tauri app should build")
            .handle()
            .clone()
    }

    #[tokio::test]
    #[ignore]
    async fn generating_candidates_for_a_real_mood_returns_deduped_scored_tracks() {
        let dev_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        let resolver = Resolver::new(ResolverConfig {
            deno_path: dev_dir.join("deno"),
            timeout: Duration::from_secs(30),
        });
        let app = test_app_handle();
        let catalog = MoodCatalog::load().unwrap();
        let mood = catalog
            .get("villain")
            .expect("the bundled catalog should have a 'villain' mood");

        let ctx = ScoringContext::default();
        let config = GenerationConfig::default();
        let mut rng = StdRng::seed_from_u64(1);

        let candidates =
            generate_mixed_candidates(&app, &[(mood, 100)], &resolver, &ctx, &config, &mut rng)
                .await
                .unwrap();

        assert!(!candidates.is_empty());
        let mut ids: Vec<_> = candidates.iter().map(|t| t.id.clone()).collect();
        let before_dedup_len = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(
            ids.len(),
            before_dedup_len,
            "candidates should already be deduped"
        );
    }
}
