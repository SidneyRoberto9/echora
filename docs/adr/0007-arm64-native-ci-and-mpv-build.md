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

## Update (2026-09-06): mpv didn't actually start on a clean machine — fixed, and a real lightness cost surfaced

A packaging audit flagged that `build-mpv.sh`'s runtime-lib filter
(`ldd ... | grep -E 'libav|libsw|libpostproc'`) only ever bundled 7 FFmpeg
libraries, while `release.yml` installs `libplacebo-dev`/`libass-dev` as
build prerequisites (added in the 2026-09-01 update above, to make the
*build* succeed) without ever bundling `libplacebo`/`libass` themselves
into the package. On a clean target machine — one without those `-dev`
packages' runtime counterparts already installed — the bundled mpv would
fail to start at all: `error while loading shared libraries:
libass.so.9: cannot open shared object file`. Verified by reproducing
this exact failure in a bare `ubuntu:24.04` Docker container with none of
the build's `-dev` packages present.

**Investigated first, per the audit's own instruction: can Echora just
not link libass/libplacebo, since it's audio-only?** Confirmed
`media/player.rs` never passes `--vo`, `--sub-*`, or any GPU/subtitle
flag — only `--idle=yes --no-video --no-terminal
--input-ipc-server=...`. But `meson setup build -Dlibass=disabled
-Dlibplacebo=disabled` was tried for real and **fails outright**:
`ERROR: Unknown options: "libass, libplacebo"`. Reading mpv 0.41.0's
`meson.build` directly confirms why: `libavcodec`, `libavfilter`,
`libavformat`, `libavutil`, `libswresample`, `libswscale`, `libplacebo`,
and `libass` are all `dependency(...)` calls with no `required:
get_option(...)` gate and no corresponding entry in `meson.options` at
all — they are unconditional, full stop, regardless of `-Dgl=disabled`/
`-Dvulkan=disabled`/`-Dx11=disabled`/etc. There is no way to build mpv
0.41.0 without linking both, short of patching mpv's own build system
and carrying that patch across every future version bump — a bigger,
open-ended maintenance burden than bundling their runtime dependency
chain. So: **not disabled, bundled instead**, per this ADR's own
existing "if a lib turns out to be necessary, bundle its transitive
deps" fallback.

**Bundling "just FFmpeg + libass + libplacebo" isn't enough either** —
Ubuntu 24.04's `libavcodec60`/etc. are themselves built with essentially
every optional codec, container, network-protocol, and text-shaping
feature FFmpeg supports (encoders Echora never invokes: libx264, libx265,
libaom, librav1e, libtheora, libwebp, five Flite TTS voices, PocketSphinx
speech recognition, several exotic network-transport libraries, and
more), all as hard `NEEDED` entries a real `ldd` confirms mpv's dynamic
loader must resolve just to *start* — whether or not the code path is
ever exercised. This isn't something `build-mpv.sh` chooses; it's
inherited from linking against the distro's monolithic shared FFmpeg
build rather than a custom minimal one.

**Fix implemented:** `build-mpv.sh`'s bundling filter is no longer a
hand-picked name list. It now bundles *everything* a real `ldd` reports,
except a short, explicitly justified exclude list: the C/C++ runtime
every ELF binary already needs to exist at all (`libc`, `libm`,
`libstdc++`, `libgcc_s`), and ALSA/PulseAudio (`libasound`,
`libpulse[common]`), which stay as `.deb` system dependencies rather than
bundled, matching how any other desktop-audio Linux app depends on them
(and because `libpulsecommon` is PulseAudio's own version-pinned private
plugin, not a normal SONAME dependency — bundling a copy that could drift
from the installed `pulseaudio` package would be worse than not bundling
it).

