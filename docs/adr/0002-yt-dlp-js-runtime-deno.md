# ADR 0002: yt-dlp as sidecar + Deno as the bundled JS runtime for YouTube extraction

## Status
Accepted

## Context
As of yt-dlp 2025.11.12, reliable YouTube extraction requires solving
nsig (signature) descrambling and PO-Token challenges. yt-dlp's internal
`jsinterp.py` is no longer sufficient; an external real JS engine is
required via yt-dlp's "EJS" (External JavaScript) mechanism. Supported
runtimes, in yt-dlp's own recommended order: Deno (≥2.3), Node (≥22),
QuickJS, QuickJS-ng, Bun (deprecated).

Separately, YouTube's PO-Token requirement is served by community
tooling (`bgutil-ytdlp-pot-provider`), which supports either a Node or a
Deno runtime.

The end user must never be asked to install any of this manually.

## Decision
Bundle **Deno** (official prebuilt binary, MIT-licensed) as the single JS
runtime sidecar, used for both:
1. yt-dlp's EJS nsig-solving (yt-dlp invokes it directly, short-lived
   per call), and
2. the PO-Token provider, run via its Deno mode.

yt-dlp itself is bundled as the official standalone Linux binary
(`yt-dlp_linux`, `yt-dlp_linux_aarch64`), spawned as a subprocess and
never linked. The PO-Token provider is invoked on demand (not run as an
always-on background HTTP server) for v1, prioritizing idle RAM; this is
revisited if benchmarking (Fase 9) shows resolve latency from repeated
cold starts is a real problem.

## Consequences
- Only one JS runtime bundled (Deno), not two — avoids bundling both
  Node and Deno.
- Deno's MIT license imposes no constraint on subprocess vs. FFI use;
  subprocess is used anyway for consistency with the rest of the sidecar
  architecture and process-lifecycle control.
- The official yt-dlp standalone binary is a GPLv3-or-later combined
  work (bundled GNU Readline in its frozen CPython interpreter), even
  though yt-dlp's own source is Unlicense. Tracked in
  `THIRD_PARTY_NOTICES.md`; safe to redistribute unmodified as a
  subprocess under the same "mere aggregation" reasoning as ADR 0001.
- Requires Echora's Rust core to manage two more subprocess lifecycles
  (yt-dlp, Deno) alongside mpv — handled by the same sidecar-management
  code path, not bespoke per binary.

## Update (2026-09-06): pinned versions, not `latest`
`scripts/fetch-sidecar-binaries.sh` originally downloaded both binaries
from each project's `releases/latest` and validated the checksum
fetched from that same `latest` release — which only proved the
download wasn't corrupted in transit, not that it was the version
Echora actually tested or that it hadn't been swapped upstream. Both
are now pinned to an explicit version with an expected SHA-256 recorded
in the script itself, resolved from the real release at pin time. This
does not freeze yt-dlp — see CONTRIBUTING.md's "Bumping sidecar
versions" section for how and when to move the pin forward.

## Update (2026-09-06): sidecars ship prefixed, and Deno's path drops the triple
Two defects found by installing the real v0.2.0 `.deb` on a developer
machine — neither reachable from CI, whose containers are bare.

Tauri's `externalBin` entries install alongside the main binary, so
`binaries/{mpv,yt-dlp,deno}` landed as `/usr/bin/{mpv,yt-dlp,deno}` and
dpkg refused the whole package on any machine already carrying the
distro's `yt-dlp`:

    dpkg: a tentar sobre-escrever '/usr/bin/yt-dlp', que também está no
    pacote yt-dlp 2024.04.09-1

Every `externalBin` entry is therefore prefixed — `echora-mpv`,
`echora-yt-dlp`, `echora-deno` — which is the only namespace Echora
controls in a shared `/usr/bin`. `.sidecar("...")` call sites, the
build/fetch/stub scripts and the CI smoke tests use the prefixed names.
A CI step installs the distro `yt-dlp` and `mpv` *before* the `.deb` so
a regression fails there rather than on a user's machine.

Separately, `resolve_deno_path()` was reconstructing
`deno-x86_64-unknown-linux-gnu`, the on-disk dev name. Both bundlers
strip the target triple: the `.deb` and the AppImage's AppDir each
carry a plain `usr/bin/echora-deno`. The old path never existed in a
shipped package, and the failure is silent — yt-dlp only warns
("No supported JavaScript runtime could be found ... some formats may
be missing") and degrades to extraction without a JS runtime, which is
exactly the deprecated path this ADR exists to avoid. Dev mode still
keeps the suffix, so the resolver branches on it and a unit test pins
both shapes.
