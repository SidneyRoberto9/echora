use std::collections::HashSet;

use tauri::State;

use crate::error::{EchoraError, Result};
use crate::media::metadata;
use crate::models::{SessionInfo, Track};
use crate::mood_engine::link_radio::{self, RADIO_FETCH_LIMIT};
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// Pulls the seed out of its own Mix (YouTube puts it first, but don't
/// rely on it). No seed in the Mix means YouTube had nothing for it.
pub(crate) fn split_seed(mut mix: Vec<Track>, seed_id: &str) -> Result<(Track, Vec<Track>)> {
    let index = mix
        .iter()
        .position(|t| t.id == seed_id)
        .ok_or(EchoraError::MixUnavailable)?;
    let seed = mix.remove(index);
    Ok((seed, mix))
}

fn scoring_context(state: &AppState) -> Result<mood_engine::scoring::ScoringContext> {
    let db = state.db.lock().unwrap();
    mood_engine::build_scoring_context(&db, GenerationConfig::default().recent_session_window)
}

/// Starts link radio from a pasted YouTube link. The Mix is fetched
/// before anything about the current session changes, so a bad link or
/// an empty Mix leaves whatever is playing untouched.
#[tauri::command]
pub async fn start_link_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<SessionInfo> {
    let seed_id = metadata::youtube_id_from_link(&url)?;
    // A yt-dlp timeout/garbled output at start reads as "no mix" to the
    // user; a private/removed seed keeps its specific message.
    let mix = state
        .resolver
        .radio(&app, &seed_id, RADIO_FETCH_LIMIT)
        .await
        .map_err(|err| match err {
            EchoraError::TrackUnavailable(_) => err,
            _ => EchoraError::MixUnavailable,
        })?;
    let (seed, rest) = split_seed(mix, &seed_id)?;

    let ctx = scoring_context(&state)?;
    let exclude: HashSet<String> = [seed.id.clone()].into();
    let upcoming = link_radio::filter_radio(rest, &exclude, &ctx);

    super::record_current_completion(&state).await?;
    let session = state.db.lock().unwrap().start_link_session(&seed)?;
    {
        let mut queue = state.queue.lock().unwrap();
        queue.clear();
        queue.add_candidates(std::iter::once(seed.clone()).chain(upcoming));
    }
    super::resolve_and_load(&app, &state, &seed).await?;
    Ok(session)
}

/// Link-radio counterpart of `top_up_queue`: next Mix from the newest
/// queued track; if everything in it was already heard this session,
/// one retry from the original seed, then give up quietly (next top-up
/// tries again).
pub(crate) async fn top_up_link_queue(
    app: &tauri::AppHandle,
    state: &AppState,
    original_seed_id: &str,
) -> Result<()> {
    let (seed_id, exclude) = {
        let queue = state.queue.lock().unwrap();
        let all = queue.all_tracks();
        let exclude: HashSet<String> = all.iter().map(|t| t.id.clone()).collect();
        (link_radio::next_seed(all, original_seed_id), exclude)
    };
    let ctx = scoring_context(state)?;

    let mut fresh = link_radio::filter_radio(
        state
            .resolver
            .radio(app, &seed_id, RADIO_FETCH_LIMIT)
            .await?,
        &exclude,
        &ctx,
    );
    if fresh.is_empty() && seed_id != original_seed_id {
        fresh = link_radio::filter_radio(
            state
                .resolver
                .radio(app, original_seed_id, RADIO_FETCH_LIMIT)
                .await?,
            &exclude,
            &ctx,
        );
    }
    state.queue.lock().unwrap().add_candidates(fresh);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn split_seed_takes_the_seed_out_of_the_mix() {
        let (seed, rest) = split_seed(vec![track("s"), track("a"), track("b")], "s").unwrap();
        assert_eq!(seed.id, "s");
        assert_eq!(
            rest.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn split_seed_finds_the_seed_anywhere_in_the_mix() {
        let (seed, rest) = split_seed(vec![track("a"), track("s")], "s").unwrap();
        assert_eq!(seed.id, "s");
        assert_eq!(rest.len(), 1);
    }

    #[test]
    fn split_seed_errors_when_the_mix_is_empty_or_lacks_the_seed() {
        for mix in [vec![], vec![track("a")]] {
            let err = split_seed(mix, "s").unwrap_err();
            assert!(matches!(err, EchoraError::MixUnavailable), "{err:?}");
        }
    }
}
