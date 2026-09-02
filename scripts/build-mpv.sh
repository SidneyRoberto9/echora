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

# Bundle mpv's runtime shared-library dependencies (FFmpeg's libs; mpv
# itself is never statically linkable on Linux — see ADR 0007) next to
# the binary, then rewrite its rpath so the loader finds them there
# regardless of install location.
ldd "$OUT_DIR/mpv-$TARGET_TRIPLE" \
  | awk '/=> \// {print $3}' \
  | grep -E 'libav|libsw|libpostproc' \
  | xargs -I{} cp -n {} "$OUT_DIR/lib/"

patchelf --force-rpath --set-rpath '$ORIGIN/lib' "$OUT_DIR/mpv-$TARGET_TRIPLE"

echo "Built $OUT_DIR/mpv-$TARGET_TRIPLE"
"$OUT_DIR/mpv-$TARGET_TRIPLE" --version
