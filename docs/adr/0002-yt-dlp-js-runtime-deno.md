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
