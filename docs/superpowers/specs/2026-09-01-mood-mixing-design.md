# Mood Mixing — Design

Status: Approved. Second of the remaining Fase 7 features (Mood Mixing,
Intensity, SponsorBlock) to get a full spec → plan → implementation cycle,
per `docs/REQUIREMENTS_FREEZE.md`'s V1 scope. Smart Search was removed from
scope before this spec was written.

## Purpose

Let the listener start a session blended from 2–3 moods at once (e.g. 70%
Villain / 30% Chill) instead of picking exactly one. Weighting controls how
many search queries each mood contributes to the candidate pool for the
session; everything downstream (dedup, scoring, shuffle, playback, history)
is unchanged.

## Non-goals (v1)

- No mid-session mood switching or re-weighting — mix is chosen once, at
  session start, same as picking a single mood today.
- No trait blending — scoring stays global (feedback/favorites/recency via
  `ScoringContext`), not a synthetic combined `MoodTraits` vector.
- No mixing via Surprise Me — that still picks exactly one mood at random.
- Scenes are unaffected — `SceneSummary` has no mood reference today and
  gets none added; a saved scene is just a frozen track list regardless of
  how the session that built it was seeded.

## Backend (Rust)

### Migration: `src-tauri/migrations/0003_mixed_sessions.sql`

```sql
CREATE TABLE session_moods (
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    mood_id TEXT NOT NULL,
    weight INTEGER NOT NULL CHECK (weight > 0 AND weight <= 100),
    PRIMARY KEY (session_id, mood_id)
);

INSERT INTO session_moods (session_id, mood_id, weight)
SELECT id, mood_id, 100 FROM sessions;

ALTER TABLE sessions DROP COLUMN mood_id;
```

Every session — single-mood or mixed — has one or more rows here. No dual
representation, no "mood_id if single else session_moods" branching in
Statistics or history reads.

### `src-tauri/src/models.rs`

```rust
#[derive(Debug, Clone, Serialize)]
pub struct SessionMood {
    pub mood_id: String,
    pub weight: u8,
}

pub struct SessionInfo {
    pub id: i64,
    pub moods: Vec<SessionMood>,   // was mood_id: String
    pub started_at: i64,
    pub ended_at: Option<i64>,
}

pub struct SessionSummary {
    pub id: i64,
    pub moods: Vec<SessionMood>,   // was mood_id: String
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub track_count: u32,
}
```

`MoodPlayCount` keeps `mood_id: String` but `play_count` becomes a weighted
sum (see below) rather than a raw row count.

### `src-tauri/src/db/sessions.rs`

- `start_session(&self, mood_id: &str)` → `start_session(&self, moods: &[(String, u8)])`.
  Validates `1..=3` entries, weights sum to exactly 100, no duplicate
  `mood_id` — returns `Error::InvalidInput` (existing variant) otherwise.
  Inserts the session row, then one `session_moods` row per entry in the
  same transaction.
