# Packaging — Design

Status: Approved. Fifth and final item of the post-v1-audit feature queue
(SponsorBlock → Auto-update → Crash report → Packaging), per the user's
priority order. This is the last piece Auto-update was waiting on (its
placeholder pubkey, and the release pipeline that publishes `latest.json`).

## Purpose

Ship real, installable `.deb` and AppImage packages for `x86_64` and
ARM64, per `docs/REQUIREMENTS_FREEZE.md`'s "zero install" requirement —
mpv, yt-dlp, and Deno bundled inside Echora's own package, nothing the
user needs to `apt install` separately. Wire the CI release pipeline
(triggered by a `vN.N.N` git tag) that builds, signs, and publishes both
formats for both architectures to GitHub Releases with a `latest.json`
manifest, so Auto-update (already implemented, currently pointed at a
release that doesn't exist yet) has something real to find.

The real updater signing keypair already exists — the user generated it
themselves (`cargo tauri signer generate`) mid-brainstorming for this
spec, and the public half is already committed
(`src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`, commit
`e681aed`). The private key and its password live only in the user's
own storage until they're added as GitHub Actions repository secrets
(`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`) —
never touched by this session, never committed.

## Non-goals

- No Windows/macOS packaging — Linux only, per the freeze.
- No automated version-bump tooling. The user bumps `package.json`,
  `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`'s `version`
  fields by hand, commits, then pushes the matching `vN.N.N` tag. CI
  only *verifies* the tag matches `tauri.conf.json`'s version (fails
  loudly if not) — it never edits or commits anything back to the repo.
- No changelog automation — GitHub's own auto-generated release notes
  (from the commit/PR history since the last tag) are enough for v1.
- No `bundleMediaFramework` in the AppImage config — that bundles
  gstreamer for the WebView's own `<audio>`/`<video>` elements, which
  Echora never uses (all playback goes through the mpv sidecar over its
  own IPC socket, never through the WebView).
