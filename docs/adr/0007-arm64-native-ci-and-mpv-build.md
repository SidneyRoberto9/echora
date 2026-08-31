# ADR 0007: Native ARM64 CI runners; mpv built from source per architecture in CI

## Status
Accepted

## Context
Two packaging risks were identified for shipping `.deb` + AppImage on
both x86_64 and ARM64:

1. **No portable mpv binary exists upstream for either architecture.**
   mpv upstream has marked Linux static builds as `wontfix` (glibc
   cannot be fully statically linked in practice). No official or
   reputable community project publishes a ready-made, self-contained
   mpv binary (static or AppImage-bundled) confirmed for both x86_64 and
   aarch64 — even BtbN's well-known static FFmpeg builds don't cover
   Linux aarch64.
2. **AppImage tooling (`linuxdeploy`) doesn't cross-compile.** An ARM64
   AppImage can only be built on real or emulated ARM64 execution, not
   cross-compiled from an x86_64 build host. QEMU emulation works but is
   documented as ~6x slower (~1 hour vs. ~10 minutes) with no benefit now
   that native runners are free.

## Decision
1. **Build mpv from source, natively, per architecture, in CI.** Compile
   an audio-only feature set (disable video outputs, DVD/CDDA input, and
   other subsystems Echora doesn't need — for size and dependency
   reduction, not for licensing, since ADR 0001 already covers the GPL
   question via the sidecar architecture). Bundle the resulting binary
   with its runtime `.so` dependencies, made relocatable via `patchelf`
   + rpath `$ORIGIN` (the same technique `linuxdeploy` uses), named per
   Tauri's sidecar target-triple convention.
2. **Use GitHub-hosted native ARM64 runners** (`ubuntu-24.04-arm`, free
   for public repositories since January 2025) for both the ARM64 mpv
   build and the ARM64 `.deb`/AppImage packaging. No QEMU, no
   cross-compilation for anything ARM64-AppImage-related.
3. CI matrix: one job on a standard `ubuntu-24.04` runner for x86_64, one
   job on `ubuntu-24.04-arm` for ARM64 — both build mpv, then the app,
   then package `.deb` + AppImage, natively.

## Update (Fase 3)
While validating real playback, a third-party community project
(`pkgforge-dev/mpv-AppImage`) was found publishing self-contained mpv
AppImages for *both* x86_64 and aarch64 — contrary to what was assumed
above. It was tried and rejected for the shipped artifact: it runs its
own auto-update check on startup (an unpinned, uncontrolled network call
Echora doesn't want happening inside a bundled dependency) and isn't
checksum-pinned the way this ADR requires. It's a useful data point that
an aarch64 mpv bundle is achievable at all, but the source stays "build
it ourselves in CI," not this AppImage. Local dev/testing instead used
the distro's plain `mpv` package, which has none of this problem since
it's just invoked directly, unmodified, with no wrapper.

## Consequences
- mpv becomes a build artifact Echora's own CI produces and
  checksum-tracks per release, not a binary fetched from a third party —
  more reproducible, but Echora now owns keeping that build working
  across mpv version bumps.
- No paid CI minutes needed for ARM64 (free hosted runner, public repo).
- `.deb` cross-compilation (which does work from x86_64) is not used
  either, for consistency — both formats, both architectures, build
  natively in their own matrix job.
