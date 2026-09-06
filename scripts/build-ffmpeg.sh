#!/usr/bin/env bash
set -euo pipefail

# Builds a minimal, audio-only FFmpeg from source and installs it to a
# private prefix, for scripts/build-mpv.sh to link mpv against instead of
# the distro's monolithic shared FFmpeg. See
# docs/adr/0007-arm64-native-ci-and-mpv-build.md's 2026-09-06 update: on
# Ubuntu 24.04, linking mpv against the system libavcodec60/etc. pulls in
# ~170 transitive libraries (~220MB) -- x264, x265, aom, dav1d, rsvg, zmq,
# and more -- none of which Echora's audio-only playback path
# (src-tauri/src/media/{resolver,metadata,player}.rs) ever touches.
#
# Usage: scripts/build-ffmpeg.sh <install-prefix>
# Example: scripts/build-ffmpeg.sh /tmp/echora-ffmpeg-prefix
#
# scripts/build-mpv.sh is the only caller: it builds this first, points
# meson at it via PKG_CONFIG_PATH, then bundles the resulting mpv +
# libraries together (see that script for the rest of the pipeline).

# Matches mpv 0.41.0's own minimum FFmpeg library versions exactly
# (libavcodec >= 60.31.102, libavfilter >= 9.12.100, etc. -- see mpv's
# meson.build) -- those minimums were set to FFmpeg 6.1's release
# versions, which also happens to be Ubuntu 24.04's own branch (6.1.1),
# so this project's existing dev-environment assumptions (ABI, behavior)
# stay valid. n6.1.6 is the latest 6.1.x point release as of this pin.
FFMPEG_VERSION="n6.1.6"

PREFIX="${1:?usage: build-ffmpeg.sh <install-prefix>}"

mkdir -p "$PREFIX"
PREFIX="$(cd "$PREFIX" && pwd)"

WORK_DIR="$(mktemp -d)"
trap 'rm -rf "$WORK_DIR"' EXIT

git clone --depth 1 --branch "$FFMPEG_VERSION" https://github.com/FFmpeg/FFmpeg.git "$WORK_DIR/ffmpeg"
cd "$WORK_DIR/ffmpeg"

