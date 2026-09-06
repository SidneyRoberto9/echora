#!/usr/bin/env bash
set -euo pipefail

# Downloads yt-dlp and Deno's official release binaries, pinned to an
# explicit version and an expected SHA-256 recorded below, and verifies
# each download against that expected hash before placing it where
# Tauri's externalBin expects it.
#
# Versions are pinned deliberately, not frozen: yt-dlp in particular
# needs to stay current for YouTube extraction to keep working (see
# docs/adr/0002-yt-dlp-js-runtime-deno.md). Bumping a pinned version
# here is a deliberate act, not automatic — see CONTRIBUTING.md's
# "Bumping sidecar versions" section for the full procedure, including
# how to regenerate the SHA-256 below.
#
# The expected SHA-256 is recorded here from the *same pinned release*
# at the time it was pinned, not fetched from `latest`/the release
# itself at build time: fetching the checksum from the same release
# you're downloading the binary from only catches transfer corruption,
# not a compromised upstream release. Pinning the hash in this script
# (checked into git, reviewed like any other change) is what actually
# proves "this is the binary we vetted," not just "this download didn't
# get corrupted in transit."
#
# Pinned versions (checked 2026-09-06 against the real GitHub releases
# API/asset lists, not assumed):
#   yt-dlp 2026.08.19  https://github.com/yt-dlp/yt-dlp/releases/tag/2026.08.19
#   Deno   v2.9.6      https://github.com/denoland/deno/releases/tag/v2.9.6
YT_DLP_VERSION="2026.08.19"
DENO_VERSION="v2.9.6"

# Usage: scripts/fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>
# <arch> is "x86_64" or "aarch64" (matches each binary's own release
# asset naming, which differs from Tauri's target-triple convention).
# Example: scripts/fetch-sidecar-binaries.sh x86_64 x86_64-unknown-linux-gnu src-tauri/binaries

ARCH="${1:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"
TARGET_TRIPLE="${2:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"
OUT_DIR="${3:?usage: fetch-sidecar-binaries.sh <arch> <target-triple> <output-dir>}"

mkdir -p "$OUT_DIR"

# --- yt-dlp ---
# Expected SHA-256 values below come from yt-dlp 2026.08.19's own
# SHA2-256SUMS asset (yt-dlp's checksum asset is named SHA2-256SUMS;
# SHA256SUMS does not exist and 404s).
YT_DLP_ASSET="yt-dlp_linux"
YT_DLP_SHA256="58162f9bfdc27458ea47bfcb311cf47028f17d8154a8bf7d689861d46399230a"
if [ "$ARCH" = "aarch64" ]; then
  YT_DLP_ASSET="yt-dlp_linux_aarch64"
  YT_DLP_SHA256="b16e4dab368a816cd05d477d698a605a6ae87ccee1c8ffd38fa21d7254141fcc"
fi
curl -fL -o "$OUT_DIR/yt-dlp-$TARGET_TRIPLE" \
  "https://github.com/yt-dlp/yt-dlp/releases/download/$YT_DLP_VERSION/$YT_DLP_ASSET"
ACTUAL_SHA="$(sha256sum "$OUT_DIR/yt-dlp-$TARGET_TRIPLE" | awk '{print $1}')"
if [ "$YT_DLP_SHA256" != "$ACTUAL_SHA" ]; then
  echo "yt-dlp checksum mismatch: expected $YT_DLP_SHA256, got $ACTUAL_SHA" >&2
  exit 1
fi
chmod +x "$OUT_DIR/yt-dlp-$TARGET_TRIPLE"

# --- Deno ---
# Expected SHA-256 values below come from Deno v2.9.6's own per-asset
# .sha256sum files (Deno publishes one alongside each zip).
DENO_ZIP="deno-x86_64-unknown-linux-gnu.zip"
DENO_SHA256="394f07f4da2bebe6ce6f1e7ce0fa16429b29b08c35e3fac3fe25972676dff4b2"
if [ "$ARCH" = "aarch64" ]; then
  DENO_ZIP="deno-aarch64-unknown-linux-gnu.zip"
  DENO_SHA256="9a46afc6c392c7cd2ff71a31558935545b46408d0e87f7a86908c712721c046e"
fi
curl -fL -o "/tmp/$DENO_ZIP" \
  "https://github.com/denoland/deno/releases/download/$DENO_VERSION/$DENO_ZIP"
ACTUAL_SHA="$(sha256sum "/tmp/$DENO_ZIP" | awk '{print $1}')"
if [ "$DENO_SHA256" != "$ACTUAL_SHA" ]; then
  echo "deno checksum mismatch: expected $DENO_SHA256, got $ACTUAL_SHA" >&2
  exit 1
fi
unzip -p "/tmp/$DENO_ZIP" deno > "$OUT_DIR/deno-$TARGET_TRIPLE"
chmod +x "$OUT_DIR/deno-$TARGET_TRIPLE"

echo "Fetched and verified yt-dlp-$TARGET_TRIPLE ($YT_DLP_VERSION) and deno-$TARGET_TRIPLE ($DENO_VERSION) into $OUT_DIR"
