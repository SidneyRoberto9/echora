# ADR 0006: Third-party licensing & bundling strategy

## Status
Accepted

## Context
Echora's own source is proprietary/source-available (PolyForm Strict +
addendum) and is publicly hosted. It bundles several third-party
components: mpv (GPL-2.0-or-later), yt-dlp's official standalone binary
(GPLv3-or-later as a combined work, source Unlicense), and Deno (MIT).
A licensing audit (see project research history) found that linking any
GPL-licensed code into Echora's own binary — statically or dynamically —
would force the combined work under GPL, with no build-flag remedy once
linked. Running an unmodified GPL binary as a separate OS process,
communicating only over its own IPC/CLI interface, is treated by the
FSF's own GPL FAQ as "mere aggregation," not a combined/derivative work.

## Decision
1. **Never FFI-link or `dlopen` any GPL-licensed component into Echora's
   own Rust binary.** mpv and yt-dlp are always invoked as unmodified
   subprocess sidecars (ADR 0001, 0002).
2. Ship the license text (and, for yt-dlp, its own
   `THIRD_PARTY_LICENSES.txt`) for every bundled GPL component, and
   point to the exact upstream source/version bundled, in
   `THIRD_PARTY_NOTICES.md`.
3. MIT-licensed sidecars (Deno) have no linking constraint either way,
   but are also run as subprocesses for consistency with the rest of the
   sidecar architecture.
4. Before adding any new bundled third-party binary or library, run it
   past this ADR: if it's GPL/AGPL and the only way to use it is linking
   (not a subprocess/CLI interface), treat it as a blocker and escalate —
   don't bundle it silently.
5. Rust crate and npm package dependencies (compile-time, not separately
   redistributed as binaries) are audited with `cargo-about`/`cargo-deny`
   and an npm license checker before the first tagged release; anything
   copyleft gets flagged and resolved, not shipped silently.

## Consequences
- Echora's own source can stay proprietary while legitimately
  redistributing GPL-licensed tools it depends on.
- Adds subprocess-management complexity (lifecycle, IPC, error handling
  for three sidecar processes) instead of simpler in-process linking —
  accepted trade-off for licensing safety, and it also isolates crashes
  in mpv/yt-dlp/Deno from taking down the main process.
- This is a technical/licensing analysis, not legal advice. Professional
  legal review is recommended before wide-scale distribution.
