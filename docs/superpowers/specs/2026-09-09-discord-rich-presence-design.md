# Discord Rich Presence — Design

Status: Implemented.

## Purpose

Show the currently playing track as a Discord "Listening to Echora"
activity status, the way Spotify does.

## Context

The request originated from a misunderstanding worth recording:
Discord's own "Compartilhar minha atividade" privacy toggle only
controls whether Discord *shows* activity to friends — it does nothing
on its own. Echora sends no presence data today, so nothing appears
regardless of that toggle. Making anything show up requires Echora to
actively speak Discord's local Rich Presence IPC protocol.

## Key decisions confirmed with the maintainer

- **Content**: track title + artist + an elapsed/remaining timestamp,
  matching Spotify's presentation.
- **Reconnect policy**: periodic background retry (~20-30s) while the
  feature is enabled and Discord isn't reachable, so opening Discord
  after Echora still picks up presence without restarting Echora.
- **Paused behavior**: keep the presence visible, showing "Paused"
  (not cleared).
- **Discord application**: already registered by the maintainer.
  Client ID `1547402077863542914` is public (embedded in the protocol
  handshake itself, not a secret) and is hardcoded as a constant. The
  Client Secret is unused — basic Rich Presence is a local IPC
  handshake, no OAuth flow involved.

## Approach: hand-rolled IPC client, no new crate

Discord's Rich Presence transport is a Unix domain socket
(`$XDG_RUNTIME_DIR/discord-ipc-0`, falling back to `/tmp/discord-ipc-0`
and similar `discord-ipc-{0..9}` paths) carrying frames of an 8-byte
header (4-byte little-endian opcode + 4-byte little-endian length)
followed by a JSON payload. A handshake frame (opcode `0`,
`{"v":1,"client_id":...}`) is sent once per connection; activity
updates are opcode `1` frames
(`{"cmd":"SET_ACTIVITY","nonce":...,"args":{"pid":...,"activity":{...}}}`).

`tokio` (already a dependency, `net` feature already enabled) and
`serde_json` (already a dependency) are sufficient to implement this
directly — no new crate needed. This means no
`licensing-compliance-reviewer` pass and no `THIRD_PARTY_NOTICES.md`
entry.

**Rejected alternative**: a ready-made crate (e.g.
`discord-rich-presence`). It would save roughly 150-250 lines of
protocol code that has been stable for years, at the cost of a new
third-party dependency, a licensing review, and extra surface in the
dependency tree — against the project's "don't add a dependency just
in case" rule and its #1 priority (lightness).

## Non-goals

- **No album art** (`large_image`/`small_image`). Discord's classic
  Rich Presence assets require pre-uploading fixed image *keys* in the
  Developer Portal, not arbitrary per-track URLs. Out of scope for v1.
- **No per-second refresh of the elapsed timer.** Discord renders
  `timestamps.start` client-side and ticks it locally; Echora only
  sends an update on real events (track change, play, pause, seek).
- **No reliance on Discord's automatic activity detection**
  (`detectable.json` process/window-title matching). It carries no
  track/artist data, isn't under Echora's control, and doesn't satisfy
  "show what's playing."
- **No non-Linux socket paths.** Echora is Linux-only; only the
  XDG-runtime/tmp Unix socket conventions apply.

## Backend (Rust)

### 1. Consolidate the playback-change broadcast point

