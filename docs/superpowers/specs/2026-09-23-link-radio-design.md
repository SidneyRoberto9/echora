# Link Radio — Design

Date: 2026-09-23
Status: approved in chat, pending spec review

## Goal

On Home, the user pastes a YouTube link. Echora starts a session seeded
by that track and keeps queueing similar music indefinitely, until the
user pastes another link or picks a mood (either starts a new session,
which already replaces the current one).

## Decisions (from brainstorming)

- **Input:** links only — `youtube.com/watch?v=`, `youtu.be/`,
  `music.youtube.com/watch?v=` (with or without `www.`/`m.`, extra
  params like `&t=`, `&si=`, `&list=` ignored). No free-text search.
- **History:** a link session is a normal session: persisted, shown in
  Library as `Mix · <seed title>`, its plays count toward stats and
  scoring signals.
- **Similarity source:** YouTube's own auto-generated Mix, fetched with
  the bundled yt-dlp sidecar:
  `https://www.youtube.com/watch?v=<ID>&list=RD<ID>` with
  `--flat-playlist`. Probed 2026-09-23: returns ~25-50 related tracks in
  ~2s. No new dependency, no API key, no extra network service.
- **Infinite:** top-up re-seeds the Mix from the last track in the
  queue; falls back to the original seed if every result is a duplicate.
- **Order:** Mix order is preserved (no `rank_candidates` shuffle) —
  that order is what carries the similarity.

## Backend (Rust)

### Link parsing — `media::metadata::youtube_id_from_link(&str) -> Result<String>`

Pure function. Trims input, parses scheme+host+path+query by hand (no
new crate unless `url` is already a direct dependency). Accepts hosts
`youtube.com`, `www.youtube.com`, `m.youtube.com`, `music.youtube.com`
(path `/watch`, id from `v=`) and `youtu.be` (id = first path segment).
Result must pass `is_valid_youtube_id`. Anything else →
`EchoraError::InvalidInput("That's not a YouTube link.")` (reuse an
existing validation variant if one fits; add one otherwise).

### `Resolver::radio(app, seed_id, limit) -> Result<Vec<Track>>`

Same shape as `search`: builds the URL only from a validated id, passes
arguments as an array (`--js-runtimes deno:…`, url, `--flat-playlist`,
`--dump-json`, `--no-warnings`, `--playlist-end <limit>`), parses each
line with `metadata::parse_search_result`. Rejects an invalid id before
spawning yt-dlp.

### Persistence — migration `0005_link_sessions.sql`

```sql
ALTER TABLE sessions ADD COLUMN seed_track_id TEXT REFERENCES tracks (id);
```

A link session has `seed_track_id` set and zero `session_moods` rows.
The seed `Track` is upserted into `tracks` when the session starts so
its title is available via join.

`SessionInfo` and `SessionSummary` gain `seed: Option<Track>`
(serialized to the frontend; `None` for mood sessions). `Db` gains
`start_link_session(&Track) -> Result<SessionInfo>`; `current_session`
and `list_sessions` populate `seed`. Mood-only queries
(`recent_mood_ids`, `mood_play_counts`) are unaffected since link
sessions have no mood rows.

### Candidate filtering — `mood_engine::link_radio` (new small module)

`fn filter_radio(raw, exclude_ids, ctx) -> Vec<Track>`: pure, keeps Mix
order; applies existing `dedup`, `filter_out_unavailable`,
`filter_non_music`, drops disliked tracks (from `ScoringContext`) and
any id in `exclude_ids` (everything already queued/played this session
— `Queue` exposes its full id set).

`fn next_seed(queue_view, original_seed) -> String`: last upcoming track,
else current, else original seed.

### Commands

- `start_link_session(url: String) -> SessionInfo` (new, registered in
  `capabilities/default.json`):
  1. parse id; 2. `radio(id)` — fetched **before** touching the current
  session, so a failure leaves current playback untouched;
  3. seed track = the Mix entry with the seed id (first entry), else
  error "Couldn't build a mix from this track."; 4. end current
  session, clear queue, `db.start_link_session(seed)`; 5. queue seed +
  `filter_radio(rest)`; 6. `resolve_and_load` the seed (a
  `TrackUnavailable` there surfaces via the existing message).
- `ensure_queue_topped_up` (existing): if the current session has a
  seed, call a new `top_up_link_queue` (radio from `next_seed`, filter,
  and if nothing survives retry once from the original seed); otherwise
  the existing mood path.

Starting a mood (or another link) already ends the session → radio stops.

## Frontend

- `api.startLinkSession(url)`; `SessionInfo`/`SessionSummary` TS types
  gain `seed: Track | null`.
- `HomeView`: new row directly below "Surprise Me": text input
  (placeholder "Paste a YouTube link to start a mix", link icon) +
  "Start mix" button; Enter submits. Disabled while starting (reuses
  the `busy` state), clears on success. `App.tsx` gets `onStartLink`,
  mirroring `onStartMood` (same navigation/refresh, errors →
  `ErrorBanner`).
- `LibraryTab`: sessions with a `seed` render `Mix · <seed title>` and
  replay via `startLinkSession` with the seed's watch URL (currently it
  indexes `session.moods[0]`, which would crash on an empty list).
- Anywhere else that assumes `session.moods.length > 0` (e.g.
  `setCurrentMoods` in `App.tsx`) is handled so an empty list renders the
  seed label instead.

## Errors

| Case | Result |
|---|---|
| Not a YouTube link | "That's not a YouTube link." (Rust-side) |
| Private/removed/region-blocked seed | existing `TrackUnavailable` message |
| Empty Mix / yt-dlp timeout at start | "Couldn't build a mix from this track."; current session untouched |
| Top-up failure mid-session | same as mood top-up today: queue stops growing, next top-up retries |

## Testing

Rust unit tests: link parsing (3 host forms, `www`/`m.` variants, extra
params, rejects other domains, injected characters, wrong-length id,
empty); `filter_radio` (order kept, excludes session ids, disliked,
unavailable); `next_seed` (upcoming → current → original);
migration 0005 over a pre-0005 DB; `start_link_session` DB round-trip
(`current_session`/`list_sessions` return `seed`). One `#[ignore]`d
real-yt-dlp test for `radio`, like the existing resolver tests.

Gates: `npm run lint && npm run build`; `cargo fmt --check && cargo
clippy --all-targets -- -D warnings && cargo test`. Live-app check is
done by the user (not launched by the agent).

## Out of scope

Free-text search, playlist links as seeds, "similarity" tuning,
mixing a link with moods.
