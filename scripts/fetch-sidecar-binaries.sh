#!/usr/bin/env bash
set -euo pipefail

# Downloads yt-dlp and Deno's official release binaries and verifies
# them against upstream-published checksums before placing them where
# Tauri's externalBin expects them.
#
# Usage: scripts/fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>
# <arch> is "x86_64" or "aarch64" (matches each binary's own release
# asset naming, which differs from Tauri's target-triple convention).
# Example: scripts/fetch-sidecar-binaries.sh x86_64 x86_64-unknown-linux-gnu src-tauri/binaries

ARCH="${1:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"
TARGET_TRIPLE="${2:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"
OUT_DIR="${3:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"

mkdir -p "$OUT_DIR"

# --- yt-dlp ---
YT_DLP_ASSET="yt-dlp_linux"
if [ "$ARCH" = "aarch64" ]; then
  YT_DLP_ASSET="yt-dlp_linux_aarch64"
fi
curl -fL -o "$OUT_DIR/yt-dlp-$TARGET_TRIPLE" \
  "https://github.com/yt-dlp/yt-dlp/releases/latest/download/$YT_DLP_ASSET"
# yt-dlp's checksum asset is named SHA2-256SUMS (confirmed against the
# real release assets list; SHA256SUMS does not exist and 404s).
curl -fL -o "/tmp/SHA2-256SUMS" \
  "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS"
EXPECTED_SHA="$(grep " $YT_DLP_ASSET\$" /tmp/SHA2-256SUMS | awk '{print $1}')"
ACTUAL_SHA="$(sha256sum "$OUT_DIR/yt-dlp-$TARGET_TRIPLE" | awk '{print $1}')"
if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
  echo "yt-dlp checksum mismatch: expected $EXPECTED_SHA, got $ACTUAL_SHA" >&2
  exit 1
fi
chmod +x "$OUT_DIR/yt-dlp-$TARGET_TRIPLE"

# --- Deno ---
DENO_ZIP="deno-x86_64-unknown-linux-gnu.zip"
if [ "$ARCH" = "aarch64" ]; then
  DENO_ZIP="deno-aarch64-unknown-linux-gnu.zip"
fi
curl -fL -o "/tmp/$DENO_ZIP" \
  "https://github.com/denoland/deno/releases/latest/download/$DENO_ZIP"
# Deno publishes a per-asset .sha256sum file alongside each zip
# (confirmed against the real release assets list, not assumed).
curl -fL -o "/tmp/$DENO_ZIP.sha256sum" \
  "https://github.com/denoland/deno/releases/latest/download/$DENO_ZIP.sha256sum"
EXPECTED_SHA="$(awk '{print $1}' "/tmp/$DENO_ZIP.sha256sum")"
ACTUAL_SHA="$(sha256sum "/tmp/$DENO_ZIP" | awk '{print $1}')"
if [ "$EXPECTED_SHA" != "$ACTUAL_SHA" ]; then
  echo "deno checksum mismatch: expected $EXPECTED_SHA, got $ACTUAL_SHA" >&2
  exit 1
fi
unzip -p "/tmp/$DENO_ZIP" deno > "$OUT_DIR/deno-$TARGET_TRIPLE"
chmod +x "$OUT_DIR/deno-$TARGET_TRIPLE"

echo "Fetched and verified yt-dlp-$TARGET_TRIPLE and deno-$TARGET_TRIPLE into $OUT_DIR"