Today, `platform::mpris::notify()` is called from roughly seven
scattered call sites: `platform/mpris.rs`'s inbound MPRIS handlers
(Pause/Stop/Play/Seek/SetPosition), `commands/mod.rs::resolve_and_load`
(the shared entry point behind track advance, `queue_previous`,
`queue_skip_to`, `play_single_track`, `play_scene_impl`, and
session-start's first track), `commands/mod.rs::toggle_play_pause`, and
`commands/playback.rs`'s `pause_playback`/`resume_playback`/`seek_playback`.

It is **missing** from `end_session_impl` (`commands/session.rs`) and
from app shutdown (`lib.rs`'s `RunEvent::Exit`) — the same two gaps
already found and fixed today for `record_current_completion`.

Introduce `notify_playback_changed(app: &AppHandle, state: &AppState)`
that calls `mpris::notify(...)` and then `discord::notify(...)`.
Replace all existing `mpris::notify` call sites with it, and add the
two missing calls at `end_session_impl` and shutdown. Fixing MPRIS's
pre-existing gap here is a direct side effect of giving Discord correct
coverage at the same two points, not an unrelated refactor.

### 2. New module `platform/discord.rs`

Mirrors `platform/mpris.rs`'s shape:

- `const DISCORD_CLIENT_ID: &str = "1547402077863542914";`
- `pub struct Handle { tx: tokio::sync::watch::Sender<PresenceState> }`,
  returned by `pub fn spawn() -> Handle`, called once from `lib.rs`
  setup unconditionally — the spawned task itself checks the enabled
  flag before doing any IO, so startup never depends on Discord being
  present.
- `enum PresenceState { Disabled, Playing { title, artist, position_secs, duration_secs }, Paused { title, artist }, Idle }`
  (`Idle` = feature enabled but nothing loaded, e.g. after
  `end_session_impl` — clears the activity).
- The background task owns an optional `UnixStream`, a reconnect
  interval, and the `watch::Receiver<PresenceState>`. It `select!`s
  between the receiver changing and the reconnect timer ticking. When a
  new non-`Disabled` state arrives while disconnected, it attempts
  connect + handshake; while connected, it builds the sanitized
  `SET_ACTIVITY` payload and writes the frame. Any write/parse error is
  treated as "connection lost" — drop the stream, log once, fall back
  to the retry timer — never propagated as a crash.
- `pub fn notify(handle: &Handle, state: &AppState)` reads the same
  queue/player state `mpris::notify` already reads, maps it to a
  `PresenceState`, and sends it through the `watch` channel — which
  naturally coalesces to the latest value, giving free debounce for
  rapid skip clicks with no extra buffering logic.
- `pub fn set_enabled(handle: &Handle, enabled: bool)`, called from
  `update_settings`'s side-effect step (same pattern as
  `autostart::sync`). Disabling sends `PresenceState::Disabled`, which
  makes the task clear the activity (best-effort `SET_ACTIVITY` with
  `activity: null`) and close the socket.

### 3. Sanitization (pure function, unit-testable)

`fn sanitize_field(s: &str, max_bytes: usize) -> String` strips control
characters, collapses whitespace runs, truncates at a UTF-8 char
boundary not exceeding `max_bytes` (128 — Discord's `details`/`state`
limit), and falls back to `"Echora"` if the result is empty. Applied to
title and artist before building any payload — this is the
untrusted-YouTube-metadata boundary the project's security rules call
out.

### 4. Settings

`Settings` (`models.rs`) gains `#[serde(default)] discord_presence_enabled: bool`
(default `false`, same pattern as `crash_report_enabled`/
`autostart_enabled`). `SettingsPatch` gains the matching
`Option<bool>` field. `update_settings`'s side-effect step calls
`discord::set_enabled(&state.discord, value)` alongside the existing
`autostart::sync` call.

## Frontend

One `Toggle` added to `SettingsView.tsx`, next to the other opt-in
integrations (autostart, crash reports), using the existing
`useSettings().update()` optimistic-patch pattern already used for
those — no new frontend logic.

## Error handling & degradation

- Discord not installed/running: connect fails, the task retries
  silently on the timer; not surfaced as an error (expected, not a
  fault).
- Socket present but handshake malformed/rejected: handled like any
  other IO error — drop, retry later, one log line per state
  transition (not per retry attempt), to avoid log spam.
- Disabling the setting while connected: best-effort clear, then close;
  if the clear write itself fails, no retry — cosmetic only (stale
  presence self-clears on Echora's next connect, or via Discord's own
  stale-RPC timeout).
- App shutdown: best-effort clear via the new `notify_playback_changed`
  call in `RunEvent::Exit`, with a short timeout so a stuck Discord
  socket can never hang app quit.

## Testing

- Unit tests for `sanitize_field` (multi-byte truncation boundary,
  empty-string fallback, control-character stripping) and for the
  frame-encoding function (header bytes match the given JSON payload's
  length) — both pure, no socket required.
- No automated test for the live handshake/connection (no Discord
  process in CI). Manual verification: enable with Discord open
  (status appears, correct title/artist/timer); enable with Discord
  closed, then open Discord (reconnects within the retry window);
  pause (shows "Paused"); skip rapidly (no flood/crash, final state
  wins); end session / quit app (presence clears).
- Standard gate before claiming done:
  `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
  (backend) plus `npm run lint && npm run build` (frontend, for the one
  new `Toggle`).

## Documentation deliverables (tracked for the implementation plan)

Not written as part of this spec — tracked here so the implementation
plan includes them, per the user's request to land this in the
project's own documentation:

- `docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md` — the
  crate-vs-hand-rolled decision, same shape as ADRs 0005/0006/0009.
- `docs/REQUIREMENTS_FREEZE.md` — add "Discord Rich Presence (opt-in)"
  to the V1 scope "In:" list, and note under "Telemetry" that this is a
  second manual opt-in exception alongside crash reports, with no data
  leaving the device beyond the local Discord client's own IPC socket.