**Verified for real, not just reasoned about:** built `scripts/build-mpv.sh`
end-to-end on Ubuntu 24.04 x86_64 in this environment. The new filter
resolves to **170 libraries, ~220MB**. Installed the resulting bundle
(mpv binary + `lib/`) at the real `usr/bin/mpv` +
`usr/lib/echora/lib/*` layout into a bare `ubuntu:24.04` Docker container
with *no* build-time `-dev` packages present — only `apt-get install
libasound2t64 libpulse0` (see the `deb.depends` fix below) — and
confirmed: `ldd /usr/bin/mpv` reports zero `not found`, `mpv --version`
runs, and real WAV playback (`--no-video --no-terminal --ao=null
<file>.wav`) exits 0. As a negative control, deleting one bundled `.so`
(`libplacebo.so.338`) before the same check reproduces the exact original
failure and a non-zero exit — confirming the check is decisive, not
accidentally always-green. `.github/workflows/ci.yml`'s new
`package-smoke-test` job re-runs this same check on every real build
(weekly + on demand) rather than relying on this comment staying true
across mpv/Ubuntu version bumps.

**Separately confirmed and fixed: `deb.depends: ["libasound2", ...]` was
broken on the actual build/target OS.** Ubuntu 24.04 (`noble`)'s "t64"
64-bit-`time_t` package transition renamed the ALSA runtime package;
`libasound2` no longer exists in `noble`'s repos at all (`apt-cache
policy libasound2` → `Candidate: (none)`). An unversioned `Depends:
libasound2` doesn't just fail to install cleanly — worse, verified in a
clean container that `apt` silently resolves it to `liboss4-salsa-asound2`
(an OSS-compatibility shim providing a virtual `libasound2`, not the real
ALSA library), an unrelated package unlikely to be what's actually
installed on a real desktop. Fixed to the alternative-dependency syntax
`"libasound2t64 | libasound2"`, verified in the same container to
correctly prefer the real `libasound2t64` package over the OSS shim.

**Known, unresolved lightness cost — flagged, not silently accepted.**
~220MB of bundled libraries is a serious increase from the previous
(broken) ~34MB bundle, and sits in real tension with this project's
lightness priority. This is *not* something a smarter `ldd` filter can
fix further — every one of those 170 libraries is a genuine hard runtime
dependency of the mpv binary as currently built. Reducing it for real
would mean either (a) building a minimal custom FFmpeg from source with
`--disable-everything` plus only the specific decoders/protocols/muxers
Echora's real playback path needs, replacing reliance on Ubuntu's
kitchen-sink shared FFmpeg package, or (b) patching mpv's own
`meson.build` to make `libass`/`libplacebo` genuinely optional and
maintaining that patch across mpv version bumps. Both are materially
bigger undertakings than this fix's scope (a packaging-script/CI/config
change) and change what this ADR's own toolchain choice implies — they
need their own explicit decision, not a quiet default here.

**Separately noticed, not fixed (out of this change's file scope):**
neither `build-mpv.sh` nor `release.yml`'s mpv build-prerequisite list
installs `libavdevice-dev`, so the CI-built mpv can't open `av://`
sources (`[lavf] Unknown lavf format lavfi`). This doesn't affect real
playback — Echora only ever loads real HTTP(S) media URLs via
`loadfile`, never `av://` — but it does mean `media/player.rs`'s
`#[ignore]`d local smoke tests that use `"av://lavfi:sine=..."` as a
synthetic signal source would fail if ever run against a CI-built mpv
instead of the system `mpv` package they currently rely on for local dev.

## Update (2026-09-06): minimal custom FFmpeg replaces the distro's
monolithic one — the "known, unresolved lightness cost" above is resolved
Option (a) from that unresolved-cost note above was built:
`scripts/build-ffmpeg.sh` (new) compiles FFmpeg `n6.1.6` from source with
`--disable-everything`, then enables only the specific decoders,
demuxers, protocols, and filters Echora's real playback path
(`src-tauri/src/media/{resolver,metadata,player}.rs`) needs, plus a
deliberate small margin — see that script's own comments for the full,
grouped justification of every flag. `scripts/build-mpv.sh` builds this
first, into a private prefix, and points mpv's meson build at it via
`PKG_CONFIG_PATH` (searched before system pkgconfig dirs) instead of
linking Ubuntu's `libavcodec60`/etc.