- `recent_mood_ids` — join `session_moods`, unchanged semantics (still "any
  mood id seen in the last N sessions").
- `mood_play_counts` — query changes from `GROUP BY mood_id, COUNT(*)` to
  `GROUP BY mood_id, SUM(weight) / 100.0`, so a 70/30 mixed session credits
  0.7 plays to one mood and 0.3 to the other instead of a full play to
  both. `MoodPlayCount.play_count` becomes the summed fraction (still
  ordered desc for the ranking — ties resolved as today).
- `list_sessions` / row-to-`SessionSummary` mapping — one extra join +
  group to collect each session's `Vec<SessionMood>`.

### `src-tauri/src/mood_engine/mod.rs`

```rust
pub async fn generate_mixed_candidates(
    moods: &[(Mood, u8)],           // 1-3 entries, weights sum to 100
    resolver: &Resolver,
    ctx: &ScoringContext,
    config: &GenerationConfig,
    rng: &mut impl Rng,
) -> Result<Vec<Track>> {
    let total_budget = config.queries_per_round * moods.len().max(1);
    let mut raw = Vec::new();
    let mut last_err = None;

    for (mood, weight) in moods {
        let share = ((total_budget as f32) * (*weight as f32) / 100.0).round() as usize;
        let count = share.max(1);
        for query in candidates::select_queries(mood, count, rng) {
            match resolver.search(&query, config.results_per_query).await {
                Ok(tracks) => raw.extend(tracks),
                Err(err) => last_err = Some(err),
            }
        }
    }

    if raw.is_empty() && let Some(err) = last_err {
        return Err(err);
    }

    Ok(scoring::shuffle_by_score(candidates::dedup(raw), ctx, rng))
}
```

`generate_candidates` (single-mood) stays as-is and keeps being the path
`top_up_queue` uses for topping up an already-running session — mixing only
applies to the initial candidate batch at session start (non-goal: no
re-weighting mid-session, so subsequent top-ups don't need mix awareness
either; they use the session's already-established mood pool via a new
`top_up_mixed_queue` that reuses the same per-mood share logic instead of
`generate_candidates`, called with the session's stored `session_moods`).

`select_queries` and `dedup`/`shuffle_by_score` are reused unchanged.

### `src-tauri/src/commands/mod.rs` and `commands/session.rs`

- `start_session_and_play(state, mood_id: &str)` →
  `start_session_and_play(state, moods: &[(String, u8)])`; single-mood
  callers (`start_session` Tauri command, Surprise Me) pass a one-element
  slice `[(mood_id, 100)]`.
- `top_up_queue(state, mood_id: &str)` gets a sibling
  `top_up_mixed_queue(state, moods: &[(String, u8)])` used when the active
  session has more than one mood; `start_session_and_play` picks whichever
  applies based on `moods.len()`.
- New Tauri command `start_mixed_session(state, moods: Vec<MoodWeight>)`
  where `MoodWeight { mood_id: String, weight: u8 }` — thin wrapper,
  validates via the same `db.start_session` path. The existing
  `start_session(mood_id: String)` command stays for the single-mood UI
  path (Home tap-to-start, Surprise Me) — no behavior change there.
- `build_listening_stats`: `top_mood_id` now reads off the (already
  weighted) `mood_play_counts` result — no change to this function's body,
  just picks up the new weighting from `db::sessions::mood_play_counts`.

## Frontend (React)

- `HomeView`: new "Mix moods" toggle button next to the Surprise Me banner.
  Toggling it switches `MoodCard` taps from `onStartMood(id)` (immediate
  start) to an accumulating local selection (`Set<string>`, capped at 3).
- New `MoodMixBar` component: fixed bottom bar, shown only when 1+ moods
  are selected in mix mode. Displays the selected moods as chips with a
  drag-adjustable weight split (3-segment bar, default even split, always
  sums to 100 by construction — dragging one edge takes proportionally
  from neighbors) and a "Start Mix" button (disabled below 2 selections).
- `src/lib/api.ts`: add `startMixedSession(moods: {moodId: string; weight: number}[])`
  wrapping the new command; `SessionInfo`/`SessionSummary` types gain
  `moods: {moodId: string; weight: number}[]` replacing `moodId: string`.
- Call sites reading `session.moodId` (PlayerView header, session history
  rows in Discover's Library tab) switch to `session.moods`: single-mood
  sessions render the one mood's name as today; 2–3 render
  `"Villain + Chill"` (names joined by category-appropriate label lookup,
  already-existing `CATEGORY_LABEL`/mood name resolution, no new lookup
  logic needed).

## Error handling / edge cases

- Weight sum ≠ 100, count outside 1–3, or a duplicate mood id: rejected by
  `db.start_session`'s validation before any network call — surfaced as the
  existing `InvalidInput` error path (same toast/error handling the app
  already has for other command errors).
- All queries across all mixed moods fail (no internet, sidecar down): same
  rule as today — error propagates only if the whole round comes back
  empty; a partial batch from just one of the moods is still a valid
  session start.
- A mood id that doesn't exist in the catalog: rejected the same way single
  `start_session` already rejects an unknown `mood_id` today (via
  `state.moods.get(mood_id)?`), applied per entry.

## Testing

- Rust:
  - `mood_engine::generate_mixed_candidates`: query distribution across 3
    unevenly-weighted moods never leaves a mood at 0 queries; dedup/shuffle
    behavior matches the existing single-mood tests.
  - `db::sessions::start_session`: rejects weight sums ≠ 100, rejects >3 or
    duplicate mood ids, persists and reads back `session_moods` correctly
    (including the single-mood `[(id, 100)]` case, which every existing
    single-mood test now exercises through the new signature).
  - `db::sessions::mood_play_counts`: mixed session credits fractional
    weight to each mood; existing single-mood ranking tests keep passing
    unchanged (weight 100 = 1 full play, same as before).
  - Migration test: `0003_mixed_sessions.sql` applies cleanly on top of
    `0001_init.sql` + `0002_scenes.sql` and backfills existing rows.
- Frontend: no new test framework — `tsc`/lint/build, matching project
  convention (see Discover/Scenes specs).
