# Security Policy

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, use GitHub's private vulnerability reporting for this repository
(Security tab → "Report a vulnerability"), or open a draft security
advisory. This lets us assess and fix the issue before it's public.

Please include:
- A description of the vulnerability and its potential impact.
- Steps to reproduce, or a proof of concept if possible.
- The Echora version and OS/distro/architecture affected.

## Scope

Echora is a desktop application. Relevant areas include (non-exhaustive):
- Tauri IPC commands and capabilities configuration.
- Handling of untrusted external input: search results, video/track
  metadata, thumbnails, resolved media URLs.
- Sidecar process invocation (mpv, yt-dlp, the JS runtime used for
  YouTube's anti-bot challenges) — argument handling, path handling,
  process lifecycle.
- The auto-update mechanism and signature verification.
- Local data storage (SQLite database, cache, settings) and file paths.

Out of scope: vulnerabilities in third-party components themselves (mpv,
yt-dlp, the JS runtime) that are not specific to how Echora invokes or
configures them — please report those upstream. If you're unsure, report
here and we'll redirect if needed.

## Response

We aim to acknowledge reports within a reasonable time and will keep you
updated as we investigate and fix. Credit is given in the release notes
unless you ask otherwise.