**What's enabled and why, briefly** (full reasoning lives in
`scripts/build-ffmpeg.sh`, not duplicated here so it can't drift out of
sync with the actual flags): native decoders for opus/aac/vorbis/mp3/
flac/pcm (bestaudio is Opus-in-WebM or AAC-in-MP4 in practice — confirmed
against the real yt-dlp binary in this task; the rest is margin, all
native FFmpeg code, no external codec libs at all); demuxers for the
containers those arrive in, including `hls`; protocols http/https/tcp/
tls/crypto; `avdevice` + the `lavfi` indev + `abuffer`/`abuffersink`/
`astats`/`sine`/`aformat`/`aresample` filters (test-signal generation and
the orb's RMS-metering filter, not real playback). No `--enable-gpl`,
`--enable-nonfree`, ever.

**Livestream decision, made explicit per this task's own instruction not
to leave it implicit:** yt-dlp's `ytsearch` can return a currently-live
stream (confirmed for real in this task — resolving
`youtube.com/watch?v=rFZHOHl-L8A`, a live 24/7 lofi stream, via
`yt-dlp -f bestaudio` returns `protocol=m3u8_native`, an HLS manifest, not
a progressive URL). **Decided: support it.** Live ambient/lofi radio is
exactly the content class a mood-first player exists for, and declining
would silently break playback for anything `ytsearch` can return live.
Cost: the `hls` demuxer (which force-selects `mpegts`/`mov` internally)
plus the protocols already needed for the non-live case.

**TLS backend: OpenSSL 3, not Mbed TLS.** Mbed TLS was the first choice —
smallest (~0.9MB installed on Ubuntu 24.04: `libmbedtls14t64` +
`libmbedcrypto7t64` + `libmbedx509-1t64` = 227+528+160KB via
`apt-cache show`) and nominally Apache-2.0, self-contained, no transitive
dependency chain. **Actually running `./configure --enable-mbedtls`
against this build failed outright**: `mbedtls is version3 and
--enable-version3 is not specified` — verified for real against the real
`libmbedtls-dev 2.28.8` in Ubuntu 24.04's own repos (not a newer-mbedtls-
only edge case; FFmpeg's `configure` hard-codes `mbedtls` into
`EXTERNAL_LIBRARY_VERSION3_LIST` unconditionally, for any version,
reflecting the FSF's own position that Apache-2.0 is GPLv3/LGPLv3-
compatible but *not* GPLv2/LGPLv2.1-compatible). Passing
`--enable-version3` would have "fixed" the build error but upgraded
*this entire FFmpeg build* from LGPL-2.1-or-later to LGPL-3.0-or-later as
a build-wide condition — the exact same category of license escalation
GnuTLS's GPL/LGPL-3.0-licensed GMP dependency was already rejected for,
just via a different, non-obvious mechanism only the real build attempt
surfaced. OpenSSL ≥3.0.0 hits a different branch of that same configure
check that does *not* require `--enable-version3` when `--enable-gpl` is
absent (confirmed by reading the condition directly:
`enabled gplv3 || ! enabled gpl || enabled nonfree || die ...`), so it's
the Apache-2.0 backend that actually keeps this build at
LGPL-2.1-or-later. Measured cost: `libssl3t64`'s 6615KB installed vs.
Mbed TLS's ~915KB — paid gladly to avoid the license escalation. GnuTLS
remains rejected for its own, separate reason (GMP).

**Real before/after measurement, this task, same environment (Ubuntu
24.04 x86_64, in a Docker container mirroring `ci.yml`'s
`package-smoke-test` prerequisites exactly, including the pre-existing
system `libavcodec-dev` et al. left installed specifically to prove
`PKG_CONFIG_PATH` wins over them):**

| | Before (system FFmpeg) | After (minimal custom FFmpeg) |
|---|---|---|
| Bundled library count | 170 | 52 |
| Bundled `lib/` size | 219MB (229,303,560 bytes) | 33MB (34,474,768 bytes) |

