# Audio-reactive orb — Design

Status: Approved (sections 1 and 2 confirmed with the user). Not yet
implemented.

## Purpose

The player screen's orb (`.orb-wrap`/`.orb-ring`/`.orb` in
`src/components/PlayerView.tsx` + `src/styles.css`) currently plays a
fixed 3.6s CSS `echora-pulse` keyframe animation that ignores playback
entirely — it runs identically whether a track is playing, paused, or no
track is loaded at all. This task makes it react to the actual audio
level of what's playing, and stop consuming CPU/IPC when nobody can see
it.

## Key constraint discovered during research

mpv exposes no built-in IPC property for audio level. Getting one
requires attaching an FFmpeg audio filter (`astats`, via mpv's `af`)
and reading its metadata over IPC (`get_property af-metadata/<label>`,
returning e.g. `lavfi.astats.Overall.RMS_level`). This works and updates
continuously as audio plays, but:

- No prior art exists for driving an *external UI* off this data — every
  documented mpv audio-visualizer renders inside mpv's own video output
  (`lavfi-complex` + `showspectrum`/`showcqt`), which doesn't apply here
  (Echora runs `--no-video`, audio-only). This is a first-of-its-kind
  integration for this specific pattern, not a known-good recipe.
- `observe_property` is documented as unsuitable for high-frequency
  properties (mpv issue #5661 — stresses the IPC). Active polling via
  repeated `get_property` calls is the recommended approach instead.
- The alternative `ebur128` (loudness) filter has an unresolved upstream
  issue (#2311) about unreliable metadata reads — `astats` is the safer
  choice of the two.

Given this, the design deliberately treats live audio-reactivity as
**best-effort, with the existing static pulse as the permanent fallback**
— not a "make it work or crash" requirement.

## Non-goals

- No real spectrum/waveform visualization — a single scalar level
  (loudness), not multi-band frequency data.
- No `observe_property`-based push updates — active polling only, per
  the mpv #5661 finding above.
- No change to the orb's shape, layout, or base aesthetic — the existing
  gradient/ring design and `echora-pulse` keyframe stay as the visual
  floor; audio-reactivity modulates on top of it, it doesn't replace it.
- Not required to work identically across every possible audio source —
  if a track's stream never gets the filter attached successfully (rare,
  but possible), that track just keeps the static pulse. No retry loop,
  no error surfaced to the user.

## Backend (Rust) — `src-tauri/src/media/player.rs` and friends

1. **Attach the filter per-track**, not as a permanent startup flag: after
   `loadfile`, send `af add @echora_level:lavfi=[astats=metadata=1:reset=1]`
   over the existing one-shot IPC connection. If this command errors,
   treat the track as having no live-level data for its whole duration
   (see Fallback below) — don't retry.

2. **Background polling task**: a Tokio task that, while gated open (see
   next point), calls `get_property af-metadata/echora_level` roughly
   12-15 times/second, reading `lavfi.astats.Overall.RMS_level` (dB,
   typically a very negative number down to `-inf` at silence), and
   normalizes/clamps it to a `0.0..=1.0` f32 (pure function, unit-tested
   in isolation — no mpv/IPC needed to test the math).

   Uses the same one-shot-per-request IPC pattern the rest of `Player`
   already uses, **unless** the in-flight auto-advance work (tracked
   separately, running in parallel) ends up adding a persistent IPC
   connection for `observe_property`-based end-of-file detection — if it
   does, this polling loop should reuse that connection rather than
   opening a second one. Whoever implements this must check the current
   state of `player.rs` at implementation time, not assume this document
   is still accurate about what connection pattern exists.

3. **Gating (the actual efficiency win, not just a visual nicety)**: the
   poll loop only runs while *all* of the following hold — a track is
   actively playing (not paused/idle) **and** the window is both visible
   (not minimized) **and** focused. It must stop the task entirely when
   any of those go false, not merely skip emitting — this is what keeps
   the IPC chatter and CPU cost at zero the rest of the time. Window
   visibility/focus state comes from Tauri's window events
   (`WindowEvent::Focused`, plus minimize state); playing/paused state is
   whatever `AppState` already tracks.

4. **Emit to frontend**: `app.emit("audio-level", level: f32)` on each
   successful poll. No event fires when gated closed or when a track has
   no live-level data — the frontend never needs to know *why* nothing is
   arriving, only that nothing is.

5. **Fallback**: if `af add` fails, or `get_property` errors/returns null
   a few times in a row for the current track, stop polling for that
   track (don't retry, don't error, don't tell the frontend) — the orb
   simply keeps running its existing static `echora-pulse` CSS animation,
   which never depended on this data to begin with.

## Frontend — `src/components/PlayerView.tsx`, `src/styles.css`

1. **Listen without re-rendering**: `listen("audio-level", ...)` inside
   `PlayerView`, writing each value into a CSS custom property
   (`--orb-level`) directly on the orb's DOM node via a `ref`, not via
   `useState` — at 12-15Hz, driving React state would cause unnecessary
   re-renders (Lightness/efficiency is priority #1 in this project).
   Apply a simple smoothing pass (e.g. exponential moving average) before
   writing, so the visual doesn't look jittery frame-to-frame.

2. **Visual mapping**: `--orb-level` (0..1) modulates scale and glow/
   opacity **on top of** the existing `echora-pulse` keyframe — it's
   never the only thing animating the orb, so silence/no-data still looks
   like the current gentle breathing pulse rather than a dead circle.
   Sketch: `transform: scale(calc(1 + var(--orb-level, 0) * 0.15))` on
   `.orb`, similar treatment on `.orb-ring`'s opacity — exact coefficients
   are a tuning pass during implementation, not fixed by this spec.

3. **Cleanup**: the event listener unregisters on `PlayerView` unmount and
   whenever the current track changes (a stale listener from a previous
   track must not keep writing into a new track's orb).

## Testing

- Rust: pure unit tests for (a) the dB→`0.0..=1.0` normalization/clamp
  function and (b) the "should the poll loop be running right now"
  predicate (playing ∧ visible ∧ focused) — both testable without a real
  mpv process, matching the existing separation of testable logic from
  IPC glue already used elsewhere in `commands/` and `player.rs`.
- A real-mpv smoke test for the `af add` + `af-metadata` read round-trip,
  `#[ignore]`d like `player.rs`'s existing real-sidecar tests (not run in
  CI's fast lane, run manually / in the `build` job's environment).
- No new frontend test tooling — this project has none (vitest isn't
  installed); manual verification via the running app is the existing
  precedent for frontend changes.

## Open coordination note

This was brainstormed while a separate, parallel change (auto-advance to
the next track on natural end-of-file) was being implemented against the
same `player.rs`/`AppState`. Before implementation starts on this spec,
re-check `player.rs` for what that work landed — specifically whether it
introduced a persistent IPC connection or any polling/background-task
infrastructure this feature should share instead of duplicating.
