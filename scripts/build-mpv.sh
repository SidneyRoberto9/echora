#!/usr/bin/env bash
set -euo pipefail

# Builds mpv from source, audio-only, for the current machine's
# architecture, and makes the result relocatable so it can ship inside
# Echora's own package (see docs/adr/0007-arm64-native-ci-and-mpv-build.md).
#
# Usage: scripts/build-mpv.sh <target-triple> <output-dir>
# Example: scripts/build-mpv.sh x86_64-unknown-linux-gnu src-tauri/binaries

MPV_VERSION="v0.41.0"
TARGET_TRIPLE="${1:?usage: build-mpv.sh <target-triple> <output-dir>}"
OUT_DIR="${2:?usage: build-mpv.sh <target-triple> <output-dir>}"

# Resolve to an absolute path before the `cd` below, or every later use
# of $OUT_DIR (relative) would land inside $WORK_DIR and get wiped by
# the EXIT trap along with the rest of the mpv clone.
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

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
  -Dcplayer=true

meson compile -C build

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
# Verified for real on Ubuntu 24.04 x86_64 (see task report): this
# resolves to 170 libraries (~220MB). Installed into a clean `ubuntu:24.04`
# container with none of the `-dev` build packages present — bundle plus
# only `deb.depends` — `ldd` reports zero "not found" and `mpv --version`
# plus real WAV playback both work. CI's `package-smoke-test` job
# re-proves this on every run rather than trusting this comment to stay
# true across mpv/Ubuntu version bumps.
EXCLUDE_LIBS='/lib(c|m|stdc\+\+|gcc_s|pthread|dl|rt)\.so|/libasound\.so|/libpulse\.so|/libpulsecommon-[0-9.]+\.so'
ldd "$OUT_DIR/mpv-$TARGET_TRIPLE" \
  | awk '/=> \// {print $3}' \
  | grep -Ev "$EXCLUDE_LIBS" \
  | xargs -I{} cp --update=none {} "$OUT_DIR/lib/"

# This dir is copied into the package's mpv-x86_64-unknown-linux-gnu/lib/
# under Tauri's `resources` config, which lands at usr/lib/echora/lib/ in
# both the .deb and the AppImage (confirmed empirically, not documented
# by Tauri — see ADR 0007's Update). mpv itself lands at usr/bin/mpv, so
# that's `$ORIGIN/../lib/echora/lib` from there. `--force-rpath` writes
# the legacy DT_RPATH tag (not DT_RUNPATH) so this wins over the AppImage
# AppRun's LD_LIBRARY_PATH, which otherwise resolves to a *different*,
# system/GStreamer-provided copy of the same libavcodec.so.60 et al. that
# linuxdeploy bundles flat under usr/lib/ for WebKitGTK's own use.
patchelf --force-rpath --set-rpath "\$ORIGIN/../lib/echora/lib" "$OUT_DIR/mpv-$TARGET_TRIPLE"

echo "Built $OUT_DIR/mpv-$TARGET_TRIPLE"
"$OUT_DIR/mpv-$TARGET_TRIPLE" --version
