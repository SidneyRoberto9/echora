# Requirements Freeze

Locked product decisions, agreed with the maintainer before implementation
started. Treat these as immutable during implementation unless the
maintainer explicitly requests a change.

## Platform & distribution

- Linux only for v1 (Ubuntu/Zorin-based). No Windows/macOS yet.
- Architectures: `x86_64` and `ARM64`.
- "Zero install" means a genuinely self-contained package: no
  dependency on the user running `apt install` for anything Echora
  needs, beyond installing Echora's own package.
- Distribution formats: both `.deb` and AppImage, for both architectures.
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

**In:** auto-update, free-text search, Smart Search, Scenes, Mood
Mixing, Intensity, Discover, Statistics, SponsorBlock, autostart on
system startup.

**Out:** download/offline mode.

Being "in v1" doesn't mean built first — the internal build order
follows the phases in the main project brief: the core mood → search →
resolve → queue → playback → background-controls → history loop lands
first; Smart Search, Scenes, Mood Mixing, Intensity, Discover,
Statistics, SponsorBlock, and autostart layer on afterward, once the
core is validated.

## Product behavior

- UI language: English for v1.
- Closing the window minimizes to tray (does not quit).
- Launching the app (manual or via autostart) starts minimized to tray,
  no window flash.
- Search is YouTube-only for v1.
- Audio quality: best cost/benefit — good quality, minimal resource use;
  not "always the absolute best available."
- Default cache limit: 500MB, adjustable (250MB / 500MB / 1GB / 2GB /
  Unlimited).
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

- A genuinely self-contained mpv binary doesn't exist upstream for
  either architecture — Echora builds it from source in CI per
  architecture instead. See `docs/adr/0007-arm64-native-ci-and-mpv-build.md`.
- ARM64 AppImage tooling doesn't cross-compile — mitigated with
  GitHub-hosted native `ubuntu-24.04-arm` runners.
