# ADR 0001: mpv runs as a sidecar subprocess, never linked via libmpv FFI

## Status
Accepted

## Context
Echora needs audio playback. Two options were considered: (a) link
`libmpv` directly into the Rust binary via FFI, in-process; (b) spawn the
`mpv` binary as a separate sidecar process and control it over its
documented JSON IPC socket (`--input-ipc-server`).

Echora's own source must stay proprietary/source-available (PolyForm
Strict + addendum). mpv's default build is effectively GPL (X11 VO, DVD,
CDDA, and other GPL-only subsystems are compiled in). A genuinely
LGPL-only build (`-Dgpl=false`) is possible in principle, but only if
*every* transitively linked dependency is also free of GPL-only code
(FFmpeg without `--enable-gpl`, no libcdio, no libdvdnav/read, no GPL-only
filters) — and this has to be re-verified on every version bump. No
mainstream libmpv-linking frontend (IINA, mpv.net, Celluloid, Haruna,
Bomi, SMPlayer) ships this way while staying proprietary; all of them are
GPL-licensed as a direct consequence of linking libmpv.

The FSF's own GPL FAQ treats two programs communicating as separate OS
processes (pipes, sockets, CLI) as "mere aggregation," not a combined
work — this is exactly the sidecar pattern, and mpv ships a JSON IPC
protocol built for precisely this use case.

## Decision
mpv runs as a spawned subprocess, controlled only via its `--input-ipc-server`
JSON IPC socket. It is never `dlopen`'d or linked into Echora's own Rust
binary. The bundled mpv binary is built by Echora's own CI (see ADR 0007)
and shipped unmodified.

## Consequences
- Legally simpler and has real-world precedent; avoids an ongoing,
  version-by-version GPL-taint audit burden.
- One extra OS process per playback session (small RAM/CPU overhead vs.
  in-process linking) — acceptable against the "extreme lightness"
  priority, and still far lighter than a browser + YouTube tab.
- No native video window: mpv is invoked audio-only (`--no-video` /
  equivalent), matching the product's audio-only scope.
- Since mpv is unmodified upstream (GPL is fine for a subprocess), we do
  not need to chase a clean LGPL build — this also simplifies ADR 0007's
  build task (build for audio-only feature set for size/leanness, not
  for licensing).
