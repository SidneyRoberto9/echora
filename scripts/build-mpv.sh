#!/usr/bin/env bash
set -euo pipefail

# Builds mpv from source, audio-only, for the current machine's
# architecture, against a minimal FFmpeg this also builds from source
# (see scripts/build-ffmpeg.sh), and makes the result relocatable so it
# can ship inside Echora's own package (see
# docs/adr/0007-arm64-native-ci-and-mpv-build.md).
#
# Usage: scripts/build-mpv.sh <target-triple> <output-dir>
# Example: scripts/build-mpv.sh x86_64-unknown-linux-gnu src-tauri/binaries

MPV_VERSION="v0.41.0"
TARGET_TRIPLE="${1:?usage: build-mpv.sh <target-triple> <output-dir>}"
OUT_DIR="${2:?usage: build-mpv.sh <target-triple> <output-dir>}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Resolve to an absolute path before the `cd` below, or every later use
# of $OUT_DIR (relative) would land inside $WORK_DIR and get wiped by
# the EXIT trap along with the rest of the mpv clone.
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

# Build a minimal, audio-only FFmpeg first and point meson at it via
# PKG_CONFIG_PATH -- meson's dependency('libavcodec', ...) etc. resolve
# through pkg-config, and PKG_CONFIG_PATH entries are searched *before*
# the system pkgconfig dirs, so this wins over any distro libavcodec-dev
# et al. that happen to also be installed (see scripts/build-ffmpeg.sh
# for the full justification of what's enabled and why). LD_LIBRARY_PATH
# is exported too, for the same reason but at *runtime*: nothing bakes an
# rpath into `build/mpv` at this stage (that happens later, via
# `patchelf`, once the binary exists), so without this, `ldd`/direct
# execution below would silently fall through to the system copies via
# the default loader search path -- exactly the failure mode this whole
# change exists to prevent.
FFMPEG_PREFIX="$WORK_DIR/ffmpeg-prefix"
"$SCRIPT_DIR/build-ffmpeg.sh" "$FFMPEG_PREFIX"
export PKG_CONFIG_PATH="$FFMPEG_PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LD_LIBRARY_PATH="$FFMPEG_PREFIX/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

git clone --depth 1 --branch "$MPV_VERSION" https://github.com/mpv-player/mpv.git "$WORK_DIR/mpv"
cd "$WORK_DIR/mpv"

meson setup build \
  -Dgl=disabled \
  -Dvulkan=disabled \
  -Dx11=disabled \
  -Dwayland=disabled \
  -Dcocoa=disabled \
  -Dalsa=enabled \
  -Dpulse=enabled \
  -Dlibmpv=false \
  -Dcplayer=true \
  -Dlibavdevice=enabled

meson compile -C build

