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
