# ADR 0003: No standalone ffmpeg binary bundled

## Status
Accepted

## Context
yt-dlp can hand off a direct audio stream URL without needing ffmpeg
itself, as long as it's used purely for resolution (no `-x`/postprocessing
flags). Separately, mpv links its own FFmpeg internally
(`libavformat`/`libavcodec`) and can consume a direct stream URL,
handling demux/decode itself.

Offline download/re-encoding is out of v1 scope (see Requirements
Freeze), which is the main scenario that would need yt-dlp's own
ffmpeg-based postprocessing.

## Decision
Echora does not bundle a standalone `ffmpeg` binary. yt-dlp is invoked in
resolve-only mode (no postprocessing); mpv's own internal FFmpeg handles
decoding of the resolved stream.

## Consequences
- One fewer sidecar binary to build, sign, update, and license-track —
  smaller package, less surface area.
- If offline download ever ships post-v1, this decision is revisited:
  that feature would need either yt-dlp's postprocessing (requiring
  ffmpeg) or a different remux approach.

## Update (2026-09-06): mpv's internal FFmpeg is now a minimal custom
build, not "some unspecified FFmpeg" — this decision still holds
This ADR's decision was, and remains, specifically about not bundling an
`ffmpeg`/`ffplay` **CLI binary**. That's still true: `scripts/build-mpv.sh`
now builds mpv's internal FFmpeg from source too (see
`scripts/build-ffmpeg.sh`, replacing what used to be Ubuntu's own
monolithic shared FFmpeg package — see
`docs/adr/0007-arm64-native-ci-and-mpv-build.md`'s 2026-09-06 update for
the full "why" and the real before/after measurements), and that build
passes `--disable-programs --disable-doc` explicitly, same as it
implicitly relied on the distro package never installing `ffmpeg`/
`ffplay` binaries before. No `ffmpeg` CLI is bundled either way — only
the shared libraries mpv itself links against, which this ADR was never
about.
