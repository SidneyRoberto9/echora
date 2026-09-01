# Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship real `.deb`/AppImage packages for x86_64 + ARM64 with mpv
built from source, yt-dlp/Deno bundled as production sidecars, a real
updater signing keypair (public half already committed), and a CI
release pipeline triggered by a `vN.N.N` git tag.

**Architecture:** mpv/yt-dlp/Deno become Tauri `externalBin` sidecars,
spawned via `tauri-plugin-shell` (the only Tauri-2-supported way to
invoke a bundled `externalBin`, confirmed — no path-only resolution API
exists). `Player`/`Resolver` stay concrete, non-generic structs; only
the methods that actually spawn a process become generic over
`R: tauri::Runtime`, taking `&tauri::AppHandle<R>` as a parameter —
this keeps `AppState` and every command handler untouched, and lets
tests pass a `tauri::test::mock_builder()`-produced `AppHandle<MockRuntime>`
without genericizing the whole app. mpv is built from source per
architecture in CI (per `docs/adr/0007`); yt-dlp/Deno are downloaded
from their official releases and checksum-verified. `tauri-apps/tauri-action`
runs the actual build+sign+publish in a 2-job architecture matrix.

**Tech Stack:** Rust, `tauri-plugin-shell` 2.x, GitHub Actions
(`tauri-apps/tauri-action` v1, native `ubuntu-24.04`/`ubuntu-24.04-arm`
runners), Meson (mpv build), `patchelf`.

**Spec:** `docs/superpowers/specs/2026-09-01-packaging-design.md`

## Global Constraints

- No new dependency beyond `tauri-plugin-shell` (Rust, official Tauri
  plugin, MIT/Apache-2.0 — no new licensing review needed).
- The updater signing public key is already committed
  (`src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`, commit
  `e681aed`) — do not touch it. The private key/password are the user's
  own GitHub Actions secrets, entirely outside this plan's scope.
- No automated version-bump tooling — CI only _verifies_ the pushed
  tag matches `tauri.conf.json`'s `version`, never edits/commits.
- `libasound2`/`libpulse0` are declared `.deb` dependencies (standard
  system libraries, not a "zero install" violation) — never bundled.
- **Known, accepted testing gap**: after this plan, the existing
  `#[ignore]` real-process smoke tests in `player.rs`/`resolver.rs`
  that spawn mpv/yt-dlp for real can no longer run under `cargo test`
  — a confirmed, unresolved upstream Tauri bug
  (`tauri-apps/tauri#13767`) makes sidecar spawning fail inside the
  `cargo test` harness (it looks for the sidecar binary in
  `target/debug/deps/` instead of `target/debug/`), even with
  `tauri::test::mock_builder()`. This is a deliberate, ruled-on
  trade-off (see spec) — do not attempt to work around the upstream
  bug in this plan. Real verification of actual sidecar spawning moves
  to manual `npm run tauri dev` testing (the user's own terminal, never
  launched by an agent).
- Before claiming any task done: `cargo fmt --check && cargo clippy
--all-targets -- -D warnings && cargo test` (backend tasks).
- Never run `git commit` directly — every task's commit step goes
  through the `auto-commit` skill instead (project rule, `~/.claude/CLAUDE.md`).

---

## Task 1: Dependencies, config, and dev binary layout

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/lib.rs` (plugin registration only — not the
  `SidecarPaths`/`Player`/`Resolver` construction, that's Tasks 2-3)
- Create/move: dev sidecar binaries under `src-tauri/binaries/`

**Interfaces:**

- Produces: `tauri_plugin_shell::init()` registered as a Tauri plugin;
  `externalBin` entries `binaries/mpv`, `binaries/yt-dlp`,
  `binaries/deno` declared in `tauri.conf.json`; real sidecar binaries
  present on disk at `src-tauri/binaries/{mpv,yt-dlp,deno}-x86_64-unknown-linux-gnu`
  for local dev/testing. Tasks 2 and 3 depend on all of this existing
  before their own `cargo build`/test runs can succeed.

- [ ] **Step 1: Add `tauri-plugin-shell` and the `tauri` test feature**

In `src-tauri/Cargo.toml`, add to `[dependencies]` (keep the existing
`tauri` line with `tray-icon` unchanged):

```toml
tauri-plugin-shell = "2"
```

Add a new `[dev-dependencies]` section (Cargo unifies feature sets for
the same crate across dependency kinds within one build, so this
activates `tauri`'s `test` feature only when running `cargo test`, never
in a release build):

```toml
[dev-dependencies]
tauri = { version = "2", features = ["test"] }
```

- [ ] **Step 2: Register the shell plugin**

In `src-tauri/src/lib.rs`, add to the plugin chain (order among plugins
doesn't matter — add it next to the others):

```rust
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
```

- [ ] **Step 3: Add `externalBin`, `createUpdaterArtifacts`, and `.deb` deps to `tauri.conf.json`**

In `src-tauri/tauri.conf.json`'s `bundle` section, replace:

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
```

with:

```json
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
    "icon": [
```

(The rest of the `icon` array and the file's closing braces are
unchanged.)

- [ ] **Step 4: Scope the shell-spawn capability to exactly the 2 spawned sidecars**

In `src-tauri/capabilities/default.json`, add one entry to the
`permissions` array (after `"process:allow-restart"`):

```json
    "process:allow-restart",
    {
      "identifier": "shell:allow-spawn",
      "allow": [
        { "name": "binaries/mpv", "sidecar": true },
        { "name": "binaries/yt-dlp", "sidecar": true }
      ]
    }
```

Deno is never spawned by Echora's own Rust code (yt-dlp spawns it as
its own child process) — it needs no `shell:allow-spawn` entry, only
its `externalBin` bundling entry from Step 3.

- [ ] **Step 5: Migrate dev sidecar binaries to the target-triple naming convention**

Tauri's `externalBin`/sidecar resolution (used in dev mode too, not
just bundled builds) expects binaries at
`src-tauri/binaries/<name>-<target-triple>`, not the current ad hoc
`src-tauri/binaries/dev/` layout. This machine is x86_64
(`x86_64-unknown-linux-gnu` triple) — confirm with `uname -m` before
running these commands, and adjust the triple suffix if this environment
is actually aarch64:

```bash
mkdir -p src-tauri/binaries
mv src-tauri/binaries/dev/yt-dlp_linux src-tauri/binaries/yt-dlp-x86_64-unknown-linux-gnu
mv src-tauri/binaries/dev/deno src-tauri/binaries/deno-x86_64-unknown-linux-gnu
ln -s "$(command -v mpv)" src-tauri/binaries/mpv-x86_64-unknown-linux-gnu
rmdir src-tauri/binaries/dev
chmod +x src-tauri/binaries/yt-dlp-x86_64-unknown-linux-gnu src-tauri/binaries/deno-x86_64-unknown-linux-gnu
```

(`src-tauri/binaries/*` is already fully gitignored except a
`.gitkeep` — confirm with `git status --porcelain` that this move
doesn't show up as anything to commit.) If this environment has no
system `mpv` on `PATH`, note that explicitly instead of creating a
broken symlink — Task 2/3's real-process manual verification will need
a real `mpv` present.

- [ ] **Step 6: Verify**

Run: `cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -40`
— this is the first real signal for a genuinely open question: does
Tauri's build-time tooling (via `tauri-build`'s `build.rs`, which runs
on every `cargo build`, not just `cargo tauri build`) validate that
`externalBin`-declared files actually exist on disk? If the build fails
citing a missing sidecar file, that _proves_ Step 5's binary placement
is mandatory for the crate to even compile — record that finding in your
report either way (pass or fail), don't just report the end state.

Then run the full gate: `cargo fmt --check && cargo clippy --all-targets
-- -D warnings && cargo test` (from `src-tauri/`). Expect these to pass
unchanged from before this task — nothing in `player.rs`/`resolver.rs`
has been touched yet, so the existing (pre-Packaging) test count and
behavior should be identical; this task only adds unused-so-far plugin
registration and config.

- [ ] **Step 7: Commit**

