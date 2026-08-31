# ADR 0005: MPRIS via the `mpris-server` crate; no separate media-keys plugin

## Status
Accepted

## Context
Echora is Linux-only for v1. Media-key handling and desktop
integration (GNOME/KDE "now playing" widgets, lock-screen controls) on
Linux are conventionally done by exposing the standard MPRIS D-Bus
interface — the desktop environment then routes hardware media keys to
whichever app is the active MPRIS player. A cross-platform community
Tauri plugin for media keys exists (`tauri-plugin-media`), but it carries
Windows/macOS code paths Echora doesn't need for a Linux-only v1.

Two Rust MPRIS crates were compared: `mpris-server` (Linux-only,
actively maintained, LGPL-3.0+/MPL-2.0) and `souvlaki` (cross-platform,
heavier, simpler API).

## Decision
Use `mpris-server`. Do not add a separate media-keys plugin — MPRIS
already gives Echora media-key routing for free on Linux desktop
environments that implement the standard (GNOME, KDE, XFCE via the
FreeDesktop status notifier / MPRIS spec).

## Consequences
- One dependency instead of two; no unused cross-platform code paths.
- If Echora ever targets Windows/macOS, media-key handling needs a
  platform-specific mechanism at that point (MPRIS is Linux/D-Bus only)
  — deliberately deferred, not designed for now.