**`scripts/build-mpv.sh` now asserts this itself, every run, not just
once:** after `meson compile`, it resolves mpv's own `libavcodec.so` via
`ldd`, fails the build if that path isn't under the FFmpeg prefix just
built (the exact "PKG_CONFIG_PATH lost to the system copy" failure mode
this whole change exists to prevent), and separately fails if that
resolved `libavcodec` itself pulls in any GPL/nonfree codec lib
(`libx264`/`libx265`/`libvpx`/`libaom`/`libdav1d`/`librav1e`) that a
`--disable-everything` build can never legitimately have.

**Clean-container smoke test re-run with the new bundle:** same
methodology as the "Verified for real" paragraph above (bare
`ubuntu:24.04`, only `libasound2t64`/`libpulse0` installed, no `-dev`
packages) — `ldd` reports zero "not found", `mpv --version` runs, real
WAV playback (`--ao=null`) exits 0. Negative control repeated too:
deleting the bundled `libplacebo.so.338` before the same check correctly
reproduces the exact failure and non-zero exit.

**`astats` proof — the single highest-stakes check in this whole
change, since a build missing it doesn't fail loudly, it just kills the
orb's audio reactivity at runtime.** Drove the new mpv binary over its
own JSON IPC socket with the exact command sequence
`media/player.rs::enable_level_metering`/`audio_level_db` use: loaded
`av://lavfi:sine=frequency=440:duration=10` (proving the `lavfi` indev +
`sine` filter, enabled specifically so `player.rs`'s `#[ignore]`d smoke
tests can finally run against a CI-built mpv instead of only the system
package — this also resolves this ADR's own previously "separately
noticed, not fixed" `libavdevice` gap; the built `avdevice` reports
version `60.3.100`, satisfying mpv's own minimum exactly), ran
`af add @echora_level:lavfi=[astats=metadata=1:reset=1]`, then
`get_property af-metadata/echora_level`. Real reply:
`lavfi.astats.Overall.RMS_level: "-21.014229"` (string-typed, exactly as
`player.rs`'s own comment describes) — a real, finite RMS value, not a
build that merely compiles.

**https proof:** resolved a real, live YouTube video via the real
`yt-dlp`/Deno binaries in `src-tauri/binaries/dev/` (`-f bestaudio` →
`acodec=opus, ext=webm, protocol=https`, a real `googlevideo.com` URL),
loaded it into the new mpv over IPC, and confirmed `time-pos` advancing
against a real wall clock with `duration` matching yt-dlp's own reported
`19.021`s exactly.

**HLS livestream proof (the deliberate decision above, verified, not
just decided):** resolved the same live lofi stream
(`rFZHOHl-L8A`) yt-dlp's `flat-playlist` fixture already uses in this
project's own tests, got a real `m3u8_native` manifest URL, loaded it
into the new mpv, and confirmed `time-pos` starts advancing from a real
live HLS stream.

**Smaller, separate, *not* acted on in this change — flagged, not
fixed:** roughly 2.9MB of the 33MB bundle (`libsndfile`, `libFLAC`,
`libvorbis`/`libvorbisenc`, `libopus`, `libogg`, `libmpg123`,
`libmp3lame`) is reachable only via PulseAudio's own private
`libpulsecommon-16.1.so` plugin (confirmed via `readelf -d`: none of
Echora's own libavcodec/libass/libplacebo need any of them) — the exact
same "PulseAudio's own version-pinned private plugin, don't bundle a
copy that could drift" situation this ADR already excludes
`libpulsecommon` itself for, just one hop further down that plugin's own
dependency graph, currently un-excluded. A real system with PulseAudio
installed already has these via `libpulse0`'s own apt `Depends` chain,
making Echora's bundled copies redundant. Out of this change's scope
(the task was FFmpeg's own codec/protocol footprint, not PulseAudio's) —
noted for a future, separate pass over `build-mpv.sh`'s exclude list.

**AppImage soname-collision risk — open, not resolved by this change,
and not silently assumed safe.** This ADR's own "AppImage: verified
working, but with a caveat worth tracking" note (above) already recorded
that `linuxdeploy` rewrites mpv's rpath from `$ORIGIN/../lib/echora/lib`
to its own flat `$ORIGIN/../lib`, where it separately auto-bundles *its
own* copies of the same sonames (`libavcodec.so.60` et al.) for
WebKitGTK/GStreamer's own use — and explicitly flagged that the two
copies being identical was "coincidental, not structurally guaranteed."
With a minimal custom FFmpeg, that coincidence is gone for good: the two
copies are now definitely *not* identical, and whichever one
`linuxdeploy` actually places at that shared path is invisible from the
outside (mpv still starts either way — only its actual codec/library
footprint differs). Two real failure modes: mpv silently gets the full
system copy (this task's size win doesn't actually ship), or
WebKitGTK/GStreamer silently gets the minimal copy (missing codecs they
expect, breaking something unrelated and hard to trace back to this
change). **Not verified in this task's environment**: building the real
signed AppImage via `tauri-action`/`cargo tauri build` requires
`TAURI_SIGNING_PRIVATE_KEY`, which this task's environment doesn't have
access to (it's a CI secret) — attempted a local build to check this
directly and it did not complete in the time available; see this task's
own report for exactly what was and wasn't reached. **What resolves this
gap:** `.github/workflows/ci.yml`'s `package-smoke-test` job now has a
dedicated step, `Smoke test the AppImage's mpv against the wrong FFmpeg
(soname-collision check)`, that extracts the real built AppImage
(`--appimage-extract`, no FUSE needed) and fails the job if the AppImage's
`mpv` resolves a `libavcodec` that pulls in any GPL/nonfree codec lib a
minimal build can't have (`libx264`/`libx265`/`libvpx`/`libaom`/
`libdav1d`/`librav1e`) — the same decisive signature used to verify the
`.deb` path above. That job is gated to `workflow_dispatch`/the weekly
`schedule` (real packaging is expensive), so this will get a real answer
the next time it runs, on a runner with the signing secret available —
until then, treat this as an open risk, not a closed one.

**LGPL §6 corresponding-source obligation, now Echora's to carry
directly:** building FFmpeg from source instead of redistributing
Ubuntu's own `.deb` packages removes the transitivity that used to
satisfy this (Canonical hosting the source package). `release.yml` now
uploads, per release, per architecture: the exact pinned tag (via the
same `grep` pattern already used for mpv/yt-dlp/Deno), an unmodified
source mirror (`git archive` of that exact tag, generated by
`scripts/build-ffmpeg.sh` itself), and the exact `./configure` line used
(also generated by that script, from the same array it actually invokes
`./configure` with — never hand-copied elsewhere, so it can't silently
drift from what was actually built).

## Update (2026-09-06): ARM64 dropped from the CI/release matrix
No ARM users. `ci.yml`'s `build`/`package-smoke-test` jobs and
`release.yml`'s `release` job all had their `arm64`/`aarch64`
(`ubuntu-24.04-arm`) matrix entries removed, leaving a single `x86_64`
entry — the matrix structure itself (and every script's
`<target-triple>` parameterization) is untouched, so bringing ARM64 back
is re-adding one matrix entry per workflow (each removal is commented
in-place with the exact entry to restore), not rewriting the pipeline.
Everything above in this ADR about *how* to build mpv/FFmpeg for ARM64
(native runner, no cross-compilation, no QEMU) remains accurate if/when
that entry comes back.

## Consequences
- mpv becomes a build artifact Echora's own CI produces and
  checksum-tracks per release, not a binary fetched from a third party —
  more reproducible, but Echora now owns keeping that build working
  across mpv version bumps.
- No paid CI minutes needed for ARM64 (free hosted runner, public repo).
- `.deb` cross-compilation (which does work from x86_64) is not used
  either, for consistency — both formats, both architectures, build
  natively in their own matrix job.
