# Requirements Freeze

Locked product decisions, agreed with the maintainer before implementation
started. Treat these as immutable during implementation unless the
maintainer explicitly requests a change.

## Platform & distribution

- Linux only for v1 (Ubuntu/Zorin-based). No Windows/macOS yet.
- Architectures: `x86_64` only. ARM64 was dropped on 2026-09-06 — no
  ARM users to serve, and the ARM leg was never actually verified on a
  runner. The build scripts stay architecture-parameterized, so this
  comes back by re-adding the matrix entry when someone asks for it.
- "Zero install" means a genuinely self-contained package: no
  dependency on the user running `apt install` for anything Echora
  needs, beyond installing Echora's own package.
- Distribution formats: both `.deb` and AppImage.
- Auto-update ships in v1 (Tauri's official updater plugin, signed
  releases; the private signing key is never committed).

## Git & hosting

- GitHub, personal account `SidneyRoberto9`.
- Public repository, named `echora`.
- Branch `main`.

## License

- PolyForm Strict License 1.0.0 + a custom addendum (end-user run rights
  for official releases, temporary contribution-only forks). See
  `LICENSE`.
- Copyright holder: `SidneyRoberto9`, year 2026.
- Third-party forks for contribution are allowed; no formal CLA is
  required beyond the inbound license grant in the LICENSE addendum.

## V1 scope

**In:** auto-update, Scenes, Mood Mixing, Discover, Statistics,
SponsorBlock, autostart on system startup.

**Out:** download/offline mode, Smart Search (removed from scope),
free-text search UI (removed from scope — mood-driven search remains the
only discovery path; the resolver's `search()` still powers mood
candidate generation internally), Intensity (removed from scope — never
had a concrete design; `MoodTraits` on each catalog mood — energy,
darkness, romance, sadness, aggression, focus — remain unused data, not
wired to any scoring or query-selection behavior).

Being "in v1" doesn't mean built first — the internal build order
follows the phases in the main project brief: the core mood → search →
resolve → queue → playback → background-controls → history loop lands
first; Scenes, Mood Mixing, Discover, Statistics,
SponsorBlock, and autostart layer on afterward, once the core is
validated.

## Product behavior

- UI language: English for v1.
- Closing the window minimizes to tray (does not quit).
- Launching the app (manual or via autostart) starts minimized to tray,
  no window flash.
- Search is YouTube-only for v1.
- Audio quality: best cost/benefit — good quality, minimal resource use;
  not "always the absolute best available."
- Cache: deferred. No audio cache layer exists yet, so the Settings
  control for it was removed in the 2026-09-06 audit remediation rather
  than left as a knob that changes nothing. When a real cache lands, the
  limit comes back as 500MB by default, adjustable (250MB / 500MB / 1GB
  / 2GB / Unlimited).
- History: retained indefinitely by default, clearable and disableable
  in Settings.
- Telemetry: none by default. The one exception is a fully manual,
  opt-in crash report — a local log plus a button that opens a
  pre-filled GitHub issue in the user's browser. No automatic network
  call, no third-party SDK.

## Appearance

- Dark theme only.
- Palette: black with a light purple accent; the rest is left to visual
  design work.
- Minimalist player, not a large visual/immersive layout.

## Known risks, tracked (not blockers)

- A genuinely self-contained mpv binary doesn't exist upstream, so
  Echora builds it from source in CI. See
  `docs/adr/0007-arm64-native-ci-and-mpv-build.md` — that ADR also
  records why the ARM64 half of it is no longer built.
