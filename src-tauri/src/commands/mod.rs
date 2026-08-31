pub mod library;
pub mod mood;
pub mod playback;
pub mod queue;
pub mod search;
pub mod session;
pub mod settings;

use rand::SeedableRng;
use rand::rngs::StdRng;

use crate::error::Result;
use crate::mood_engine::{self, GenerationConfig};
use crate::state::AppState;

/// Generates a fresh batch of candidates for `mood_id` and appends them to
/// the queue. Shared by starting a mood session and topping the queue back
/// up mid-session — both are "get more candidates for this mood," just
/// triggered at different times.
pub(crate) async fn top_up_queue(state: &AppState, mood_id: &str) -> Result<()> {
    let mood = state.moods.get(mood_id)?;
    let config = GenerationConfig::default();
    let ctx = {
        let db = state.db.lock().unwrap();
        mood_engine::build_scoring_context(&db, config.recent_session_window)?
    };

    // A plain `StdRng`, not the thread-local `rand::rng()` — Tauri's async
    // commands require `Send` futures, and `ThreadRng` (`Rc`-based) isn't.
    let mut rng = StdRng::from_rng(&mut rand::rng());
    let candidates =
        mood_engine::generate_candidates(mood, &state.resolver, &ctx, &config, &mut rng).await?;
    state.queue.lock().unwrap().add_candidates(candidates);
    Ok(())
}