# Method, per the task this was built for: --disable-everything, then
# enable only what Echora's real playback path needs, grouped and
# justified below -- not a --disable-X list, which ages badly as FFmpeg
# adds new optional features over time.
# shellcheck disable=SC2054 # commas below are FFmpeg's own --enable-X=a,b,c
# list syntax within a single array element, not a bash array separator typo.
CONFIGURE_ARGS=(
  --prefix="$PREFIX"

  # Shared, no static libs: mpv links dynamically, and build-mpv.sh's
  # rpath/bundling scheme assumes .so files, not a static archive.
  --disable-static
  --enable-shared

  # No ffmpeg/ffplay CLI binary is ever bundled (ADR 0003) -- this is
  # what keeps that true for a from-source build the same way it was
  # already true for the (now-replaced) system package.
  --disable-programs
  --disable-doc

  # Explicit, not relied-on-as-default: avdevice is needed only for the
  # lavfi indev below (test-signal generation, not real playback);
  # postproc is needed by nothing mpv links against (confirmed by
  # reading mpv 0.41.0's meson.build directly -- no dependency('libpostproc')
  # call exists at all).
  --enable-avdevice
  --disable-postproc

  # No GPL/nonfree code, full stop -- this is what keeps the whole build
  # LGPL-2.1-or-later instead of GPL (see docs/adr/0006). Every
  # decoder/demuxer/protocol/filter enabled below is either FFmpeg's own
  # native code or a permissively-licensed external lib (Mbed TLS,
  # Apache-2.0) -- nothing here needs --enable-gpl, --enable-nonfree, or
  # --enable-version3 (see the mbedtls note below for why that last one
  # matters), so none are passed.
  --disable-everything

  # --- Codecs Echora's real playback path needs, plus a deliberate
  # safety margin. bestaudio is Opus-in-WebM in practice for a normal
  # video (confirmed against the real yt-dlp binary in
  # src-tauri/binaries/dev, resolving a live YouTube video during this
  # task: acodec=opus, ext=webm, protocol=https) or AAC-in-MP4/M4A for
  # others. vorbis/mp3/flac/pcm are margin: small native decoders, no
  # extra libs, and getting this wrong would silently break playback for
  # a track class this project doesn't have automated coverage for. All
  # are FFmpeg's own native decoders (opusdec.c, aacdec.c, vorbisdec.c,
  # mpegaudiodec_float.c, flacdec.c, pcm.c) -- none need libopus/
  # libvorbis/libmp3lame, so there's no extra external codec dependency
  # at all, for any of them.
  --enable-decoder=aac,flac,mp3float,opus,vorbis,'pcm_*'
  --enable-parser=aac,flac,mpegaudio,opus

  # --- Containers those decoders arrive in. hls is explained in the
  # livestream note below; it force-selects the mpegts/mov demuxers it
  # needs internally (confirmed by reading libavformat/Makefile's
  # hls_demuxer_select) -- not something this list has to spell out.
  --enable-demuxer=aac,flac,hls,matroska,mov,mp3,ogg,wav

  # --- Network: mpv itself fetches the resolved URL via `loadfile`
  # (player.rs) -- yt-dlp only resolves it, never proxies the bytes.
  # https/tcp/tls cover the normal case (every resolved URL observed in
  # this task was https, googlevideo.com). http and crypto are margin:
  # http for any plain-http redirect hop, crypto for FFmpeg's own
  # (native, no extra dep) AES-128 HLS segment decryption, in case a
  # livestream ever uses it.
  --enable-protocol=http,https,tcp,tls,crypto

  # TLS backend for https: OpenSSL 3, not Mbed TLS. Mbed TLS was the
  # first choice on size (~0.9MB installed on Ubuntu 24.04 --
  # libmbedtls14t64 + libmbedcrypto7t64 + libmbedx509-1t64 =
  # 227+528+160KB via `apt-cache show`, vs. OpenSSL's 6615KB) and both
  # are nominally Apache-2.0 -- but actually running `./configure
  # --enable-mbedtls` against this build (no --enable-gpl) fails outright
  # in FFmpeg 6.1.6: "mbedtls is version3 and --enable-version3 is not
  # specified" (verified for real in this task, against the real
  # libmbedtls-dev 2.28.8 in Ubuntu 24.04's own repos -- not a
  # newer-mbedtls-only edge case). Reading configure's own
  # EXTERNAL_LIBRARY_VERSION3_LIST confirms this is unconditional for
  # *any* mbedtls version, gpl enabled or not: FFmpeg's own project
  # position is that Mbed TLS's Apache-2.0 license is incompatible with
  # GPLv2-family licenses (the FSF's own compatibility position: Apache-2.0
  # is GPLv3/LGPLv3-compatible, not GPLv2/LGPLv2.1-compatible), so linking
  # it forces --enable-version3 -- upgrading *this entire FFmpeg build*
  # from LGPL-2.1-or-later to LGPL-3.0-or-later, the exact same category
  # of build-wide license escalation GnuTLS's GPL/LGPL-3.0 GMP dependency
  # was already rejected for. OpenSSL >=3.0.0 hits a *different* branch of
  # the same configure logic that does not require --enable-version3 when
  # --enable-gpl is absent (confirmed by reading the actual condition:
  # `enabled gplv3 || ! enabled gpl || enabled nonfree || die ...` --
  # `! enabled gpl` alone satisfies it here), so it's the one Apache-2.0
  # backend that keeps this build at LGPL-2.1-or-later. Worth the extra
  # ~5.7MB for that; GnuTLS remains rejected for the reason above.
  --enable-openssl

  # --- Test-signal generation only (av://lavfi:sine=...), not real
  # playback -- so media/player.rs's existing #[ignore]d smoke tests can
  # run against this build instead of only the system mpv package (which
  # is what they were silently limited to before, per ADR 0007's prior
  # note that neither build-mpv.sh nor release.yml enabled libavdevice).
  # abuffer/abuffersink are what mpv's own lavfi bridge
  # (filters/f_lavfi.c) uses to feed/read any lavfi filter graph --
  # confirmed by reading that file directly, not assumed. astats is the
  # RMS-metering filter player.rs's enable_level_metering()/
  # audio_level_db() depend on for the orb's audio reactivity -- the
  # single highest-stakes filter in this whole build, since leaving it
  # out doesn't fail the build, it just kills the orb silently at
  # runtime. sine is the signal source av://lavfi:sine=... needs.
  # aformat/aresample are included so libavfilter's automatic
  # pad-format-negotiation fallback has somewhere to go if a future
  # filter graph ever needs it; both are libavfilter-internal, no extra
  # dependency.
  --enable-filter=abuffer,abuffersink,aformat,aresample,astats,sine
  --enable-indev=lavfi

  # Nothing else: no encoders, no muxers, no outdevs, no other indevs, no
  # hwaccels, no bitstream filters -- Echora only ever decodes and plays,
  # never encodes, remuxes, or captures.
)

# Unmodified-source mirror + the exact configure line, shipped alongside
# every release build for the LGPL §6 "corresponding source" obligation.
# Before this change, Echora redistributed Ubuntu's own FFmpeg .deb
# packages, and that obligation was satisfied by transitivity (Canonical
# already hosts the source package). Building FFmpeg ourselves removes
# that shortcut -- Echora becomes the sole distributor of this exact
# binary, so the obligation is Echora's now. release.yml uploads both of
# these as release assets; nothing about this configure line is ever
# hand-copied anywhere else, so it can't silently drift from what was
# actually built.
git archive --format=tar.gz --output="$PREFIX/echora-ffmpeg-source.tar.gz" "$FFMPEG_VERSION"
{
  echo "FFmpeg source: https://github.com/FFmpeg/FFmpeg, tag $FFMPEG_VERSION (unmodified)"
  echo "Unmodified source mirror: echora-ffmpeg-source.tar.gz (sibling of this file, this exact tag)"
  echo "License: LGPL-2.1-or-later (--enable-gpl/--enable-nonfree/--enable-version3 never passed)"
  echo "Configure line:"
  echo "./configure ${CONFIGURE_ARGS[*]}"
} >"$PREFIX/echora-ffmpeg-provenance.txt"

echo "Configure line: ./configure ${CONFIGURE_ARGS[*]}"
./configure "${CONFIGURE_ARGS[@]}"
make -j"$(nproc)"
make install

# Ubuntu's own FFmpeg packages ship stripped .so files (confirmed via
# `file` reporting "stripped" on the apt-installed libavcodec.so.60.31.102
# during this task) -- match that so this task's required before/after
# size comparison is apples-to-apples, and so debug symbols for code
# Echora never exercises don't quietly defeat the point of this change.
find "$PREFIX/lib" -name '*.so*' -type f -exec strip --strip-unneeded {} +

echo "Built FFmpeg $FFMPEG_VERSION into $PREFIX"
