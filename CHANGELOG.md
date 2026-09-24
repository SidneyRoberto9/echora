# Changelog

All notable changes to Echora are recorded here. Versions follow
[Semantic Versioning](https://semver.org/).

## [0.2.5] - 2026-09-24

### Added

- **Link radio**: paste a YouTube link on Home to start an endless
  session seeded by that track. Echora plays YouTube's own Mix for the
  video and keeps topping up the queue from the latest track, skipping
  recently played and already-queued tracks. Now playing shows
  "Mix · <title>", and link radio sessions in Library history replay
  from the same link.

### Fixed

- Pausing or resuming from the tray or MPRIS media controls no longer
  leaves the player showing the wrong play/pause state.
- Pasted YouTube links that end in a `#fragment` are accepted.
- A failed top-up fetch retries from the original seed instead of
  stalling the radio.
- Top-up results that arrive after you switch sessions no longer land
  in the new session's queue.
- Unrecognized Mix failures show a generic message instead of internal
  error details.

## [0.2.4] - 2026-09-09

See the [GitHub release](https://github.com/SidneyRoberto9/echora/releases/tag/v0.2.4)
for this and earlier versions.