- This task does not touch SponsorBlock's pre-existing, unrelated rustls
  `CryptoProvider` panic (flagged during Crash report's final review) —
  tracked separately.

## Component 1: mpv built from source per architecture (CI)

Per `docs/adr/0007-arm64-native-ci-and-mpv-build.md`'s already-accepted
decision: no portable upstream mpv binary exists for both architectures,
so CI builds it from source, natively, once per architecture.

- Pin mpv `v0.41.0` (current stable at spec time — confirm still current
  at implementation time before pinning, same posture prior specs in
  this project have taken with fast-moving upstream versions).
- Build with Meson, audio-only feature set: `-Dgl=disabled
  -Dvulkan=disabled -Dx11=disabled -Dwayland=disabled -Dcocoa=disabled
  -Dwin32-desktop=disabled`, ALSA + PulseAudio backends left enabled
  (`-Dalsa=enabled -Dpulse=enabled`).
- `patchelf --force-rpath --set-rpath '$ORIGIN/lib'` on the resulting
  binary, and copy its runtime shared library dependencies (FFmpeg's
  `libavcodec`/`libavformat`/`libavutil`/`libswresample`, whatever
  version the build produced) into a `lib/` directory next to it —
  `--force-rpath` (not the default `--set-rpath`, which only touches the
  lower-priority `DT_RUNPATH`) so the relocatable path actually wins at
  load time, matching `linuxdeploy`'s own technique per ADR 0007.
- `libasound2`/`libpulse0` (the ALSA/PulseAudio *client* libraries, not
  FFmpeg's decode libraries) are **not** bundled — they're standard
  Ubuntu/Zorin base-system libraries already present on any real desktop
  install, declared as ordinary `.deb` dependencies instead (see
  Component 4). This isn't a "zero install" violation — that requirement
  is about not asking the user to separately install mpv/yt-dlp/Node/
  Python, not about excluding libraries every Linux desktop already has.
- Result: `mpv-x86_64-unknown-linux-gnu` and `mpv-aarch64-unknown-linux-gnu`
  (Tauri's required `externalBin` naming, see Component 4) plus each
  one's `lib/` sibling directory, produced by the matching CI matrix job
  and placed under `src-tauri/binaries/`.
- patchelf itself is architecture-agnostic (operates on ELF files, not
  native code) — the same command sequence works unchanged on both the
  `ubuntu-24.04` (x86_64) and `ubuntu-24.04-arm` (ARM64) runners.

## Component 2: production sidecar wiring — `externalBin` + `tauri-plugin-shell`

**This is the component with the most real code change**, discovered
mid-brainstorming: Tauri 2.x has no documented API to resolve an
`externalBin` sidecar's filesystem path without going through
`tauri-plugin-shell`'s `app.shell().sidecar(name)` — there is no
`PathResolver`-style call for it the way there is for `bundle.resources`
(confirmed against `tauri-apps/tauri#15134`, which flags this exact gap
as undocumented upstream). Tauri v1's `Command::new_sidecar()` was
removed outright in v2; the shell plugin is the only supported path.

### New dependency

`tauri-plugin-shell` (Rust crate, `Cargo.toml`) + `@tauri-apps/plugin-shell`
if any frontend-side shell interaction were needed (it isn't — Echora's
own IPC to mpv and Rust-side spawning of yt-dlp stay entirely in Rust,
so only the Rust crate is needed). MIT/Apache-2.0 licensed, official
Tauri plugin — no new licensing review needed (ADR 0006 already covers
the sidecar *processes* themselves; this is just the mechanism Rust uses
to launch them, not a new bundled runtime dependency).

### `Player` (`src-tauri/src/media/player.rs`)

Only talks to mpv over its own Unix IPC socket — never reads mpv's
stdout/stderr (already `Stdio::null()` for both). Minimal change:

- `Player::new` takes an `AppHandle` (or `tauri::Manager`-bound generic)
  instead of a plain `mpv_path: PathBuf`.
- `start()` spawns via `app.shell().sidecar("mpv")?.args([...]).spawn()`
  instead of `tokio::process::Command::new(mpv_path).spawn()`. This
  returns `(Receiver<CommandEvent>, CommandChild)` — the event receiver
  is dropped/ignored (nothing to read; mpv's own IPC socket is the real
  channel), `CommandChild` replaces the stored `child: Option<Child>`
  field's type.
- `shutdown()`'s `child.kill()` call becomes synchronous —
  `CommandChild::kill(self) -> Result<(), Error>` is not `async`, unlike
  `tokio::process::Child::kill()`.
- The reactive sidecar-crash-detection logic added by the Crash report
  feature (`send_command`'s `UnixStream::connect` failure path) is
  unaffected — it never touched the `Child`/`CommandChild` type directly,
  only `self.child.is_some()`/`= None`.

### `Resolver` (`src-tauri/src/media/resolver.rs`)

**Correction to what was initially assumed mid-brainstorming**: this
module does *not* read yt-dlp's stdout incrementally line-by-line — its
`run()` method calls `child.wait_with_output()`, which waits for process
exit and returns the fully-buffered stdout/stderr as one blob. The
rewrite is smaller than first feared:

- `Resolver::new` takes an `AppHandle` instead of nothing extra beyond
  its existing `ResolverConfig` (which keeps `deno_path: PathBuf` — see
  below) and `timeout`.
- `run()` spawns via `app.shell().sidecar("yt-dlp")?.args(&args).spawn()`,
  getting `(mut rx, child)`. Replace `wait_with_output()` with a loop:
  accumulate `CommandEvent::Stdout(bytes)`/`CommandEvent::Stderr(bytes)`
  chunks into two `Vec<u8>` buffers as they arrive, until
  `CommandEvent::Terminated(payload)` — then check `payload`'s exit
  status exactly like the current `output.status.success()` check, and
  treat the accumulated buffers exactly like today's
  `output.stdout`/`output.stderr`. Wrap the whole receive-loop future in
  the same `tokio::time::timeout(self.config.timeout, ...)` that already
  guards `run()` today — same behavior, same `EchoraError::SidecarTimeout`
  on expiry.
- `--js-runtimes deno:<path>` still needs deno's real filesystem path as
  a CLI argument string — yt-dlp spawns deno itself, Echora's Rust code
  never does. See the open risk below for how that path gets resolved
  in an installed build.

### `SidecarPaths` (`src-tauri/src/media/sidecar_paths.rs`)

Becomes obsolete for mpv and yt-dlp (their real paths are never needed —
only spawned by name via the shell plugin, which resolves paths
internally). **Not** obsolete for Deno — see the open risk immediately
below. Rename/narrow this module to just resolving Deno's path, or fold
that one function directly into `Resolver`.

### Open risk — Deno's resolved path in an installed build

No documented Tauri API returns a bare `PathBuf` for an `externalBin`
entry. The available community knowledge points at predictable-but-
unconfirmed conventions: a `.deb` install places `externalBin` binaries
alongside the main executable (both end up under `/usr/bin/`), and an
AppImage's mounted runtime exposes its own root via the `$APPDIR`
environment variable (Echora's own `is_appimage_build` command already
reads a sibling AppImage-runtime env var, `APPIMAGE`, so this project's
code already leans on that convention elsewhere). The likely correct
approach: resolve Deno's path relative to `std::env::current_exe()`'s
parent directory for a `.deb` install, and relative to `$APPDIR` for an
AppImage — but **this must be confirmed by actually building a real
`.deb` and AppImage and inspecting where the bundled `externalBin`
files land**, not assumed from documentation or reasoning alone. This
project has been burned twice already (SponsorBlock's TLS backend,
Auto-update's bundler flag) by runtime-sensitive assumptions that looked
right in code/docs but weren't verified against a real build. The
implementation plan must include an explicit step that builds a real
package locally (or in a scratch CI run) and confirms the resolved path
before the "happy path" code is trusted.

### Dev-mode implications

`.sidecar("mpv")`/`.sidecar("yt-dlp")` resolve `externalBin` entries the
same way in dev as in a bundled build — relative to
`src-tauri/binaries/<name>-<target-triple>` — so dev binaries move from
today's `src-tauri/binaries/dev/{yt-dlp_linux,deno}` (ad hoc names) to
the same target-triple-suffixed convention as production:
`src-tauri/binaries/yt-dlp-x86_64-unknown-linux-gnu`,
`src-tauri/binaries/mpv-x86_64-unknown-linux-gnu`, etc. (ARM64 dev
machines get their own triple-suffixed set; most contributors won't need
both). This is a one-time local dev-setup change, documented in
whatever the project's dev-setup docs are (README or CONTRIBUTING, not
yet checked as part of this spec).

## Component 3: signing keypair

Already done (see Purpose) — `cargo tauri signer generate`'s public key
is committed at `src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`.
Remaining steps are the user's own, outside this codebase: add
`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as
GitHub Actions repository secrets before the release workflow's first
real run.

## Component 4: `tauri.conf.json` bundle config

```json
{
  "bundle": {
    "active": true,
    "targets": "all",
    "externalBin": [
      "binaries/mpv",
      "binaries/yt-dlp",
      "binaries/deno"
    ],
    "createUpdaterArtifacts": true,
    "linux": {
      "deb": {
        "depends": ["libasound2", "libpulse0"]
      }
    },
    "icon": [ ... unchanged ... ]
  }
}
```

`createUpdaterArtifacts: true` was present in Auto-update's original
design but had to be removed during that feature's implementation — it
broke `cargo tauri build`/dev builds because no real bundle inputs
existed yet (see `docs/superpowers/specs/2026-09-01-auto-update-design.md`'s
Non-goals). Packaging is exactly the point where those inputs (the 3
sidecar binaries) start existing, so it's safe to re-add here — but per
this project's runtime-verification lesson, confirm a real `cargo tauri
build` still succeeds locally before trusting it in CI.

## Component 5: `capabilities/default.json`

Add sidecar-scoped shell permissions, naming each binary explicitly (no
catch-all shell execute permission):

```json
{
  "permissions": [
    "core:default",
    "core:window:allow-close",
    "core:window:allow-start-dragging",
    "opener:default",
    "updater:allow-check",
    "updater:allow-download-and-install",
    "process:allow-restart",
    {
      "identifier": "shell:allow-spawn",
      "allow": [
        { "name": "binaries/mpv", "sidecar": true },
        { "name": "binaries/yt-dlp", "sidecar": true }
      ]
    }
  ]
}
```

`deno` is not spawned by Echora's own Rust code (yt-dlp spawns it), so
it needs no `shell:allow-spawn` entry — only its `externalBin` bundling
entry in Component 4.

## Component 6: `.github/workflows/release.yml` (new file)

Separate from the existing `ci.yml` (which keeps running on every push/PR
— `cargo build --release` only, no packaging). New workflow:

- Trigger: `on: push: tags: ['v*.*.*']`.
- Matrix: `x86_64` on `ubuntu-24.04`, `arm64` on `ubuntu-24.04-arm` — same
  split ADR 0007 already specifies for the mpv build.
- Each matrix job, in order: install Tauri Linux build prerequisites
  (same apt packages `ci.yml` already installs) plus mpv's own build
  dependencies (Meson, FFmpeg dev headers, `patchelf`) → build mpv from
  source (Component 1) → download yt-dlp/Deno's official release
  binaries for that job's architecture, verify against the checksums
  upstream publishes (SHA256SUMS for yt-dlp; Deno's release checksums)
  → a step that reads the git tag, strips the `v` prefix, and fails the
  job if it doesn't exactly match `tauri.conf.json`'s `version` field →
  run `tauri-apps/tauri-action@v1` with `TAURI_SIGNING_PRIVATE_KEY`/
  `_PASSWORD` from repository secrets and `uploadUpdaterJson: true`,
  publishing `.deb` + AppImage + `latest.json` to a GitHub Release
  named from the tag.
- Release is published (not draft, not prerelease) directly on a
  successful matrix run — the tag push itself is the deliberate release
  signal per the earlier trigger decision; no extra manual approval step.
- If either matrix job fails, nothing is published for that
  architecture — GitHub Releases created by `tauri-action` from a failed
  workflow don't get artifacts attached, so a `.deb` existing for one
  architecture and not the other (mid-failure) is visible/obvious, not
  silently broken.

## Error handling / edge cases

- Tag/version mismatch → workflow fails before any build step runs, no
  partial release.
- Checksum mismatch on a downloaded yt-dlp/Deno binary → job fails
  before packaging, never ships an unverified binary.
- mpv build failure on one architecture → only that matrix job fails;
  the other architecture's release artifacts, if its job already
  succeeded, are unaffected (GitHub Actions matrix jobs are independent).
- Deno path resolution failing at runtime (the open risk above) surfaces
  as yt-dlp's own `--js-runtimes` argument pointing at a nonexistent
  path — yt-dlp already returns a classifiable failure in that case,
  which `Resolver`'s existing `classify_ytdlp_failure` error handling
  already covers; no new error path needed, just verifying the *input*
  path is actually correct.

## Testing

Not testable with `cargo test`/`npm test` — this is CI/build
infrastructure. Real verification means actually running it:

- Before trusting the release workflow against a real tag: run it (or
  the packaging steps of it) against a scratch/test tag
  (e.g. `v0.1.1-test`) on a throwaway branch, download the resulting
  `.deb` and AppImage, and confirm: the package installs, the app
  launches, mpv actually plays audio (the sidecar chain works end to
  end, not just "the binary exists"), yt-dlp resolves a real track
  (confirms Deno's path was resolved correctly), and Auto-update's
  "Check for Updates" finds the published `latest.json`.
- `Resolver`'s existing `#[ignore]` smoke tests (already real,
  network/binary-dependent) continue to be the correctness check for
  yt-dlp/Deno's actual resolution behavior after the shell-plugin
  rewrite — rerun them against the new production-shaped binary layout
  (triple-suffixed names under `src-tauri/binaries/`), not just the old
  `binaries/dev/` layout, to catch anything the rewrite changed.
- `Player`'s existing `#[ignore]` smoke tests, same treatment.
