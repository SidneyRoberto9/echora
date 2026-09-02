#!/usr/bin/env bash
set -euo pipefail

# Creates placeholder sidecar binaries for CI's fast lane (fmt, clippy,
# test, cargo build --release). Tauri's build.rs validates that every
# `externalBin` entry AND every `resources` glob resolves to at least one
# file on disk for the current target triple on EVERY `cargo
# build`/`clippy`/`test`, not just `tauri build` (see
# .superpowers/sdd/2026-09-01-packaging/task-1-report.md). None of those
# commands execute the sidecars or load the libs for real — the tests
# that do are #[ignore]d — so empty placeholders satisfy the check
# without the real mpv source build or yt-dlp/Deno downloads that
# release.yml does.
#
# Usage: scripts/stub-sidecar-binaries.sh <target-triple> <output-dir>

TARGET_TRIPLE="${1:?usage: stub-sidecar-binaries.sh <target-triple> <output-dir>}"
OUT_DIR="${2:?usage: stub-sidecar-binaries.sh <target-triple> <output-dir>}"

mkdir -p "$OUT_DIR" "$OUT_DIR/lib"
for name in mpv yt-dlp deno; do
  path="$OUT_DIR/$name-$TARGET_TRIPLE"
  printf '#!/bin/sh\nexit 0\n' >"$path"
  chmod +x "$path"
done
: >"$OUT_DIR/lib/libstub.so"