Never run `git commit` directly — stage `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`,
`src-tauri/src/lib.rs`, then invoke the `auto-commit` skill. Suggested
message: `feat(packaging): register shell plugin, externalBin, dev binary layout`.
(The dev binary moves under `src-tauri/binaries/` are gitignored and
won't be part of any commit — only the 4 tracked files above are.)

---

## Task 2: `Player` — sidecar spawn via `tauri-plugin-shell`, `AppHandle` threading

**Files:**

- Modify: `src-tauri/src/media/player.rs`
- Modify: `src-tauri/src/error.rs` (new `Sidecar` error variant)
- Modify: `src-tauri/src/commands/mod.rs` (`resolve_and_load`,
  `top_up_queue`, `start_session_and_play` gain an `app` parameter)
- Modify: `src-tauri/src/commands/mood.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/session.rs` (thread `app: tauri::AppHandle`
  through every `#[tauri::command]` that calls into the functions above)
- Modify: `src-tauri/src/lib.rs` (`Player::new` call site, shutdown
  block)
- Modify: `src-tauri/src/media/sidecar_paths.rs` (remove just the now-
  unused `mpv` field — Step 7 below; `yt_dlp`/`deno` and the struct
  itself stay until Task 3 replaces the whole file)
- Modify: `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/crash.rs` (`test_state()` fixtures)

**Interfaces:**

- Consumes: `tauri_plugin_shell::ShellExt` (Task 1's plugin
  registration).
- Produces: `Player::new(socket_path: PathBuf, app_dir: PathBuf,
crash_reporting_enabled: Arc<AtomicBool>) -> Player` (drops the
  `mpv_path: PathBuf` parameter entirely — Task 3 must not still pass
  one). `Player::start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()>`,
  `Player::load<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, stream_url: &str) -> Result<()>`
  — every other `Player` method (`set_paused`, `is_paused`, `set_volume`,
  `volume_percent`, `seek_to`, `position_seconds`, `duration_seconds`,
  `shutdown`) is unchanged in signature (they operate purely over the
  already-open Unix IPC socket, never need `AppHandle`).

- [ ] **Step 1: Add the `Sidecar` error variant**

In `src-tauri/src/error.rs`, add one variant to `EchoraError` (after
`SponsorBlock`):

```rust
    #[error("sidecar error: {0}")]
    Sidecar(String),
```

Add its code to `EchoraError::code()`'s match:

```rust
            EchoraError::Sidecar(_) => "sidecar_error",
```

- [ ] **Step 2: Rewrite `Player`'s struct, constructor, and `start`/`load`**

In `src-tauri/src/media/player.rs`, replace the struct and constructor:

```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::crash;
use crate::error::{EchoraError, Result};

pub struct Player {
    socket_path: PathBuf,
    child: Option<CommandChild>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
}

impl Player {
    pub fn new(
        socket_path: PathBuf,
        app_dir: PathBuf,
        crash_reporting_enabled: Arc<AtomicBool>,
    ) -> Self {
        Player {
            socket_path,
            child: None,
            app_dir,
            crash_reporting_enabled,
        }
    }

    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    pub async fn start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);

        let (_rx, child) = app
            .shell()
            .sidecar("mpv")
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?
            .args([
                "--idle=yes".to_string(),
                "--no-video".to_string(),
                "--no-terminal".to_string(),
                format!("--input-ipc-server={}", self.socket_path.display()),
            ])
            .spawn()
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?;
        self.child = Some(child);

        self.wait_for_socket().await
    }
```

`_rx` (mpv's stdout/stderr event stream) is intentionally dropped —
`Player` never needs mpv's console output, only its IPC socket. The
shell plugin pipes rather than nulls a spawned sidecar's stdio (it has
to, to deliver `CommandEvent`s at all); an un-drained receiver's
internal channel could in principle back-pressure a chatty child if it
writes enough before the receiver is dropped, but mpv run with
`--no-terminal` in idle/IPC-only mode is not a heavy stdout writer —
flagging this as a known, low-probability risk rather than a change
needed here. If real playback testing (Task 6's manual verification)
ever shows mpv hanging on startup, this is the first place to check.

`wait_for_socket`, `send_command` (including the Crash-report reactive
sidecar-crash-detection logic — untouched, it only ever checked
`self.child.is_some()`/`= None`, which works identically regardless of
`child`'s concrete type), and every method other than `start`/`load`/
`shutdown` are **unchanged** — do not rewrite them.

`load` gains the same generic `app` parameter (it doesn't spawn
anything itself, but takes `app` for signature consistency with how
callers use it alongside `start`; if you find `load` genuinely never
needs `app`, leave it as today's plain `&mut self` signature instead —
don't add an unused parameter just for consistency. Verify by attempting
the compile both ways and keeping whichever the compiler says is
actually needed).

- [ ] **Step 3: Rewrite `shutdown`'s synchronous `kill`**

Replace:

```rust
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
```

with:

```rust
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
```

(`tauri_plugin_shell::process::CommandChild::kill(self) -> Result<(), tauri_plugin_shell::Error>`
is synchronous, unlike `tokio::process::Child::kill()` — no `.await`,
and it consumes `self` rather than taking `&mut self`, hence dropping
the `mut` from the `if let Some(mut child)` pattern.)

- [ ] **Step 4: Thread `AppHandle` through the playback call chain**

`commands/mod.rs::resolve_and_load` calls both `player.start()` and
(Task 3 will add) resolver methods that also need `app`. Add the
parameter now so Task 3 doesn't have to re-thread the same chain:

```rust
pub(crate) async fn resolve_and_load(
    app: &tauri::AppHandle,
    state: &AppState,
    track: &Track,
) -> Result<()> {
    let resolved = state.resolver.resolve_with_retry(&track.id).await?; // Task 3 adds `app` here
    let mut player = state.player.lock().await;
    if !player.is_started() {
        player.start(app).await?;
    }
    player.load(app, &resolved.stream_url).await?; // drop `app` here if Step 2 concluded `load` doesn't need it
    drop(player);
    crate::platform::mpris::notify(state).await;
    Ok(())
}
```

Do the same for its callers in the same file:

```rust
pub(crate) async fn top_up_queue(app: &tauri::AppHandle, state: &AppState, moods: &[(String, u8)]) -> Result<()> {
    // ...unchanged body, except:
    let candidates =
        mood_engine::generate_mixed_candidates(app, &resolved, &state.resolver, &ctx, &config, &mut rng)
            .await?; // Task 3 threads `app` into generate_mixed_candidates
    // ...
}

pub(crate) async fn start_session_and_play(
    app: &tauri::AppHandle,
    state: &AppState,
    moods: &[(String, u8)],
) -> Result<SessionInfo> {
    let session = crate::commands::session::start_session_impl(state, moods)?;
    top_up_queue(app, state, moods).await?;

    let current = state.queue.lock().unwrap().current().cloned();
    if let Some(track) = current {
        resolve_and_load(app, state, &track).await?;
    }
    Ok(session)
}
```

- [ ] **Step 5: Thread `app` from every `#[tauri::command]` entry point down to these helpers**

Run `grep -n "start_session_and_play\|resolve_and_load\|top_up_queue" src-tauri/src/commands/*.rs`
to find every call site (there were 9 at the time this plan was written
— `commands/mood.rs`, `commands/queue.rs` (6 sites), `commands/session.rs`
(2 sites); re-run the grep yourself since Task 1/later work may have
shifted line numbers). For each `#[tauri::command]` function that calls
one of these three helpers:

1. If the command function doesn't already take an `app: tauri::AppHandle`
   parameter, add one (Tauri auto-injects it — no frontend/IPC change
   needed, `invoke()` callers don't pass it).
2. Update the call to pass `&app` as the new first argument.

Example (the exact "before" shape may differ slightly per function —
match this pattern, don't guess if a signature looks different from
what's shown here):

```rust
#[tauri::command]
pub async fn start_mood_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mood_id: String,
) -> Result<SessionInfo> {
    start_session_and_play(&app, &state, &[(mood_id, 100)]).await
}
```

This is a mechanical, compiler-verified change — after updating every
site the grep found, `cargo build` either succeeds (you got them all)
or names the next one to fix. Do not stop until it compiles clean.

- [ ] **Step 6: Update `lib.rs`'s `Player::new` call and shutdown block**

Replace:

```rust
            let player = media::player::Player::new(
                sidecar_paths.mpv,
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );
```

with:

```rust
            let player = media::player::Player::new(
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );
```

(Leave `sidecar_paths.yt_dlp`/`sidecar_paths.deno` construction alone —
Task 3 handles those.) `shutdown()`'s signature didn't change, so the
`.run(|app_handle, event| { ... state.player.lock().await.shutdown().await ... })`
block at the bottom of `run()` needs no change.

- [ ] **Step 7: Remove `SidecarPaths`'s now-unused `mpv` field**

After Step 6, nothing in the crate reads `SidecarPaths.mpv` anymore —
left in place, that field would fail `cargo clippy --all-targets -- -D
warnings` with a `dead_code` warning before Task 3 gets a chance to
remove the whole struct. Remove just the one field now instead of
leaving dead code for a later task to clean up. In
`src-tauri/src/media/sidecar_paths.rs`, remove `pub mpv: PathBuf,` from
the `SidecarPaths` struct and `mpv: PathBuf::from("mpv"),` from
`discover_dev()` — leave `yt_dlp`/`deno` untouched (Task 3 removes
those, and the struct itself, when it replaces this file).

- [ ] **Step 8: Add a `mock_builder()`-based `AppHandle` test helper, update the 3 fixtures**

`tauri::test::mock_builder()` produces `AppHandle<tauri::test::MockRuntime>`
— a genuinely different concrete type than production's
`AppHandle<tauri::Wry>`, which is exactly why `Player::start`/`load`
were made generic over `R: tauri::Runtime` in Step 2 rather than fixed
to a concrete handle type. Add this helper to each of the 3 fixture
files that construct a `Player` (`src-tauri/src/commands/session.rs`,
`src-tauri/src/commands/queue.rs`, `src-tauri/src/commands/crash.rs`,
inside each file's existing `#[cfg(test)] mod tests`):

```rust
fn test_app_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .plugin(tauri_plugin_shell::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app should build")
        .handle()
        .clone()
}
```

None of these 3 fixtures' existing tests actually call `player.start()`/
`.load()` (they exercise queue/session/db/crash logic, not real
playback) — this helper only needs to exist so `Player::new`'s
constructor call in each `test_state()` compiles; you are not expected
to make these fixtures spawn a real sidecar. If `Player::new` no longer
needs any handle at construction time (per Step 2's design, it
doesn't — only `start`/`load` do), this helper may turn out to be
unused in `test_state()` itself; only add it where something in that
file's tests actually needs to call `.start()`/`.load()`. Check each
file before assuming it's needed everywhere.

Update each `test_state()`'s `Player::new(...)` call to match the new
3-argument constructor from Step 2 (drop whatever `PathBuf::from("mpv")`
first argument each fixture currently passes).

- [ ] **Step 9: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings &&
cargo test` (from `src-tauri/`). Expect all non-ignored tests to pass,
zero warnings (Step 7 already removed the one `dead_code` risk this
task would otherwise have introduced). The `#[ignore]` real-mpv smoke
tests in `player.rs` will now fail if run (`cargo test -- --ignored`)
— per this plan's Global Constraints, this is the accepted, ruled-on
trade-off from the upstream Tauri bug, not something to fix. Do not
attempt to make them pass; do not delete them either (they document
intent) — just don't claim they pass.

- [ ] **Step 10: Commit**

Never run `git commit` directly — stage every file this task touched
(`src-tauri/src/media/player.rs`, `src-tauri/src/media/sidecar_paths.rs`,
`src-tauri/src/error.rs`, `src-tauri/src/commands/mod.rs`,
`src-tauri/src/commands/mood.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/session.rs`, `src-tauri/src/commands/crash.rs`,
`src-tauri/src/lib.rs`), then invoke the `auto-commit` skill. Suggested
message: `feat(packaging): spawn mpv via tauri-plugin-shell`.

---

## Task 3: `Resolver` — sidecar spawn, event-accumulation, Deno path resolution

**Files:**

- Modify: `src-tauri/src/media/resolver.rs`
- Modify: `src-tauri/src/media/sidecar_paths.rs` (narrow to Deno-only;
  or delete and fold its one function into `resolver.rs` — pick
  whichever keeps `resolver.rs` from growing unreasonably large, per
  the plan's file-structure guidance; either is acceptable)
- Modify: `src-tauri/src/lib.rs` (`Resolver::new`/`ResolverConfig`
  construction)
- Modify: `src-tauri/src/mood_engine/mod.rs` (`generate_mixed_candidates`
  gains an `app` parameter)
- Modify: `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/crash.rs` (`test_state()` fixtures —
  `Resolver::new`/`ResolverConfig` call sites)

**Interfaces:**

- Consumes: Task 2's `app: &tauri::AppHandle<R>` already threaded
  through `resolve_and_load`/`top_up_queue`/`start_session_and_play`;
  `tauri_plugin_shell::ShellExt` (Task 1).
- Produces: `Resolver::new(config: ResolverConfig) -> Resolver` (keeps
  the same shape — `ResolverConfig` still carries `deno_path: PathBuf`
  and `timeout: Duration`, but drops `yt_dlp_path: PathBuf` since
  yt-dlp is now spawned by sidecar name, not a resolved path).
  `Resolver::search<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, query: &str, limit: u32) -> Result<Vec<Track>>`,
  `Resolver::resolve_with_retry<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream>`
  — both gain the generic `app` parameter; `resolve` and `run` (private)
  do too, since they're on the call path.

- [ ] **Step 1: Resolve Deno's path — narrow `SidecarPaths` or fold it into `resolver.rs`**

This implements the spec's flagged "open risk" — the best-available
approach given no documented Tauri API resolves an `externalBin` path
directly. **This must be verified against a real built `.deb`/AppImage
before being trusted** (see Task 6's verification step) — do not treat
this code as correct just because it compiles.

Replace `src-tauri/src/media/sidecar_paths.rs` entirely with:

```rust
use std::path::PathBuf;

/// Deno's resolved path for yt-dlp's `--js-runtimes` argument — yt-dlp
/// spawns Deno itself as its own child process, so Echora needs Deno's
/// real filesystem path (not just the ability to spawn it), unlike mpv
/// and yt-dlp which Echora spawns directly by sidecar name.
///
/// No documented Tauri 2.x API resolves an `externalBin` entry to a
/// bare path (only `app.shell().sidecar(name)`, which spawns rather
/// than resolves) — this reasons from each package format's own
/// layout convention instead. **Unverified against a real built
/// package as of this comment** — confirm by building a real `.deb`
/// and AppImage and checking where `deno-<triple>` actually lands
/// before trusting this in production (see
/// docs/superpowers/plans/2026-09-01-packaging.md, Task 6).
pub fn resolve_deno_path() -> PathBuf {
    let triple = format!("{}-unknown-linux-gnu", std::env::consts::ARCH);
    let filename = format!("deno-{triple}");

    if let Ok(appdir) = std::env::var("APPDIR") {
        // Running from a mounted AppImage — Deno sits at the same
        // level Tauri places every externalBin/resource inside the
        // AppImage's own root.
        return PathBuf::from(appdir).join("usr/bin").join(&filename);
    }

    if let Ok(exe) = std::env::current_exe()
        && exe.starts_with("/usr/")
    {
        // A .deb install places externalBin binaries alongside the
        // main executable, both under /usr/bin/.
        if let Some(dir) = exe.parent() {
            return dir.join(&filename);
        }
    }

    // Dev mode (cargo run / cargo tauri dev, not a bundled build).
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(&filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_mode_resolves_relative_to_the_manifest_dir() {
        // SAFETY: this test reads but does not mutate APPDIR; no other
        // test in this crate touches it.
        unsafe {
            std::env::remove_var("APPDIR");
        }
        let path = resolve_deno_path();
        assert!(path.starts_with(env!("CARGO_MANIFEST_DIR")));
        assert!(path.to_string_lossy().contains("deno-"));
    }
}
```

- [ ] **Step 2: Rewrite `Resolver`'s `run()` to spawn via the shell plugin and accumulate events**

In `src-tauri/src/media/resolver.rs`, replace the imports and
`ResolverConfig`:

```rust
use std::path::PathBuf;
use std::time::Duration;

use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;
use tokio::time::timeout;

use super::metadata;
use crate::error::{EchoraError, Result};
use crate::models::{ResolvedStream, Track};

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub deno_path: PathBuf,
    pub timeout: Duration,
}
```

Replace `run()`:

```rust
    async fn run<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, args: Vec<String>) -> Result<String> {
        let (mut rx, _child) = app
            .shell()
            .sidecar("yt-dlp")
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?
            .args(&args)
            .spawn()
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?;

        let collect = async {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_success = false;
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => stdout.extend_from_slice(&bytes),
                    CommandEvent::Stderr(bytes) => stderr.extend_from_slice(&bytes),
                    CommandEvent::Terminated(payload) => {
                        exit_success = payload.code == Some(0);
                        break;
                    }
                    CommandEvent::Error(_) => break,
                    _ => {}
                }
            }
            (stdout, stderr, exit_success)
        };

        let (stdout, stderr, exit_success) = timeout(self.config.timeout, collect)
            .await
            .map_err(|_| EchoraError::SidecarTimeout("yt-dlp".into()))?;

        if exit_success {
            Ok(String::from_utf8_lossy(&stdout).into_owned())
        } else {
            Err(metadata::classify_ytdlp_failure(&String::from_utf8_lossy(
                &stderr,
            )))
        }
    }
```

Note: `CommandEvent`'s `Stdout`/`Stderr` variants deliver
newline-delimited chunks already (per the shell plugin's own buffering,
confirmed against its docs), but this code doesn't depend on that —
it just concatenates raw bytes across every event until `Terminated`,
matching the old `wait_with_output()`'s "give me everything" semantics
exactly. `CommandEvent` may have other variants beyond the 4 handled
here in the installed `tauri-plugin-shell` version — the `_ => {}` arm
handles any that don't need action; if `cargo build` reports a
non-exhaustive match despite the wildcard arm, or if `Terminated`'s
payload field isn't named `code`, adjust to match the actual generated
type (check with `cargo doc --open -p tauri-plugin-shell` or
`cargo expand` if unsure — don't guess a field name that fails to
compile).

- [ ] **Step 3: Thread `app` through `search`/`resolve`/`resolve_with_retry`**

```rust
    pub async fn search<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, query: &str, limit: u32) -> Result<Vec<Track>> {
        let search_spec = format!("ytsearch{limit}:{query}");
        let stdout = self
            .run(
                app,
                vec![
                    "--js-runtimes".into(),
                    format!("deno:{}", self.config.deno_path.display()),
                    search_spec,
                    "--flat-playlist".into(),
                    "--dump-json".into(),
                    "--no-warnings".into(),
                ],
            )
            .await?;

        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(metadata::parse_search_result)
            .collect()
    }

    pub async fn resolve_with_retry<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream> {
        match self.resolve(app, track_id).await {
            Err(EchoraError::SidecarTimeout(_) | EchoraError::Io(_)) => {
                self.resolve(app, track_id).await
            }
            other => other,
        }
    }

    async fn resolve<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream> {
        let url = format!("https://www.youtube.com/watch?v={track_id}");
        let stdout = self
            .run(
                app,
                vec![
                    "--js-runtimes".into(),
                    format!("deno:{}", self.config.deno_path.display()),
                    url,
                    "-f".into(),
                    "bestaudio".into(),
                    "--dump-json".into(),
                    "--no-warnings".into(),
                    "--no-playlist".into(),
                ],
            )
            .await?;
        metadata::parse_resolved(&stdout, track_id)
    }
```

- [ ] **Step 4: Thread `app` into `mood_engine::generate_mixed_candidates`**

In `src-tauri/src/mood_engine/mod.rs`, add `app: &tauri::AppHandle<R>`
(generic over `R: tauri::Runtime`) as `generate_mixed_candidates`'s
first parameter, and pass it into whatever call it makes to
`resolver.search(...)` around line 68 (`match resolver.search(&query,
config.results_per_query).await` becomes `match resolver.search(app,
&query, config.results_per_query).await`). Task 2, Step 4 already
updated `top_up_queue` (this function's caller) to accept and forward
`app` — confirm that wiring is in place; if `top_up_queue` didn't end
up calling `generate_mixed_candidates` the way Task 2's sketch assumed
(the actual code may differ slightly), thread `app` through whatever
the real call chain is, verified by `cargo build`.

- [ ] **Step 5: Update `lib.rs`'s `Resolver::new`/`ResolverConfig` construction**

Replace:

```rust
            let sidecar_paths = media::sidecar_paths::SidecarPaths::discover_dev();
            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                yt_dlp_path: sidecar_paths.yt_dlp,
                deno_path: sidecar_paths.deno,
                timeout: std::time::Duration::from_secs(30),
            });
```

with:

```rust
            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                deno_path: media::sidecar_paths::resolve_deno_path(),
                timeout: std::time::Duration::from_secs(30),
            });
```

(This also removes the last use of `sidecar_paths.mpv`, so the
`SidecarPaths` struct/`discover_dev()` function from Task 2 is now
fully gone — Step 1 already replaced the whole file.)

- [ ] **Step 6: Update the 3 fixtures' `Resolver::new`/`ResolverConfig` calls**

In `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/crash.rs`'s `test_state()`, remove the
`yt_dlp_path` field from each `ResolverConfig { ... }` literal (keep
`deno_path`/`timeout` — `PathBuf::from("deno")` as a dummy value is
fine, these tests never actually resolve anything real).

- [ ] **Step 7: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings &&
cargo test` (from `src-tauri/`). Expect all non-ignored tests to pass.
Same accepted gap as Task 2 applies to `resolver.rs`'s `#[ignore]`
real-network smoke tests — don't attempt to make `cargo test --
--ignored` pass for them.

- [ ] **Step 8: Commit**

Never run `git commit` directly — stage every file this task touched
(`src-tauri/src/media/resolver.rs`, `src-tauri/src/media/sidecar_paths.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/mood_engine/mod.rs`,
`src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/crash.rs`), then invoke the `auto-commit` skill.
Suggested message: `feat(packaging): spawn yt-dlp via tauri-plugin-shell, resolve deno path`.

---

## Task 4: mpv build script

**Files:**

- Create: `scripts/build-mpv.sh`

**Interfaces:**

- Produces: a script that, given `$TARGET_TRIPLE` and an output
  directory, builds mpv `v0.41.0` from source (audio-only Meson
  config), makes it relocatable via `patchelf`, and places
  `mpv-$TARGET_TRIPLE` plus its `lib/` sibling directory at the given
  output path. Task 6 (release workflow) invokes this script per
  matrix job.

- [ ] **Step 1: Write the build script**

Create `scripts/build-mpv.sh`:

```bash
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
  -Dwin32-desktop=disabled \
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
```

```bash
chmod +x scripts/build-mpv.sh
```

The `ldd | grep -E 'libav|libsw|libpostproc'` filter bundles FFmpeg's
own libraries (not guaranteed present system-wide at the exact ABI
version mpv was built against) while deliberately leaving out
`libasound`/`libpulse` (the ALSA/PulseAudio _client_ libraries — those
stay as ordinary system libraries via `.deb`'s `depends`, per the spec;
bundling them too would risk ABI conflicts with the system's own sound
server client at runtime).

- [ ] **Step 2: Attempt to actually run it in this environment**

Check whether this environment has the build dependencies mpv needs:
`which meson ninja gcc pkg-config patchelf`, and whether FFmpeg dev
headers are importable via `pkg-config --exists libavcodec libavformat
libavutil libswresample && echo yes`. If all present, actually run:

```bash
./scripts/build-mpv.sh x86_64-unknown-linux-gnu /tmp/mpv-build-test
```

and confirm the resulting binary runs (`--version` succeeds) and is
genuinely relocatable — copy the whole `/tmp/mpv-build-test` directory
somewhere else and confirm `mpv-x86_64-unknown-linux-gnu --version`
still works from the new location (proves the rpath fix actually
works, not just that it ran once from its original build directory).

If any build dependency is missing in this environment, **say so
explicitly in your report** rather than claiming the script works —
this is exactly the kind of runtime-sensitive thing this project
requires real verification for, not just "the script looks right."
Note precisely which command was unavailable and that the script's
correctness for a real CI run (Task 6) remains unverified locally.

- [ ] **Step 3: Commit**

Never run `git commit` directly — stage `scripts/build-mpv.sh`, then
invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add mpv from-source build script`.

---

## Task 5: yt-dlp/Deno production fetch + checksum verification script

**Files:**

- Create: `scripts/fetch-sidecar-binaries.sh`

**Interfaces:**

- Produces: a script that, given `$TARGET_TRIPLE` and an output
  directory, downloads yt-dlp's and Deno's official release binaries
  for the matching architecture, verifies them against upstream's
  published checksums, and places `yt-dlp-$TARGET_TRIPLE` and
  `deno-$TARGET_TRIPLE` at the given output path. Task 6 invokes this
  per matrix job.

- [ ] **Step 1: Write the script**

Create `scripts/fetch-sidecar-binaries.sh`:

```bash
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
curl -fL -o "/tmp/SHA256SUMS" \
  "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA256SUMS"
EXPECTED_SHA="$(grep " $YT_DLP_ASSET\$" /tmp/SHA256SUMS | awk '{print $1}')"
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
# Deno publishes per-asset .sha256sum files alongside each zip.
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
```

```bash
chmod +x scripts/fetch-sidecar-binaries.sh
```

Deno's exact per-asset checksum file naming/existence
(`$DENO_ZIP.sha256sum`) was reported by research, not independently
confirmed against a real Deno release page — verify this file actually
exists at that URL in Step 2 below before trusting it in CI; if it
doesn't, fall back to whatever checksum artifact Deno's release page
actually publishes (a single `SHASUMS256.txt`-style file covering all
assets is a common alternative pattern) and adjust the script.

- [ ] **Step 2: Actually run it**

This environment already has real yt-dlp/Deno dev binaries at
`src-tauri/binaries/{yt-dlp,deno}-x86_64-unknown-linux-gnu` from Task
1 — this script produces the same files a different way (fresh
download + checksum, not whatever was there before), so running it for
real is a direct, meaningful verification, not just a syntax check:

```bash
./scripts/fetch-sidecar-binaries.sh x86_64 x86_64-unknown-linux-gnu /tmp/sidecar-fetch-test
```

Confirm it downloads, verifies, and produces both files without error.
If the Deno checksum step fails because the assumed URL/file doesn't
exist (see Step 1's caveat), fix the script based on what Deno's real
release page actually publishes — don't leave a known-broken checksum
step in place.

- [ ] **Step 3: Commit**

Never run `git commit` directly — stage `scripts/fetch-sidecar-binaries.sh`,
then invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add yt-dlp/deno fetch and checksum script`.

---

## Task 6: `.github/workflows/release.yml`

**Files:**

- Create: `.github/workflows/release.yml`

**Interfaces:**

- Consumes: `scripts/build-mpv.sh` (Task 4), `scripts/fetch-sidecar-binaries.sh`
  (Task 5), the `externalBin`/`createUpdaterArtifacts` config (Task 1),
  the rewritten `Player`/`Resolver` (Tasks 2-3).
- Produces: a tag-triggered CI workflow publishing signed `.deb` +
  AppImage + `latest.json` to GitHub Releases for both architectures.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags: ['v*.*.*']

env:
  CARGO_TERM_COLOR: always

jobs:
  release:
    name: Release (${{ matrix.arch }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x86_64
            triple: x86_64-unknown-linux-gnu
            runner: ubuntu-24.04
          - arch: arm64
            triple: aarch64-unknown-linux-gnu
            runner: ubuntu-24.04-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      - name: Verify tag matches tauri.conf.json version
        run: |
          TAG_VERSION="${GITHUB_REF_NAME#v}"
          CONF_VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
          if [ "$TAG_VERSION" != "$CONF_VERSION" ]; then
            echo "Tag $GITHUB_REF_NAME does not match tauri.conf.json version $CONF_VERSION" >&2
            exit 1
          fi

      - name: Install Tauri Linux prerequisites
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
            libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

      - name: Install mpv build prerequisites
        run: |
          sudo apt-get install -y meson ninja-build patchelf pkg-config \
            libavcodec-dev libavformat-dev libavutil-dev libswresample-dev \
            libasound2-dev libpulse-dev

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - run: npm ci

      - name: Build mpv
        run: ./scripts/build-mpv.sh ${{ matrix.triple }} src-tauri/binaries

      - name: Fetch yt-dlp/Deno
        run: ./scripts/fetch-sidecar-binaries.sh ${{ matrix.arch }} ${{ matrix.triple }} src-tauri/binaries

      - uses: tauri-apps/tauri-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'Echora ${{ github.ref_name }}'
          releaseDraft: false
          prerelease: false
          includeUpdaterJson: true
```

- [ ] **Step 2: Validate the YAML**

Run whatever YAML/GitHub-Actions linting is available in this
environment — `actionlint .github/workflows/release.yml` if installed,
otherwise at minimum a plain YAML syntax check
(`python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/release.yml'))"`
or equivalent). Report exactly which check you ran and its result —
this workflow cannot be executed end-to-end in this environment (no
real GitHub Actions runner, no tag push), so syntax validation is the
only automated verification available here.

- [ ] **Step 3: Do NOT push a tag or trigger a real release run**

Pushing a `vN.N.N` tag triggers a real CI run against the shared
GitHub repository (consumes Actions minutes, may create a real GitHub
Release if it succeeds) — that is a side effect outside this worktree
requiring the user's own explicit action, not something to do
autonomously. Stop here. In your final report, hand the user the exact
recommended first real verification (matching the spec's own Testing
section): push a scratch tag like `v0.1.1-test` from a disposable
branch, watch the workflow run, download the resulting `.deb`/AppImage,
and confirm the package installs, mpv plays real audio, yt-dlp resolves
a real track (confirming Deno's path resolution — the open risk from
Task 3 — actually works in a real built package), and Auto-update's
"Check for Updates" finds the published `latest.json`.

- [ ] **Step 4: Commit**

Never run `git commit` directly — stage `.github/workflows/release.yml`,
then invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add tag-triggered release workflow`.

# Packaging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship real `.deb`/AppImage packages for x86_64 + ARM64 with mpv
built from source, yt-dlp/Deno bundled as production sidecars, a real
updater signing keypair (public half already committed), and a CI
release pipeline triggered by a `vN.N.N` git tag.

**Architecture:** mpv/yt-dlp/Deno become Tauri `externalBin` sidecars,
spawned via `tauri-plugin-shell` (the only Tauri-2-supported way to
invoke a bundled `externalBin`, confirmed — no path-only resolution API
exists). `Player`/`Resolver` stay concrete, non-generic structs; only
the methods that actually spawn a process become generic over
`R: tauri::Runtime`, taking `&tauri::AppHandle<R>` as a parameter —
this keeps `AppState` and every command handler untouched, and lets
tests pass a `tauri::test::mock_builder()`-produced `AppHandle<MockRuntime>`
without genericizing the whole app. mpv is built from source per
architecture in CI (per `docs/adr/0007`); yt-dlp/Deno are downloaded
from their official releases and checksum-verified. `tauri-apps/tauri-action`
runs the actual build+sign+publish in a 2-job architecture matrix.

**Tech Stack:** Rust, `tauri-plugin-shell` 2.x, GitHub Actions
(`tauri-apps/tauri-action` v1, native `ubuntu-24.04`/`ubuntu-24.04-arm`
runners), Meson (mpv build), `patchelf`.

**Spec:** `docs/superpowers/specs/2026-09-01-packaging-design.md`

## Global Constraints

- No new dependency beyond `tauri-plugin-shell` (Rust, official Tauri
  plugin, MIT/Apache-2.0 — no new licensing review needed).
- The updater signing public key is already committed
  (`src-tauri/tauri.conf.json`'s `plugins.updater.pubkey`, commit
  `e681aed`) — do not touch it. The private key/password are the user's
  own GitHub Actions secrets, entirely outside this plan's scope.
- No automated version-bump tooling — CI only _verifies_ the pushed
  tag matches `tauri.conf.json`'s `version`, never edits/commits.
- `libasound2`/`libpulse0` are declared `.deb` dependencies (standard
  system libraries, not a "zero install" violation) — never bundled.
- **Known, accepted testing gap**: after this plan, the existing
  `#[ignore]` real-process smoke tests in `player.rs`/`resolver.rs`
  that spawn mpv/yt-dlp for real can no longer run under `cargo test`
  — a confirmed, unresolved upstream Tauri bug
  (`tauri-apps/tauri#13767`) makes sidecar spawning fail inside the
  `cargo test` harness (it looks for the sidecar binary in
  `target/debug/deps/` instead of `target/debug/`), even with
  `tauri::test::mock_builder()`. This is a deliberate, ruled-on
  trade-off (see spec) — do not attempt to work around the upstream
  bug in this plan. Real verification of actual sidecar spawning moves
  to manual `npm run tauri dev` testing (the user's own terminal, never
  launched by an agent).
- Before claiming any task done: `cargo fmt --check && cargo clippy
--all-targets -- -D warnings && cargo test` (backend tasks).
- Never run `git commit` directly — every task's commit step goes
  through the `auto-commit` skill instead (project rule, `~/.claude/CLAUDE.md`).

---

## Task 1: Dependencies, config, and dev binary layout

**Files:**

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/lib.rs` (plugin registration only — not the
  `SidecarPaths`/`Player`/`Resolver` construction, that's Tasks 2-3)
- Create/move: dev sidecar binaries under `src-tauri/binaries/`

**Interfaces:**

- Produces: `tauri_plugin_shell::init()` registered as a Tauri plugin;
  `externalBin` entries `binaries/mpv`, `binaries/yt-dlp`,
  `binaries/deno` declared in `tauri.conf.json`; real sidecar binaries
  present on disk at `src-tauri/binaries/{mpv,yt-dlp,deno}-x86_64-unknown-linux-gnu`
  for local dev/testing. Tasks 2 and 3 depend on all of this existing
  before their own `cargo build`/test runs can succeed.

- [ ] **Step 1: Add `tauri-plugin-shell` and the `tauri` test feature**

In `src-tauri/Cargo.toml`, add to `[dependencies]` (keep the existing
`tauri` line with `tray-icon` unchanged):

```toml
tauri-plugin-shell = "2"
```

Add a new `[dev-dependencies]` section (Cargo unifies feature sets for
the same crate across dependency kinds within one build, so this
activates `tauri`'s `test` feature only when running `cargo test`, never
in a release build):

```toml
[dev-dependencies]
tauri = { version = "2", features = ["test"] }
```

- [ ] **Step 2: Register the shell plugin**

In `src-tauri/src/lib.rs`, add to the plugin chain (order among plugins
doesn't matter — add it next to the others):

```rust
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
```

- [ ] **Step 3: Add `externalBin`, `createUpdaterArtifacts`, and `.deb` deps to `tauri.conf.json`**

In `src-tauri/tauri.conf.json`'s `bundle` section, replace:

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
```

with:

```json
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
    "icon": [
```

(The rest of the `icon` array and the file's closing braces are
unchanged.)

- [ ] **Step 4: Scope the shell-spawn capability to exactly the 2 spawned sidecars**

In `src-tauri/capabilities/default.json`, add one entry to the
`permissions` array (after `"process:allow-restart"`):

```json
    "process:allow-restart",
    {
      "identifier": "shell:allow-spawn",
      "allow": [
        { "name": "binaries/mpv", "sidecar": true },
        { "name": "binaries/yt-dlp", "sidecar": true }
      ]
    }
```

Deno is never spawned by Echora's own Rust code (yt-dlp spawns it as
its own child process) — it needs no `shell:allow-spawn` entry, only
its `externalBin` bundling entry from Step 3.

- [ ] **Step 5: Migrate dev sidecar binaries to the target-triple naming convention**

Tauri's `externalBin`/sidecar resolution (used in dev mode too, not
just bundled builds) expects binaries at
`src-tauri/binaries/<name>-<target-triple>`, not the current ad hoc
`src-tauri/binaries/dev/` layout. This machine is x86_64
(`x86_64-unknown-linux-gnu` triple) — confirm with `uname -m` before
running these commands, and adjust the triple suffix if this environment
is actually aarch64:

```bash
mkdir -p src-tauri/binaries
mv src-tauri/binaries/dev/yt-dlp_linux src-tauri/binaries/yt-dlp-x86_64-unknown-linux-gnu
mv src-tauri/binaries/dev/deno src-tauri/binaries/deno-x86_64-unknown-linux-gnu
ln -s "$(command -v mpv)" src-tauri/binaries/mpv-x86_64-unknown-linux-gnu
rmdir src-tauri/binaries/dev
chmod +x src-tauri/binaries/yt-dlp-x86_64-unknown-linux-gnu src-tauri/binaries/deno-x86_64-unknown-linux-gnu
```

(`src-tauri/binaries/*` is already fully gitignored except a
`.gitkeep` — confirm with `git status --porcelain` that this move
doesn't show up as anything to commit.) If this environment has no
system `mpv` on `PATH`, note that explicitly instead of creating a
broken symlink — Task 2/3's real-process manual verification will need
a real `mpv` present.

- [ ] **Step 6: Verify**

Run: `cargo build --manifest-path src-tauri/Cargo.toml 2>&1 | tail -40`
— this is the first real signal for a genuinely open question: does
Tauri's build-time tooling (via `tauri-build`'s `build.rs`, which runs
on every `cargo build`, not just `cargo tauri build`) validate that
`externalBin`-declared files actually exist on disk? If the build fails
citing a missing sidecar file, that _proves_ Step 5's binary placement
is mandatory for the crate to even compile — record that finding in your
report either way (pass or fail), don't just report the end state.

Then run the full gate: `cargo fmt --check && cargo clippy --all-targets
-- -D warnings && cargo test` (from `src-tauri/`). Expect these to pass
unchanged from before this task — nothing in `player.rs`/`resolver.rs`
has been touched yet, so the existing (pre-Packaging) test count and
behavior should be identical; this task only adds unused-so-far plugin
registration and config.

- [ ] **Step 7: Commit**

Never run `git commit` directly — stage `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`,
`src-tauri/src/lib.rs`, then invoke the `auto-commit` skill. Suggested
message: `feat(packaging): register shell plugin, externalBin, dev binary layout`.
(The dev binary moves under `src-tauri/binaries/` are gitignored and
won't be part of any commit — only the 4 tracked files above are.)

---

## Task 2: `Player` — sidecar spawn via `tauri-plugin-shell`, `AppHandle` threading

**Files:**

- Modify: `src-tauri/src/media/player.rs`
- Modify: `src-tauri/src/error.rs` (new `Sidecar` error variant)
- Modify: `src-tauri/src/commands/mod.rs` (`resolve_and_load` and
  `start_session_and_play` gain an `app` parameter — **not**
  `top_up_queue`, that's Task 3's, see Step 4 below)
- Modify: `src-tauri/src/commands/mood.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/session.rs` (thread `app: tauri::AppHandle`
  through every `#[tauri::command]` that calls `resolve_and_load` or
  `start_session_and_play` — not the one that calls `top_up_queue`
  directly)
- Modify: `src-tauri/src/lib.rs` (`Player::new` call site, shutdown
  block)
- Modify: `src-tauri/src/media/sidecar_paths.rs` (remove just the now-
  unused `mpv` field — Step 7 below; `yt_dlp`/`deno` and the struct
  itself stay until Task 3 replaces the whole file)
- Modify: `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/crash.rs` (`test_state()` fixtures)

**Interfaces:**

- Consumes: `tauri_plugin_shell::ShellExt` (Task 1's plugin
  registration).
- Produces: `Player::new(socket_path: PathBuf, app_dir: PathBuf,
crash_reporting_enabled: Arc<AtomicBool>) -> Player` (drops the
  `mpv_path: PathBuf` parameter entirely — Task 3 must not still pass
  one). `Player::start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()>`,
  `Player::load<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>, stream_url: &str) -> Result<()>`
  — every other `Player` method (`set_paused`, `is_paused`, `set_volume`,
  `volume_percent`, `seek_to`, `position_seconds`, `duration_seconds`,
  `shutdown`) is unchanged in signature (they operate purely over the
  already-open Unix IPC socket, never need `AppHandle`).

- [ ] **Step 1: Add the `Sidecar` error variant**

In `src-tauri/src/error.rs`, add one variant to `EchoraError` (after
`SponsorBlock`):

```rust
    #[error("sidecar error: {0}")]
    Sidecar(String),
```

Add its code to `EchoraError::code()`'s match:

```rust
            EchoraError::Sidecar(_) => "sidecar_error",
```

- [ ] **Step 2: Rewrite `Player`'s struct, constructor, and `start`/`load`**

In `src-tauri/src/media/player.rs`, replace the struct and constructor:

```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::crash;
use crate::error::{EchoraError, Result};

pub struct Player {
    socket_path: PathBuf,
    child: Option<CommandChild>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
}

impl Player {
    pub fn new(
        socket_path: PathBuf,
        app_dir: PathBuf,
        crash_reporting_enabled: Arc<AtomicBool>,
    ) -> Self {
        Player {
            socket_path,
            child: None,
            app_dir,
            crash_reporting_enabled,
        }
    }

    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    pub async fn start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);

        let (_rx, child) = app
            .shell()
            .sidecar("mpv")
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?
            .args([
                "--idle=yes".to_string(),
                "--no-video".to_string(),
                "--no-terminal".to_string(),
                format!("--input-ipc-server={}", self.socket_path.display()),
            ])
            .spawn()
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?;
        self.child = Some(child);

        self.wait_for_socket().await
    }
```

`_rx` (mpv's stdout/stderr event stream) is intentionally dropped —
`Player` never needs mpv's console output, only its IPC socket. The
shell plugin pipes rather than nulls a spawned sidecar's stdio (it has
to, to deliver `CommandEvent`s at all); an un-drained receiver's
internal channel could in principle back-pressure a chatty child if it
writes enough before the receiver is dropped, but mpv run with
`--no-terminal` in idle/IPC-only mode is not a heavy stdout writer —
flagging this as a known, low-probability risk rather than a change
needed here. If real playback testing (Task 6's manual verification)
ever shows mpv hanging on startup, this is the first place to check.

`wait_for_socket`, `send_command` (including the Crash-report reactive
sidecar-crash-detection logic — untouched, it only ever checked
`self.child.is_some()`/`= None`, which works identically regardless of
`child`'s concrete type), and every method other than `start`/`load`/
`shutdown` are **unchanged** — do not rewrite them.

`load` gains the same generic `app` parameter (it doesn't spawn
anything itself, but takes `app` for signature consistency with how
callers use it alongside `start`; if you find `load` genuinely never
needs `app`, leave it as today's plain `&mut self` signature instead —
don't add an unused parameter just for consistency. Verify by attempting
the compile both ways and keeping whichever the compiler says is
actually needed).

- [ ] **Step 3: Rewrite `shutdown`'s synchronous `kill`**

Replace:

```rust
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
```

with:

```rust
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
```

(`tauri_plugin_shell::process::CommandChild::kill(self) -> Result<(), tauri_plugin_shell::Error>`
is synchronous, unlike `tokio::process::Child::kill()` — no `.await`,
and it consumes `self` rather than taking `&mut self`, hence dropping
the `mut` from the `if let Some(mut child)` pattern.)

- [ ] **Step 4: Thread `AppHandle` through the playback call chain**

`commands/mod.rs::resolve_and_load` calls both `player.start()` and
(Task 3 will add) resolver methods that also need `app`. Add the
parameter now so Task 3 doesn't have to re-thread the same chain:

```rust
pub(crate) async fn resolve_and_load(
    app: &tauri::AppHandle,
    state: &AppState,
    track: &Track,
) -> Result<()> {
    let resolved = state.resolver.resolve_with_retry(&track.id).await?; // Task 3 adds `app` here
    let mut player = state.player.lock().await;
    if !player.is_started() {
        player.start(app).await?;
    }
    player.load(app, &resolved.stream_url).await?; // drop `app` here if Step 2 concluded `load` doesn't need it
    drop(player);
    crate::platform::mpris::notify(state).await;
    Ok(())
}
```

Do the same for `resolve_and_load`'s caller in the same file — but
**leave `top_up_queue` completely untouched in this task**. It's a
Player-unrelated function (it only calls `mood_engine::generate_mixed_candidates`,
never touches `Player`) — it will need `app` too, but only because Task
3's `Resolver` rewrite needs it internally, not because of anything in
this task. If Task 2 added `app` to `top_up_queue`'s signature now, the
call to `generate_mixed_candidates` inside it would need to pass `app`
too — but that function's signature isn't updated to accept it until
Task 3, so the crate would fail to compile until Task 3 also lands,
breaking this task's own "must compile and pass `cargo test`"
verification step. Task 3 owns `top_up_queue` entirely.

```rust
pub(crate) async fn start_session_and_play(
    app: &tauri::AppHandle,
    state: &AppState,
    moods: &[(String, u8)],
) -> Result<SessionInfo> {
    let session = crate::commands::session::start_session_impl(state, moods)?;
    top_up_queue(state, moods).await?; // unchanged call — Task 3 adds `app` here

    let current = state.queue.lock().unwrap().current().cloned();
    if let Some(track) = current {
        resolve_and_load(app, state, &track).await?;
    }
    Ok(session)
}
```

- [ ] **Step 5: Thread `app` from every `#[tauri::command]` entry point down to these helpers**

Run `grep -n "start_session_and_play\|resolve_and_load" src-tauri/src/commands/*.rs`
to find every call site (there were 8 at the time this plan was written
— `commands/mood.rs` (1 site), `commands/queue.rs` (5 sites),
`commands/session.rs` (2 sites); re-run the grep yourself since Task
1/later work may have shifted line numbers). **Do not include
`top_up_queue`** in this grep or this task — it has its own direct
caller too (`commands/queue.rs`'s `ensure_queue_topped_up` command,
which calls `super::top_up_queue(&state, &moods)` directly, never
through `start_session_and_play`), and per Step 4 above, `top_up_queue`
and that command are entirely Task 3's responsibility. For each
`#[tauri::command]` function that calls `start_session_and_play` or
`resolve_and_load`:

1. If the command function doesn't already take an `app: tauri::AppHandle`
   parameter, add one (Tauri auto-injects it — no frontend/IPC change
   needed, `invoke()` callers don't pass it).
2. Update the call to pass `&app` as the new first argument.

Example (the exact "before" shape may differ slightly per function —
match this pattern, don't guess if a signature looks different from
what's shown here):

```rust
#[tauri::command]
pub async fn start_mood_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    mood_id: String,
) -> Result<SessionInfo> {
    start_session_and_play(&app, &state, &[(mood_id, 100)]).await
}
```

This is a mechanical, compiler-verified change — after updating every
site the grep found, `cargo build` either succeeds (you got them all)
or names the next one to fix. Do not stop until it compiles clean.

- [ ] **Step 6: Update `lib.rs`'s `Player::new` call and shutdown block**

Replace:

```rust
            let player = media::player::Player::new(
                sidecar_paths.mpv,
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );
```

with:

```rust
            let player = media::player::Player::new(
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );
```

(Leave `sidecar_paths.yt_dlp`/`sidecar_paths.deno` construction alone —
Task 3 handles those.) `shutdown()`'s signature didn't change, so the
`.run(|app_handle, event| { ... state.player.lock().await.shutdown().await ... })`
block at the bottom of `run()` needs no change.

- [ ] **Step 7: Remove `SidecarPaths`'s now-unused `mpv` field**

After Step 6, nothing in the crate reads `SidecarPaths.mpv` anymore —
left in place, that field would fail `cargo clippy --all-targets -- -D
warnings` with a `dead_code` warning before Task 3 gets a chance to
remove the whole struct. Remove just the one field now instead of
leaving dead code for a later task to clean up. In
`src-tauri/src/media/sidecar_paths.rs`, remove `pub mpv: PathBuf,` from
the `SidecarPaths` struct and `mpv: PathBuf::from("mpv"),` from
`discover_dev()` — leave `yt_dlp`/`deno` untouched (Task 3 removes
those, and the struct itself, when it replaces this file).

- [ ] **Step 8: Add a `mock_builder()`-based `AppHandle` test helper, update the 3 fixtures**

`tauri::test::mock_builder()` produces `AppHandle<tauri::test::MockRuntime>`
— a genuinely different concrete type than production's
`AppHandle<tauri::Wry>`, which is exactly why `Player::start`/`load`
were made generic over `R: tauri::Runtime` in Step 2 rather than fixed
to a concrete handle type. Add this helper to each of the 3 fixture
files that construct a `Player` (`src-tauri/src/commands/session.rs`,
`src-tauri/src/commands/queue.rs`, `src-tauri/src/commands/crash.rs`,
inside each file's existing `#[cfg(test)] mod tests`):

```rust
fn test_app_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
    tauri::test::mock_builder()
        .plugin(tauri_plugin_shell::init())
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("mock tauri app should build")
        .handle()
        .clone()
}
```

None of these 3 fixtures' existing tests actually call `player.start()`/
`.load()` (they exercise queue/session/db/crash logic, not real
playback) — this helper only needs to exist so `Player::new`'s
constructor call in each `test_state()` compiles; you are not expected
to make these fixtures spawn a real sidecar. If `Player::new` no longer
needs any handle at construction time (per Step 2's design, it
doesn't — only `start`/`load` do), this helper may turn out to be
unused in `test_state()` itself; only add it where something in that
file's tests actually needs to call `.start()`/`.load()`. Check each
file before assuming it's needed everywhere.

Update each `test_state()`'s `Player::new(...)` call to match the new
3-argument constructor from Step 2 (drop whatever `PathBuf::from("mpv")`
first argument each fixture currently passes).

- [ ] **Step 9: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings &&
cargo test` (from `src-tauri/`). Expect all non-ignored tests to pass,
zero warnings (Step 7 already removed the one `dead_code` risk this
task would otherwise have introduced). The `#[ignore]` real-mpv smoke
tests in `player.rs` will now fail if run (`cargo test -- --ignored`)
— per this plan's Global Constraints, this is the accepted, ruled-on
trade-off from the upstream Tauri bug, not something to fix. Do not
attempt to make them pass; do not delete them either (they document
intent) — just don't claim they pass.

- [ ] **Step 10: Commit**

Never run `git commit` directly — stage every file this task touched
(`src-tauri/src/media/player.rs`, `src-tauri/src/media/sidecar_paths.rs`,
`src-tauri/src/error.rs`, `src-tauri/src/commands/mod.rs`,
`src-tauri/src/commands/mood.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/session.rs`, `src-tauri/src/commands/crash.rs`,
`src-tauri/src/lib.rs`), then invoke the `auto-commit` skill. Suggested
message: `feat(packaging): spawn mpv via tauri-plugin-shell`.

---

## Task 3: `Resolver` — sidecar spawn, event-accumulation, Deno path resolution

**Files:**

- Modify: `src-tauri/src/media/resolver.rs`
- Modify: `src-tauri/src/media/sidecar_paths.rs` (narrow to Deno-only;
  or delete and fold its one function into `resolver.rs` — pick
  whichever keeps `resolver.rs` from growing unreasonably large, per
  the plan's file-structure guidance; either is acceptable)
- Modify: `src-tauri/src/lib.rs` (`Resolver::new`/`ResolverConfig`
  construction)
- Modify: `src-tauri/src/mood_engine/mod.rs` (`generate_mixed_candidates`
  gains an `app` parameter)
- Modify: `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
  `src-tauri/src/commands/crash.rs` (`test_state()` fixtures —
  `Resolver::new`/`ResolverConfig` call sites)

**Interfaces:**

- Consumes: Task 2's `app: &tauri::AppHandle<R>` already threaded
  through `resolve_and_load`/`start_session_and_play` (and available to
  reuse inside `start_session_and_play`'s existing `app` parameter for
  the `top_up_queue` call this task adds — see Step 4);
  `tauri_plugin_shell::ShellExt` (Task 1).
- Produces: `Resolver::new(config: ResolverConfig) -> Resolver` (keeps
  the same shape — `ResolverConfig` still carries `deno_path: PathBuf`
  and `timeout: Duration`, but drops `yt_dlp_path: PathBuf` since
  yt-dlp is now spawned by sidecar name, not a resolved path).
  `Resolver::search<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, query: &str, limit: u32) -> Result<Vec<Track>>`,
  `Resolver::resolve_with_retry<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream>`
  — both gain the generic `app` parameter; `resolve` and `run` (private)
  do too, since they're on the call path.

- [ ] **Step 1: Resolve Deno's path — narrow `SidecarPaths` or fold it into `resolver.rs`**

This implements the spec's flagged "open risk" — the best-available
approach given no documented Tauri API resolves an `externalBin` path
directly. **This must be verified against a real built `.deb`/AppImage
before being trusted** (see Task 6's verification step) — do not treat
this code as correct just because it compiles.

Replace `src-tauri/src/media/sidecar_paths.rs` entirely with:

```rust
use std::path::PathBuf;

/// Deno's resolved path for yt-dlp's `--js-runtimes` argument — yt-dlp
/// spawns Deno itself as its own child process, so Echora needs Deno's
/// real filesystem path (not just the ability to spawn it), unlike mpv
/// and yt-dlp which Echora spawns directly by sidecar name.
///
/// No documented Tauri 2.x API resolves an `externalBin` entry to a
/// bare path (only `app.shell().sidecar(name)`, which spawns rather
/// than resolves) — this reasons from each package format's own
/// layout convention instead. **Unverified against a real built
/// package as of this comment** — confirm by building a real `.deb`
/// and AppImage and checking where `deno-<triple>` actually lands
/// before trusting this in production (see
/// docs/superpowers/plans/2026-09-01-packaging.md, Task 6).
pub fn resolve_deno_path() -> PathBuf {
    let triple = format!("{}-unknown-linux-gnu", std::env::consts::ARCH);
    let filename = format!("deno-{triple}");

    if let Ok(appdir) = std::env::var("APPDIR") {
        // Running from a mounted AppImage — Deno sits at the same
        // level Tauri places every externalBin/resource inside the
        // AppImage's own root.
        return PathBuf::from(appdir).join("usr/bin").join(&filename);
    }

    if let Ok(exe) = std::env::current_exe()
        && exe.starts_with("/usr/")
    {
        // A .deb install places externalBin binaries alongside the
        // main executable, both under /usr/bin/.
        if let Some(dir) = exe.parent() {
            return dir.join(&filename);
        }
    }

    // Dev mode (cargo run / cargo tauri dev, not a bundled build).
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(&filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_mode_resolves_relative_to_the_manifest_dir() {
        // SAFETY: this test reads but does not mutate APPDIR; no other
        // test in this crate touches it.
        unsafe {
            std::env::remove_var("APPDIR");
        }
        let path = resolve_deno_path();
        assert!(path.starts_with(env!("CARGO_MANIFEST_DIR")));
        assert!(path.to_string_lossy().contains("deno-"));
    }
}
```

- [ ] **Step 2: Rewrite `Resolver`'s `run()` to spawn via the shell plugin and accumulate events**

In `src-tauri/src/media/resolver.rs`, replace the imports and
`ResolverConfig`:

```rust
use std::path::PathBuf;
use std::time::Duration;

use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandEvent;
use tokio::time::timeout;

use super::metadata;
use crate::error::{EchoraError, Result};
use crate::models::{ResolvedStream, Track};

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub deno_path: PathBuf,
    pub timeout: Duration,
}
```

Replace `run()`:

```rust
    async fn run<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, args: Vec<String>) -> Result<String> {
        let (mut rx, _child) = app
            .shell()
            .sidecar("yt-dlp")
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?
            .args(&args)
            .spawn()
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?;

        let collect = async {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_success = false;
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Stdout(bytes) => stdout.extend_from_slice(&bytes),
                    CommandEvent::Stderr(bytes) => stderr.extend_from_slice(&bytes),
                    CommandEvent::Terminated(payload) => {
                        exit_success = payload.code == Some(0);
                        break;
                    }
                    CommandEvent::Error(_) => break,
                    _ => {}
                }
            }
            (stdout, stderr, exit_success)
        };

        let (stdout, stderr, exit_success) = timeout(self.config.timeout, collect)
            .await
            .map_err(|_| EchoraError::SidecarTimeout("yt-dlp".into()))?;

        if exit_success {
            Ok(String::from_utf8_lossy(&stdout).into_owned())
        } else {
            Err(metadata::classify_ytdlp_failure(&String::from_utf8_lossy(
                &stderr,
            )))
        }
    }
```

Note: `CommandEvent`'s `Stdout`/`Stderr` variants deliver
newline-delimited chunks already (per the shell plugin's own buffering,
confirmed against its docs), but this code doesn't depend on that —
it just concatenates raw bytes across every event until `Terminated`,
matching the old `wait_with_output()`'s "give me everything" semantics
exactly. `CommandEvent` may have other variants beyond the 4 handled
here in the installed `tauri-plugin-shell` version — the `_ => {}` arm
handles any that don't need action; if `cargo build` reports a
non-exhaustive match despite the wildcard arm, or if `Terminated`'s
payload field isn't named `code`, adjust to match the actual generated
type (check with `cargo doc --open -p tauri-plugin-shell` or
`cargo expand` if unsure — don't guess a field name that fails to
compile).

- [ ] **Step 3: Thread `app` through `search`/`resolve`/`resolve_with_retry`**

```rust
    pub async fn search<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, query: &str, limit: u32) -> Result<Vec<Track>> {
        let search_spec = format!("ytsearch{limit}:{query}");
        let stdout = self
            .run(
                app,
                vec![
                    "--js-runtimes".into(),
                    format!("deno:{}", self.config.deno_path.display()),
                    search_spec,
                    "--flat-playlist".into(),
                    "--dump-json".into(),
                    "--no-warnings".into(),
                ],
            )
            .await?;

        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(metadata::parse_search_result)
            .collect()
    }

    pub async fn resolve_with_retry<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream> {
        match self.resolve(app, track_id).await {
            Err(EchoraError::SidecarTimeout(_) | EchoraError::Io(_)) => {
                self.resolve(app, track_id).await
            }
            other => other,
        }
    }

    async fn resolve<R: tauri::Runtime>(&self, app: &tauri::AppHandle<R>, track_id: &str) -> Result<ResolvedStream> {
        let url = format!("https://www.youtube.com/watch?v={track_id}");
        let stdout = self
            .run(
                app,
                vec![
                    "--js-runtimes".into(),
                    format!("deno:{}", self.config.deno_path.display()),
                    url,
                    "-f".into(),
                    "bestaudio".into(),
                    "--dump-json".into(),
                    "--no-warnings".into(),
                    "--no-playlist".into(),
                ],
            )
            .await?;
        metadata::parse_resolved(&stdout, track_id)
    }
```

- [ ] **Step 4: Thread `app` into `generate_mixed_candidates` and `top_up_queue`**

This whole chain is this task's responsibility — Task 2 deliberately
left `top_up_queue` untouched (see Task 2, Step 4's note) specifically
because it's `Resolver`, not `Player`, that needs `app` here.

In `src-tauri/src/mood_engine/mod.rs`, add `app: &tauri::AppHandle<R>`
(generic over `R: tauri::Runtime`) as `generate_mixed_candidates`'s
first parameter, and pass it into its call to `resolver.search(...)`
around line 68 (`match resolver.search(&query,
config.results_per_query).await` becomes `match resolver.search(app,
&query, config.results_per_query).await`).

In `src-tauri/src/commands/mod.rs`, add `app: &tauri::AppHandle<R>` as
`top_up_queue`'s first parameter, and pass it into its call to
`generate_mixed_candidates`:

```rust
pub(crate) async fn top_up_queue<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    state: &AppState,
    moods: &[(String, u8)],
) -> Result<()> {
    // ...unchanged body, except:
    let candidates =
        mood_engine::generate_mixed_candidates(app, &resolved, &state.resolver, &ctx, &config, &mut rng)
            .await?;
    // ...
}
```

Then update its two callers:

1. `start_session_and_play` (same file, Task 2 already gave it an
   `app` parameter for its own `resolve_and_load` call) — change its
   existing `top_up_queue(state, moods).await?;` line to
   `top_up_queue(app, state, moods).await?;`.
2. `commands/queue.rs`'s `ensure_queue_topped_up` command (the one
   direct caller of `top_up_queue` that doesn't go through
   `start_session_and_play` — `super::top_up_queue(&state, &moods).await`
   at the time this plan was written; re-locate it, line numbers may
   have shifted) — add `app: tauri::AppHandle` to that command's own
   signature (Tauri auto-injects it) if it doesn't already have one,
   and pass `&app` into the call.

Verify with `cargo build` — a missed call site is a compile error
naming exactly where to fix next.

- [ ] **Step 5: Update `lib.rs`'s `Resolver::new`/`ResolverConfig` construction**

Replace:

```rust
            let sidecar_paths = media::sidecar_paths::SidecarPaths::discover_dev();
            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                yt_dlp_path: sidecar_paths.yt_dlp,
                deno_path: sidecar_paths.deno,
                timeout: std::time::Duration::from_secs(30),
            });
```

with:

```rust
            let resolver = media::resolver::Resolver::new(media::resolver::ResolverConfig {
                deno_path: media::sidecar_paths::resolve_deno_path(),
                timeout: std::time::Duration::from_secs(30),
            });
```

(This also removes the last use of `sidecar_paths.mpv`, so the
`SidecarPaths` struct/`discover_dev()` function from Task 2 is now
fully gone — Step 1 already replaced the whole file.)

- [ ] **Step 6: Update the 3 fixtures' `Resolver::new`/`ResolverConfig` calls**

In `src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/crash.rs`'s `test_state()`, remove the
`yt_dlp_path` field from each `ResolverConfig { ... }` literal (keep
`deno_path`/`timeout` — `PathBuf::from("deno")` as a dummy value is
fine, these tests never actually resolve anything real).

- [ ] **Step 7: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings &&
cargo test` (from `src-tauri/`). Expect all non-ignored tests to pass.
Same accepted gap as Task 2 applies to `resolver.rs`'s `#[ignore]`
real-network smoke tests — don't attempt to make `cargo test --
--ignored` pass for them.

- [ ] **Step 8: Commit**

Never run `git commit` directly — stage every file this task touched
(`src-tauri/src/media/resolver.rs`, `src-tauri/src/media/sidecar_paths.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/mood_engine/mod.rs`,
`src-tauri/src/commands/session.rs`, `src-tauri/src/commands/queue.rs`,
`src-tauri/src/commands/crash.rs`), then invoke the `auto-commit` skill.
Suggested message: `feat(packaging): spawn yt-dlp via tauri-plugin-shell, resolve deno path`.

---

## Task 4: mpv build script

**Files:**

- Create: `scripts/build-mpv.sh`

**Interfaces:**

- Produces: a script that, given `$TARGET_TRIPLE` and an output
  directory, builds mpv `v0.41.0` from source (audio-only Meson
  config), makes it relocatable via `patchelf`, and places
  `mpv-$TARGET_TRIPLE` plus its `lib/` sibling directory at the given
  output path. Task 6 (release workflow) invokes this script per
  matrix job.

- [ ] **Step 1: Write the build script**

Create `scripts/build-mpv.sh`:

```bash
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
  -Dwin32-desktop=disabled \
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
```

```bash
chmod +x scripts/build-mpv.sh
```

The `ldd | grep -E 'libav|libsw|libpostproc'` filter bundles FFmpeg's
own libraries (not guaranteed present system-wide at the exact ABI
version mpv was built against) while deliberately leaving out
`libasound`/`libpulse` (the ALSA/PulseAudio _client_ libraries — those
stay as ordinary system libraries via `.deb`'s `depends`, per the spec;
bundling them too would risk ABI conflicts with the system's own sound
server client at runtime).

- [ ] **Step 2: Attempt to actually run it in this environment**

Check whether this environment has the build dependencies mpv needs:
`which meson ninja gcc pkg-config patchelf`, and whether FFmpeg dev
headers are importable via `pkg-config --exists libavcodec libavformat
libavutil libswresample && echo yes`. If all present, actually run:

```bash
./scripts/build-mpv.sh x86_64-unknown-linux-gnu /tmp/mpv-build-test
```

and confirm the resulting binary runs (`--version` succeeds) and is
genuinely relocatable — copy the whole `/tmp/mpv-build-test` directory
somewhere else and confirm `mpv-x86_64-unknown-linux-gnu --version`
still works from the new location (proves the rpath fix actually
works, not just that it ran once from its original build directory).

If any build dependency is missing in this environment, **say so
explicitly in your report** rather than claiming the script works —
this is exactly the kind of runtime-sensitive thing this project
requires real verification for, not just "the script looks right."
Note precisely which command was unavailable and that the script's
correctness for a real CI run (Task 6) remains unverified locally.

- [ ] **Step 3: Commit**

Never run `git commit` directly — stage `scripts/build-mpv.sh`, then
invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add mpv from-source build script`.

---

## Task 5: yt-dlp/Deno production fetch + checksum verification script

**Files:**

- Create: `scripts/fetch-sidecar-binaries.sh`

**Interfaces:**

- Produces: a script that, given `$TARGET_TRIPLE` and an output
  directory, downloads yt-dlp's and Deno's official release binaries
  for the matching architecture, verifies them against upstream's
  published checksums, and places `yt-dlp-$TARGET_TRIPLE` and
  `deno-$TARGET_TRIPLE` at the given output path. Task 6 invokes this
  per matrix job.

- [ ] **Step 1: Write the script**

Create `scripts/fetch-sidecar-binaries.sh`:

```bash
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
curl -fL -o "/tmp/SHA256SUMS" \
  "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA256SUMS"
EXPECTED_SHA="$(grep " $YT_DLP_ASSET\$" /tmp/SHA256SUMS | awk '{print $1}')"
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
# Deno publishes per-asset .sha256sum files alongside each zip.
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
```

```bash
chmod +x scripts/fetch-sidecar-binaries.sh
```

Deno's exact per-asset checksum file naming/existence
(`$DENO_ZIP.sha256sum`) was reported by research, not independently
confirmed against a real Deno release page — verify this file actually
exists at that URL in Step 2 below before trusting it in CI; if it
doesn't, fall back to whatever checksum artifact Deno's release page
actually publishes (a single `SHASUMS256.txt`-style file covering all
assets is a common alternative pattern) and adjust the script.

- [ ] **Step 2: Actually run it**

This environment already has real yt-dlp/Deno dev binaries at
`src-tauri/binaries/{yt-dlp,deno}-x86_64-unknown-linux-gnu` from Task
1 — this script produces the same files a different way (fresh
download + checksum, not whatever was there before), so running it for
real is a direct, meaningful verification, not just a syntax check:

```bash
./scripts/fetch-sidecar-binaries.sh x86_64 x86_64-unknown-linux-gnu /tmp/sidecar-fetch-test
```

Confirm it downloads, verifies, and produces both files without error.
If the Deno checksum step fails because the assumed URL/file doesn't
exist (see Step 1's caveat), fix the script based on what Deno's real
release page actually publishes — don't leave a known-broken checksum
step in place.

- [ ] **Step 3: Commit**

Never run `git commit` directly — stage `scripts/fetch-sidecar-binaries.sh`,
then invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add yt-dlp/deno fetch and checksum script`.

---

## Task 6: `.github/workflows/release.yml`

**Files:**

- Create: `.github/workflows/release.yml`

**Interfaces:**

- Consumes: `scripts/build-mpv.sh` (Task 4), `scripts/fetch-sidecar-binaries.sh`
  (Task 5), the `externalBin`/`createUpdaterArtifacts` config (Task 1),
  the rewritten `Player`/`Resolver` (Tasks 2-3).
- Produces: a tag-triggered CI workflow publishing signed `.deb` +
  AppImage + `latest.json` to GitHub Releases for both architectures.

- [ ] **Step 1: Write the workflow**

Create `.github/workflows/release.yml`:

```yaml
name: Release

on:
  push:
    tags: ['v*.*.*']

env:
  CARGO_TERM_COLOR: always

jobs:
  release:
    name: Release (${{ matrix.arch }})
    strategy:
      fail-fast: false
      matrix:
        include:
          - arch: x86_64
            triple: x86_64-unknown-linux-gnu
            runner: ubuntu-24.04
          - arch: arm64
            triple: aarch64-unknown-linux-gnu
            runner: ubuntu-24.04-arm
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      - name: Verify tag matches tauri.conf.json version
        run: |
          TAG_VERSION="${GITHUB_REF_NAME#v}"
          CONF_VERSION="$(node -p "require('./src-tauri/tauri.conf.json').version")"
          if [ "$TAG_VERSION" != "$CONF_VERSION" ]; then
            echo "Tag $GITHUB_REF_NAME does not match tauri.conf.json version $CONF_VERSION" >&2
            exit 1
          fi

      - name: Install Tauri Linux prerequisites
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev build-essential curl wget file \
            libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev

      - name: Install mpv build prerequisites
        run: |
          sudo apt-get install -y meson ninja-build patchelf pkg-config \
            libavcodec-dev libavformat-dev libavutil-dev libswresample-dev \
            libasound2-dev libpulse-dev

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: npm

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - run: npm ci

      - name: Build mpv
        run: ./scripts/build-mpv.sh ${{ matrix.triple }} src-tauri/binaries

      - name: Fetch yt-dlp/Deno
        run: ./scripts/fetch-sidecar-binaries.sh ${{ matrix.arch }} ${{ matrix.triple }} src-tauri/binaries

      - uses: tauri-apps/tauri-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'Echora ${{ github.ref_name }}'
          releaseDraft: false
          prerelease: false
          includeUpdaterJson: true
```

- [ ] **Step 2: Validate the YAML**

Run whatever YAML/GitHub-Actions linting is available in this
environment — `actionlint .github/workflows/release.yml` if installed,
otherwise at minimum a plain YAML syntax check
(`python3 -c "import yaml, sys; yaml.safe_load(open('.github/workflows/release.yml'))"`
or equivalent). Report exactly which check you ran and its result —
this workflow cannot be executed end-to-end in this environment (no
real GitHub Actions runner, no tag push), so syntax validation is the
only automated verification available here.

- [ ] **Step 3: Do NOT push a tag or trigger a real release run**

Pushing a `vN.N.N` tag triggers a real CI run against the shared
GitHub repository (consumes Actions minutes, may create a real GitHub
Release if it succeeds) — that is a side effect outside this worktree
requiring the user's own explicit action, not something to do
autonomously. Stop here. In your final report, hand the user the exact
recommended first real verification (matching the spec's own Testing
section): push a scratch tag like `v0.1.1-test` from a disposable
branch, watch the workflow run, download the resulting `.deb`/AppImage,
and confirm the package installs, mpv plays real audio, yt-dlp resolves
a real track (confirming Deno's path resolution — the open risk from
Task 3 — actually works in a real built package), and Auto-update's
"Check for Updates" finds the published `latest.json`.

- [ ] **Step 4: Commit**

Never run `git commit` directly — stage `.github/workflows/release.yml`,
then invoke the `auto-commit` skill. Suggested message:
`feat(packaging): add tag-triggered release workflow`.