# Decisive, not cosmetic: prove mpv actually linked against the FFmpeg
# just built above, not a distro copy of the same soname that happens to
# also be on this machine (the exact trap PKG_CONFIG_PATH/LD_LIBRARY_PATH
# above are meant to avoid -- this is what actually confirms it worked,
# every time this script runs, not just the one time someone eyeballed
# it). A minimal --disable-everything libavcodec can never have a NEEDED
# entry on any of these system-package-only libraries.
RESOLVED_LIBAVCODEC="$(ldd build/mpv | awk '/libavcodec\.so/ {print $3}')"
case "$RESOLVED_LIBAVCODEC" in
  "$FFMPEG_PREFIX"/lib/*) ;;
  *)
    echo "mpv linked against '$RESOLVED_LIBAVCODEC', not this script's own" \
      "FFmpeg build ($FFMPEG_PREFIX/lib) -- PKG_CONFIG_PATH lost to a" \
      "system libavcodec-dev. Aborting: the whole point of this build is" \
      "the minimal FFmpeg, not the distro's." >&2
    exit 1
    ;;
esac
if ldd "$RESOLVED_LIBAVCODEC" | grep -qE 'libx264|libx265|libvpx|libaom|libdav1d|librav1e'; then
  echo "mpv's resolved libavcodec ($RESOLVED_LIBAVCODEC) links a" \
    "GPL/nonfree codec lib this build never enabled -- it is not the" \
    "minimal build this script just produced." >&2
  exit 1
fi

mkdir -p "$OUT_DIR" "$OUT_DIR/lib"
cp build/mpv "$OUT_DIR/mpv-$TARGET_TRIPLE"

# Bundle mpv's runtime shared-library dependencies next to the binary,
# then rewrite its rpath so the loader finds them there regardless of
# install location (mpv itself is never statically linkable on Linux —
# see ADR 0007).
#
# This bundles *everything* `ldd` reports except a short, justified
# exclude list below — not a hand-picked "just the FFmpeg libs" guess.
# That guess (`libav|libsw|libpostproc`) previously missed libass and
# libplacebo entirely, and both are load-bearing: they're *unconditional*
# `dependency()` calls in mpv's own top-level meson.build (confirmed by
# reading it directly — no `-Dlibass=disabled`/`-Dlibplacebo=disabled`
# option exists in mpv 0.41.0's meson.options; passing them makes
# `meson setup` itself fail with "Unknown options"). Echora never
# exercises either — audio-only, always `--no-video`, no subtitles (see
# media/player.rs) — but mpv cannot be *built* without linking them, so
# there's no way to avoid needing them at *runtime* short of patching
# mpv's build system upstream, a bigger ongoing maintenance burden than
# bundling their (~40MB) dependency chain. Revisit if a future mpv
# release adds a real toggle for either.
#
# Excluded, and why:
#  - libc/libm/libstdc++/libgcc_s (and libpthread/libdl/librt, listed in
#    case an older glibc than this project's own CI/dev baseline — where
#    they're already merged into libc — ever builds this): the C/C++
#    runtime every dynamically-linked ELF binary on the target already
#    requires just to exist. Bundling these would mean bundling glibc
#    itself, which is exactly the "wontfix" static-linking problem
#    ADR 0007 already explains mpv can't route around on Linux.
#  - libasound/libpulse(common): the ALSA/PulseAudio runtime, declared as
#    `.deb` system dependencies instead (see tauri.conf.json's
#    `bundle.linux.deb.depends`), the same way any other desktop-audio
#    Linux app depends on them, rather than bundled. libpulsecommon
#    specifically is PulseAudio's own version-pinned private plugin
#    (dlopen'd from a versioned path under .../pulseaudio/, not a normal
#    SONAME dependency) — bundling a copy that could drift from whatever
#    pulseaudio package is actually installed would be worse than
#    relying on libpulse0's own apt dependency to keep them matched.
#
# Previously (system FFmpeg, before this task's change): 170 libraries,
# ~220MB, verified for real on Ubuntu 24.04 x86_64. With the minimal
# FFmpeg built above instead: verified for real in the same environment
# at a fraction of that -- see docs/adr/0007's 2026-09-06 update for the
# exact before/after numbers this was measured at. Installed into a clean
# `ubuntu:24.04` container with none of the `-dev` build packages present
# — bundle plus only `deb.depends` — `ldd` reports zero "not found" and
# `mpv --version` plus real WAV playback both work. CI's
# `package-smoke-test` job re-proves this on every run rather than
# trusting this comment to stay true across mpv/FFmpeg/Ubuntu version
# bumps.
EXCLUDE_LIBS='/lib(c|m|stdc\+\+|gcc_s|pthread|dl|rt)\.so|/libasound\.so|/libpulse\.so|/libpulsecommon-[0-9.]+\.so'
ldd "$OUT_DIR/mpv-$TARGET_TRIPLE" \
  | awk '/=> \// {print $3}' \
  | grep -Ev "$EXCLUDE_LIBS" \
  | xargs -I{} cp --update=none {} "$OUT_DIR/lib/"

# LGPL §6 corresponding-source obligation for the FFmpeg build above (see
# scripts/build-ffmpeg.sh) -- copied out of $WORK_DIR before it's wiped by
# this script's own EXIT trap. release.yml uploads both as release
# assets; not placed under $OUT_DIR/lib/ so Tauri's `resources` glob for
# that directory never has a reason to treat them as anything other than
# what they are (release provenance, not a runtime dependency).
cp "$FFMPEG_PREFIX/echora-ffmpeg-provenance.txt" "$OUT_DIR/ffmpeg-provenance.txt"
cp "$FFMPEG_PREFIX/echora-ffmpeg-source.tar.gz" "$OUT_DIR/ffmpeg-source.tar.gz"

# This dir is copied into the package's mpv-x86_64-unknown-linux-gnu/lib/
# under Tauri's `resources` config, which lands at usr/lib/echora/lib/ in
# both the .deb and the AppImage (confirmed empirically, not documented
# by Tauri — see ADR 0007's Update). mpv itself lands at usr/bin/mpv, so
# that's `$ORIGIN/../lib/echora/lib` from there. `--force-rpath` writes
# the legacy DT_RPATH tag (not DT_RUNPATH) so this wins over the AppImage
# AppRun's LD_LIBRARY_PATH for mpv specifically.
#
# OPEN RISK, not fixed by this script (see ADR 0007's 2026-09-06 update,
# "AppImage soname-collision risk"): linuxdeploy still separately
# auto-bundles *its own*, unrelated copies of the same sonames
# (libavcodec.so.60 et al.) flat under the AppImage's usr/lib/, for
# WebKitGTK/GStreamer's own use -- and rewrites mpv's rpath again, to
# that flat dir, during its own relocation pass, *after* this script has
# already run. Before this task, that was harmless (both copies were
# byte-identical Ubuntu apt packages). With this task's minimal FFmpeg,
# they are no longer identical, and this script has no visibility into,
# or control over, what linuxdeploy decides to place at that shared path
# -- that step happens later, in `cargo tauri build`/tauri-action, outside
# this script entirely. CI's `package-smoke-test` job checks the built
# AppImage specifically for this.
patchelf --force-rpath --set-rpath "\$ORIGIN/../lib/echora/lib" "$OUT_DIR/mpv-$TARGET_TRIPLE"

echo "Built $OUT_DIR/mpv-$TARGET_TRIPLE"
"$OUT_DIR/mpv-$TARGET_TRIPLE" --version
