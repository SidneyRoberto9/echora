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

## Update (2026-09-01, real build attempt)
The "nothing in this environment can run a full `cargo tauri build`" gap
noted below was closed by installing the missing toolchain and actually
running `scripts/build-mpv.sh` for real. It failed twice before
succeeding, both times on things no prior review caught because nothing
had ever actually tried to run it:

1. `-Dwin32-desktop=disabled` in `scripts/build-mpv.sh`'s meson invocation
   is not a real mpv meson option — confirmed by reading mpv v0.41.0's
   `meson.options` directly (no `desktop`-named option exists at all, on
   any platform). Removed; it was a no-op typo, not a Windows/Linux
   version drift (Echora doesn't target Windows anyway).
2. mpv's `meson.build` unconditionally requires `libavfilter`,
   `libswscale`, `libplacebo`, and `libass` at build time (`dependency()`
   calls with no `required: get_option(...)` gate — see lines 22-32),
   regardless of `-Dgl=disabled`/`-Dvulkan=disabled`/etc. This project's
   own `release.yml` prerequisite list only installed `libavcodec-dev`,
   `libavformat-dev`, `libavutil-dev`, `libswresample-dev` — missing all
   four unconditional ones. Fixed in `release.yml`.

With both fixed, `scripts/build-mpv.sh x86_64-unknown-linux-gnu` succeeds
end-to-end: produces a relocatable `mpv-x86_64-unknown-linux-gnu` plus
`lib/{libavcodec,libavfilter,libavformat,libavutil,libpostproc,
libswresample,libswscale}.so.*` (7 shared objects, ~34MB).

## Resolved (2026-09-01): mpv `.so` resources bundling
The previously open pre-release blocker (mpv's `.so` deps had no
`resources` entry bundling them into the package) was closed by adding a
`resources` entry to
`src-tauri/tauri.conf.json`'s `bundle` block (`"binaries/lib/*": "lib/"`)
and, once real placement was observed, correcting `build-mpv.sh`'s rpath.
Verified against real, fully built packages (`npx tauri build`), not just
reasoning about Tauri's docs:

- **Real placement, confirmed empirically** (undocumented by Tauri):
  `resources` lands at `usr/lib/echora/lib/` in *both* the `.deb` and the
  AppImage's `AppDir` — identical relative structure in both formats.
  `externalBin`/the main binary land at `usr/bin/`. So the correct rpath
  from `usr/bin/mpv` is `$ORIGIN/../lib/echora/lib`, not the originally
  assumed `$ORIGIN/lib` — fixed in `build-mpv.sh`, using
  `patchelf --force-rpath` (legacy `DT_RPATH`, not `DT_RUNPATH`) so it
  wins over `LD_LIBRARY_PATH`.
- **`.deb`: verified clean.** Extracted the real built `.deb`
  (`dpkg-deb -x`) and ran `ldd` on the extracted `usr/bin/mpv`: all 7
  `.so` deps resolve to `usr/lib/echora/lib/*`, not any system path.
  Ran the extracted binary directly — works.
- **AppImage: verified working, but with a caveat worth tracking.**
  `linuxdeploy` (Tauri's AppImage bundler) rewrites `usr/bin/mpv`'s rpath
  during its own relocation pass — from `$ORIGIN/../lib/echora/lib` to
  `$ORIGIN/../lib` (its own convention: everything flat under `usr/lib/`,
  since it also auto-bundles system copies of `libavcodec`/`libavfilter`/
  etc. for WebKitGTK/GStreamer's own use, which happen to need the same
  sonames). Right now this is harmless — `md5sum` confirms all 7 `.so`
  files linuxdeploy auto-bundled are byte-identical to `build-mpv.sh`'s
  own copies (both are ultimately the same Ubuntu 24.04 apt packages,
  since neither this project nor GStreamer builds FFmpeg from source),
  and the AppImage's mpv runs correctly. **This identity is coincidental,
  not structurally guaranteed** — a future mpv version bump, or the CI
  runner's base image ever drifting from what GStreamer/WebKitGTK links
  against, could silently reintroduce a real ABI mismatch inside the
  AppImage specifically (the `.deb` path is unaffected either way, since
  `dpkg`-based bundling doesn't rewrite rpaths). Re-verify this specific
  check (`md5sum` the two copies, or re-run the `ldd`/ownership check
  used here) whenever mpv's pinned version changes or the CI base image
  changes — don't assume it still holds.

## Consequences
- mpv becomes a build artifact Echora's own CI produces and
  checksum-tracks per release, not a binary fetched from a third party —
  more reproducible, but Echora now owns keeping that build working
  across mpv version bumps.
- No paid CI minutes needed for ARM64 (free hosted runner, public repo).
- `.deb` cross-compilation (which does work from x86_64) is not used
  either, for consistency — both formats, both architectures, build
  natively in their own matrix job.
