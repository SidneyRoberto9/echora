# Crash Report Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the manual, opt-in crash-report feature: capture Rust
panics, unexpected mpv sidecar death, and unhandled JS errors to local
JSON files, and let the user open a pre-filled GitHub issue for any of
them from Settings — no automatic network call.

**Architecture:** A new `src-tauri/src/crash.rs` module owns all crash
persistence (write/list/format/clear, retention-capped at 10) as plain
functions over `app_dir`. Three capture points call into it: a chained
Rust panic hook, a reactive check in the mpv `Player`'s IPC error path,
and a new `report_frontend_crash` Tauri command driven by
`window.onerror`/`unhandledrejection` in React. An `Arc<AtomicBool>` on
`AppState`, kept in sync with the existing `crash_report_enabled`
setting, gates all three — off means zero disk I/O. Settings gets a new
list UI reading `crashes/` and opening `github.com/.../issues/new` via
the already-installed `@tauri-apps/plugin-opener`.

**Tech Stack:** Rust (`std::fs`, `std::backtrace`, `serde_json` — no new
crates), Tauri 2 commands, React/TypeScript.

**Spec:** `docs/superpowers/specs/2026-09-01-crash-report-design.md`

## Global Constraints

- No new Cargo or npm dependencies — everything needed already ships in
  the project (`std::fs`, `std::backtrace::Backtrace`, `serde_json`,
  `@tauri-apps/plugin-opener`).
- Toggle semantics: `crash_report_enabled` gates capture itself, not
  just the report button. Off = nothing written to disk, ever.
- Storage: one JSON file per crash in `<app_data_dir>/crashes/`, capped
  at the 10 most recent (oldest evicted first).
- Captured content is limited to crash kind, message, timestamp, app
  version, OS/arch, and (panics only) a backtrace — never user data
  (no search history, no played URLs, no listening history).
- yt-dlp resolve failures are **not** crashes — they already have their
  own `EchoraError` → UI path. Only unexpected mpv process death counts
  as a sidecar crash.
- No automatic network call anywhere in this feature — the GitHub issue
  opens only in direct response to the user clicking "Report".
- Before claiming any task done: `cargo fmt --check && cargo clippy
  --all-targets -- -D warnings && cargo test` (backend tasks) and/or
  `npm run lint && npm run build` (frontend task).
- Never run `git commit` directly — every task's commit step goes
  through the `auto-commit` skill instead (project rule, `~/.claude/CLAUDE.md`).

---

## Task 1: Core crash module

**Files:**
- Create: `src-tauri/src/crash.rs`
- Modify: `src-tauri/src/lib.rs:1-10` (add `mod crash;` to the existing
  `mod` list only — no wiring yet, that's Task 2)

**Interfaces:**
- Produces (all `pub`, used by later tasks — exact names/signatures
  other tasks depend on):
  - `pub enum CrashKind { Panic, SidecarCrash, FrontendError }`
  - `pub struct CrashRecord { kind: CrashKind, message: String, location: Option<String>, backtrace: Option<String>, timestamp: u64 /* unix millis */, app_version: String, os: String, arch: String }`
  - `CrashRecord::from_panic(info: &std::panic::PanicHookInfo) -> CrashRecord`
  - `CrashRecord::from_sidecar(process: &str, detail: &str) -> CrashRecord`
  - `CrashRecord::from_frontend(message: String, stack: Option<String>) -> CrashRecord`
  - `pub struct CrashSummary { id: String, kind: CrashKind, timestamp: u64, message: String }`
  - `pub fn record(app_dir: &Path, event: CrashRecord) -> std::io::Result<()>`
  - `pub fn list(app_dir: &Path) -> std::io::Result<Vec<CrashSummary>>`
  - `pub fn to_markdown(app_dir: &Path, id: &str) -> crate::error::Result<String>`
  - `pub fn clear(app_dir: &Path) -> std::io::Result<()>`
  - `pub fn install_panic_hook(app_dir: PathBuf, enabled: Arc<AtomicBool>)`

- [ ] **Step 1: Write the full test module first**

Create `src-tauri/src/crash.rs` with **only** the test module below (no
implementation yet) plus the minimal `use` lines it needs — this will
fail to compile, which is the expected RED state for this module:

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{EchoraError, Result};

const MAX_RETAINED: usize = 10;
const MAX_MARKDOWN_BODY_CHARS: usize = 4000;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    fn unique_temp_dir(label: &str) -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "echora-crash-test-{}-{}-{n}",
            std::process::id(),
            label
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn record_and_list_round_trip() {
        let dir = unique_temp_dir("roundtrip");
        record(
            &dir,
            CrashRecord::from_frontend("boom".into(), Some("at x.ts:1".into())),
        )
        .unwrap();

        let summaries = list(&dir).unwrap();

        assert_eq!(summaries.len(), 1);
        assert!(matches!(summaries[0].kind, CrashKind::FrontendError));
        assert_eq!(summaries[0].message, "boom");
    }

    #[test]
    fn list_tolerates_a_missing_crashes_directory() {
        let dir = unique_temp_dir("missing-dir");

        let summaries = list(&dir).unwrap();

        assert!(summaries.is_empty());
    }

    #[test]
    fn retention_caps_at_ten_and_evicts_oldest_first() {
        let dir = unique_temp_dir("retention");
        for i in 0..11u64 {
            let mut r = CrashRecord::from_frontend(format!("crash-{i}"), None);
            // Deterministic, strictly increasing, same digit-width as real
            // millis timestamps so `enforce_retention`'s lexicographic
            // filename sort matches chronological order (see the
            // `timestamp` field's doc comment on `CrashRecord`).
            r.timestamp = 1_000_000 + i;
            record(&dir, r).unwrap();
        }

        let summaries = list(&dir).unwrap();

        assert_eq!(summaries.len(), 10);
        assert!(
            summaries.iter().all(|s| s.message != "crash-0"),
            "oldest entry should have been evicted"
        );
        assert!(
            summaries.iter().any(|s| s.message == "crash-10"),
            "newest entry should be retained"
        );
    }

    #[test]
    fn to_markdown_includes_fields_and_truncates_long_backtrace() {
        let dir = unique_temp_dir("markdown");
        let long_backtrace = "x".repeat(MAX_MARKDOWN_BODY_CHARS + 500);
        let record_data = CrashRecord::new(
            CrashKind::Panic,
            "kaboom".into(),
            Some("src/foo.rs:10".into()),
            Some(long_backtrace),
        );
        record(&dir, record_data).unwrap();
        let id = list(&dir).unwrap()[0].id.clone();

        let body = to_markdown(&dir, &id).unwrap();

        assert!(body.contains("kaboom"));
        assert!(body.contains("src/foo.rs:10"));
        assert!(body.contains("(truncated)"));
    }

    #[test]
    fn to_markdown_missing_id_returns_io_error() {
        let dir = unique_temp_dir("markdown-missing");

        let err = to_markdown(&dir, "does-not-exist").unwrap_err();

        assert!(matches!(err, EchoraError::Io(_)));
    }

    #[test]
    fn clear_removes_all_stored_reports() {
        let dir = unique_temp_dir("clear");
        record(&dir, CrashRecord::from_frontend("one".into(), None)).unwrap();
        record(&dir, CrashRecord::from_frontend("two".into(), None)).unwrap();

        clear(&dir).unwrap();

        assert!(list(&dir).unwrap().is_empty());
    }

    #[test]
    fn from_sidecar_builds_a_sidecar_crash_record() {
        let record = CrashRecord::from_sidecar("mpv", "connection refused");

        assert!(matches!(record.kind, CrashKind::SidecarCrash));
        assert!(record.message.contains("mpv"));
        assert!(record.message.contains("connection refused"));
    }

    #[test]
    fn install_panic_hook_writes_a_record_when_enabled() {
        let dir = unique_temp_dir("panic-hook-on");
        let enabled = Arc::new(AtomicBool::new(true));
        let previous = std::panic::take_hook();

        install_panic_hook(dir.clone(), enabled);
        let result = std::panic::catch_unwind(|| panic!("panic-hook-test-marker"));
        std::panic::set_hook(previous);

        assert!(result.is_err());
        let summaries = list(&dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].message.contains("panic-hook-test-marker"));
    }

    #[test]
    fn install_panic_hook_writes_nothing_when_disabled() {
        let dir = unique_temp_dir("panic-hook-off");
        let enabled = Arc::new(AtomicBool::new(false));
        let previous = std::panic::take_hook();

        install_panic_hook(dir.clone(), enabled);
        let result = std::panic::catch_unwind(|| panic!("should-not-be-recorded"));
        std::panic::set_hook(previous);

        assert!(result.is_err());
        assert!(list(&dir).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml crash::`
Expected: FAIL to compile — `CrashRecord`, `CrashKind`, `record`,
`list`, `to_markdown`, `clear`, `install_panic_hook` are not defined
yet.

- [ ] **Step 3: Write the implementation**

Insert the following above the `#[cfg(test)] mod tests` block (after
the `const` declarations already in the file from Step 1):

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CrashKind {
    Panic,
    SidecarCrash,
    FrontendError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashRecord {
    pub kind: CrashKind,
    pub message: String,
    pub location: Option<String>,
    pub backtrace: Option<String>,
    /// Unix millis, not seconds — the filename embeds this directly, and
    /// second resolution would let two fast crashes of the same kind
    /// collide and silently overwrite each other.
    pub timestamp: u64,
    pub app_version: String,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashSummary {
    pub id: String,
    pub kind: CrashKind,
    pub timestamp: u64,
    pub message: String,
}

impl CrashRecord {
    fn new(
        kind: CrashKind,
        message: String,
        location: Option<String>,
        backtrace: Option<String>,
    ) -> Self {
        CrashRecord {
            kind,
            message,
            location,
            backtrace,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }

    pub fn from_panic(info: &std::panic::PanicHookInfo) -> Self {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic".to_string());
        let location = info.location().map(|l| l.to_string());
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        CrashRecord::new(CrashKind::Panic, message, location, Some(backtrace))
    }

    pub fn from_sidecar(process: &str, detail: &str) -> Self {
        CrashRecord::new(
            CrashKind::SidecarCrash,
            format!("{process} sidecar died unexpectedly: {detail}"),
            None,
            None,
        )
    }

    pub fn from_frontend(message: String, stack: Option<String>) -> Self {
        CrashRecord::new(CrashKind::FrontendError, message, None, stack)
    }
}

fn crashes_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("crashes")
}

fn kind_slug(kind: &CrashKind) -> &'static str {
    match kind {
        CrashKind::Panic => "panic",
        CrashKind::SidecarCrash => "sidecar",
        CrashKind::FrontendError => "frontend",
    }
}

/// Best-effort: a failure to write a crash record must never itself panic
/// or bubble up as a user-facing error, so every caller discards the
/// `Result` with `let _ =`.
pub fn record(app_dir: &Path, event: CrashRecord) -> std::io::Result<()> {
    let dir = crashes_dir(app_dir);
    fs::create_dir_all(&dir)?;
    let filename = format!("{}-{}.json", event.timestamp, kind_slug(&event.kind));
    fs::write(dir.join(filename), serde_json::to_vec_pretty(&event)?)?;
    enforce_retention(&dir)
}

fn enforce_retention(dir: &Path) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    // Filenames are timestamp-prefixed with a fixed digit width for the
    // foreseeable future, so lexicographic sort matches chronological.
    entries.sort();
    if entries.len() > MAX_RETAINED {
        for stale in &entries[..entries.len() - MAX_RETAINED] {
            let _ = fs::remove_file(stale);
        }
    }
    Ok(())
}

pub fn list(app_dir: &Path) -> std::io::Result<Vec<CrashSummary>> {
    let dir = crashes_dir(app_dir);
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut summaries: Vec<CrashSummary> = fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let path = e.path();
            let id = path.file_stem()?.to_str()?.to_string();
            let record: CrashRecord = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
            Some(CrashSummary {
                id,
                kind: record.kind,
                timestamp: record.timestamp,
                message: record.message,
            })
        })
        .collect();
    summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    Ok(summaries)
}

fn read_one(app_dir: &Path, id: &str) -> Result<CrashRecord> {
    let path = crashes_dir(app_dir).join(format!("{id}.json"));
    let bytes = fs::read(path).map_err(EchoraError::Io)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn to_markdown(app_dir: &Path, id: &str) -> Result<String> {
    let r = read_one(app_dir, id)?;
    let mut body = format!(
        "**Kind**: {:?}\n**When**: {}\n**App version**: {}\n**OS/Arch**: {}/{}\n\n**Message**:\n```\n{}\n```\n",
        r.kind, r.timestamp, r.app_version, r.os, r.arch, r.message,
    );
    if let Some(loc) = &r.location {
        body.push_str(&format!("\n**Location**: {loc}\n"));
    }
    if let Some(bt) = &r.backtrace {
        let truncated = bt.chars().count() > MAX_MARKDOWN_BODY_CHARS;
        // char-boundary-safe: byte-index String::truncate can panic
        // mid-UTF8 sequence (a backtrace can contain non-ASCII paths).
        let bt: String = bt.chars().take(MAX_MARKDOWN_BODY_CHARS).collect();
        body.push_str(&format!(
            "\n**Backtrace**{}:\n```\n{}\n```\n",
            if truncated { " (truncated)" } else { "" },
            bt
        ));
    }
    Ok(body)
}

pub fn clear(app_dir: &Path) -> std::io::Result<()> {
    let dir = crashes_dir(app_dir);
    if dir.exists() {
        fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

/// Chains onto Rust's default panic hook (so `cargo tauri dev` still
/// prints panics to stderr) and additionally persists a crash record
/// when enabled. Call once, at startup. Must never itself panic.
pub fn install_panic_hook(app_dir: PathBuf, enabled: Arc<AtomicBool>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if enabled.load(Ordering::Relaxed) {
            let _ = record(&app_dir, CrashRecord::from_panic(info));
        }
        default_hook(info);
    }));
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml crash::`
Expected: PASS — all 9 tests green.

- [ ] **Step 5: Register the module**

In `src-tauri/src/lib.rs`, add `mod crash;` to the existing `mod` list
at the top of the file, between `mod commands;` and `mod db;` (keeping
the list's existing alphabetical order):

```rust
mod commands;
mod crash;
mod db;
mod error;
```

- [ ] **Step 6: Full verification**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings
&& cargo test` (all from `src-tauri/`)
Expected: all pass, zero warnings. (Every `pub` item in `crash.rs` is
exercised by its own test module, so `dead_code` does not fire even
though nothing outside the module calls it yet.)

- [ ] **Step 7: Commit**

Never run `git commit` directly (project rule in `~/.claude/CLAUDE.md`)
— stage `src-tauri/src/crash.rs` and `src-tauri/src/lib.rs`, then
invoke the `auto-commit` skill. Suggested Conventional Commit message
for it to use: `feat(crash-report): add crash record storage and panic hook`.

---

## Task 2: AppState wiring and panic hook installation

**Files:**
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs:34-94` (the `run()` function's `setup`
  closure)
- Modify: `src-tauri/src/commands/session.rs` (test fixture)
- Modify: `src-tauri/src/commands/queue.rs` (test fixture)

**Interfaces:**
- Consumes: `crash::install_panic_hook` from Task 1.
- Produces: `AppState.app_dir: PathBuf`, `AppState.crash_reporting_enabled: Arc<AtomicBool>` — Tasks 3 and 4 read both fields.

- [ ] **Step 1: Add the two fields to `AppState`**

In `src-tauri/src/state.rs`, update the imports and struct:

```rust
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

use crate::db::Db;
use crate::media;
use crate::media::player::Player;
use crate::media::resolver::Resolver;
use crate::moods::MoodCatalog;
use crate::platform::mpris;
use crate::queue::Queue;

pub struct AppState {
    pub db: Mutex<Db>,
    pub queue: Mutex<Queue>,
    pub moods: MoodCatalog,
    pub resolver: Resolver,
    pub player: tokio::sync::Mutex<Player>,
    pub mpris: Option<mpris::Handle>,
    pub sponsorblock_segments: Mutex<Vec<media::sponsorblock::Segment>>,
    /// The resolved app data directory — stored here so commands (e.g.
    /// crash reporting) can reach it without threading an `AppHandle`
    /// through every call.
    pub app_dir: PathBuf,
    /// Mirrors `Settings.crash_report_enabled`, kept in sync by
    /// `update_settings`. An `Arc` because the panic hook and the mpv
    /// `Player` each need their own clone to check it without touching
    /// `AppState` (the panic hook in particular must not lock anything).
    pub crash_reporting_enabled: Arc<AtomicBool>,
}
```

(Doc comments on the existing fields are unchanged — only the two new
fields and their imports are additions.)

- [ ] **Step 2: Wire it in `lib.rs`'s `setup()`**

In `src-tauri/src/lib.rs`, replace this block:

```rust
            let moods = moods::MoodCatalog::load().expect("bundled moods.json should load");

            let sidecar_paths = media::sidecar_paths::SidecarPaths::discover_dev();
```

with:

```rust
            let moods = moods::MoodCatalog::load().expect("bundled moods.json should load");

            let initial_settings = db.get_settings()?;
            let crash_reporting_enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
                initial_settings.crash_report_enabled,
            ));

            let sidecar_paths = media::sidecar_paths::SidecarPaths::discover_dev();
```

Then replace this block:

```rust
            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                player: tokio::sync::Mutex::new(player),
                mpris,
                sponsorblock_segments: Mutex::new(Vec::new()),
            });

            platform::tray::setup(app)?;

            // Keep the OS-level autostart entry truthful to the saved
            // setting even if it drifted (manually removed, fresh profile).
            let autostart_enabled = app
                .state::<AppState>()
                .db
                .lock()
                .unwrap()
                .get_settings()?
                .autostart_enabled;
            platform::autostart::sync(app.handle(), autostart_enabled)?;

            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));

            Ok(())
```

with:

```rust
            app.manage(AppState {
                db: Mutex::new(db),
                queue: Mutex::new(queue::Queue::new()),
                moods,
                resolver,
                player: tokio::sync::Mutex::new(player),
                mpris,
                sponsorblock_segments: Mutex::new(Vec::new()),
                app_dir: app_dir.clone(),
                crash_reporting_enabled: crash_reporting_enabled.clone(),
            });

            platform::tray::setup(app)?;

            // Keep the OS-level autostart entry truthful to the saved
            // setting even if it drifted (manually removed, fresh profile).
            // Reuses `initial_settings` read above instead of a second
            // `get_settings()` query.
            platform::autostart::sync(app.handle(), initial_settings.autostart_enabled)?;

            // Chain onto Rust's default panic hook (keeps stderr output
            // for `cargo tauri dev`) and additionally persist a crash
            // record — best-effort, must never itself panic.
            crash::install_panic_hook(app_dir.clone(), crash_reporting_enabled.clone());

            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));

            Ok(())
```

Leave the `let player = media::player::Player::new(sidecar_paths.mpv,
app_dir.join("mpv-ipc.sock"));` line exactly as-is for now — Task 3
changes its signature and this call site together.

- [ ] **Step 3: Update the `commands/session.rs` test fixture**

In `src-tauri/src/commands/session.rs`'s `test_state()`, add the two
new fields (`Player::new` stays 2-arg here — Task 3 updates it):

```rust
    fn test_state() -> AppState {
        use crate::media::player::Player;
        use crate::media::resolver::{Resolver, ResolverConfig};
        use std::path::PathBuf;
        use std::time::Duration;

        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                yt_dlp_path: PathBuf::from("yt-dlp"),
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-unused.sock"),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
```

- [ ] **Step 4: Update the `commands/queue.rs` test fixture**

Same addition in `src-tauri/src/commands/queue.rs`'s `test_state()`:

```rust
    fn test_state() -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                yt_dlp_path: PathBuf::from("yt-dlp"),
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-queue-unused.sock"),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir: std::env::temp_dir(),
            crash_reporting_enabled: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
```

- [ ] **Step 5: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings
&& cargo test` (from `src-tauri/`)
Expected: all pass. This confirms `AppState` compiles with the new
fields everywhere it's constructed, and that the existing test suite
(all prior features) still passes unchanged.

- [ ] **Step 6: Commit**

Never run `git commit` directly — stage `src-tauri/src/state.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/commands/session.rs`, and
`src-tauri/src/commands/queue.rs`, then invoke the `auto-commit` skill.
Suggested message: `feat(crash-report): wire crash flag and app_dir into AppState`.

---

## Task 3: mpv sidecar crash detection

**Files:**
- Modify: `src-tauri/src/media/player.rs`
- Modify: `src-tauri/src/lib.rs` (the `Player::new(...)` call site)
- Modify: `src-tauri/src/commands/mod.rs:61` (`toggle_play_pause`)
- Modify: `src-tauri/src/media/sponsorblock.rs:182`
- Modify: `src-tauri/src/platform/mpris.rs:94,256,336`
- Modify: `src-tauri/src/commands/session.rs` (test fixture —
  `Player::new` call)
- Modify: `src-tauri/src/commands/queue.rs` (test fixture —
  `Player::new` call)

**Interfaces:**
- Consumes: `crash::record`, `crash::CrashRecord::from_sidecar` (Task
  1); `AppState.app_dir`, `AppState.crash_reporting_enabled` (Task 2).
- Produces: `Player::new(mpv_path: PathBuf, socket_path: PathBuf,
  app_dir: PathBuf, crash_reporting_enabled: Arc<AtomicBool>) -> Player`
  — no later task depends on this beyond Task 2's `lib.rs` call site,
  already covered here.

- [ ] **Step 1: Change `Player`'s struct and constructor**

In `src-tauri/src/media/player.rs`, add imports and update the struct:

```rust
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

use crate::crash;
use crate::error::{EchoraError, Result};

pub struct Player {
    mpv_path: PathBuf,
    socket_path: PathBuf,
    child: Option<Child>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
}

impl Player {
    pub fn new(
        mpv_path: PathBuf,
        socket_path: PathBuf,
        app_dir: PathBuf,
        crash_reporting_enabled: Arc<AtomicBool>,
    ) -> Self {
        Player {
            mpv_path,
            socket_path,
            child: None,
            app_dir,
            crash_reporting_enabled,
        }
    }
```

- [ ] **Step 2: Add reactive crash detection to `send_command`**

Replace the existing `send_command` method:

```rust
    async fn send_command(&self, command: Value) -> Result<Value> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(EchoraError::Io)?;
```

with:

```rust
    async fn send_command(&mut self, command: Value) -> Result<Value> {
        let stream = match UnixStream::connect(&self.socket_path).await {
            Ok(s) => s,
            Err(err) => {
                if self.child.is_some() {
                    // mpv is supposed to be running but its socket is
                    // gone — it died outside our own shutdown() path.
                    if self.crash_reporting_enabled.load(Ordering::Relaxed) {
                        let _ = crash::record(
                            &self.app_dir,
                            crash::CrashRecord::from_sidecar("mpv", &err.to_string()),
                        );
                    }
                    self.child = None; // it's actually gone; stop treating it as started
                }
                return Err(EchoraError::Io(err));
            }
        };
```

(The rest of the method body — building and sending `payload`, reading
the reply — is unchanged.)

- [ ] **Step 3: Update every other method's `&self` → `&mut self`**

`send_command` is now `&mut self`, so every method that forwards into
it must be too. In the same file, change the `&self` in each of these
signatures to `&mut self` (bodies unchanged):

```rust
    pub async fn load(&mut self, stream_url: &str) -> Result<()> {
    pub async fn set_paused(&mut self, paused: bool) -> Result<()> {
    pub async fn is_paused(&mut self) -> Result<Option<bool>> {
    pub async fn set_volume(&mut self, volume_percent: u8) -> Result<()> {
    pub async fn volume_percent(&mut self) -> Result<Option<u8>> {
    pub async fn seek_to(&mut self, seconds: f64) -> Result<()> {
    pub async fn position_seconds(&mut self) -> Result<Option<f64>> {
    pub async fn duration_seconds(&mut self) -> Result<Option<f64>> {
```

`is_started(&self)`, `start(&mut self)`, `wait_for_socket(&self)`, and
`shutdown(&mut self)` are unchanged — they don't call `send_command`.

- [ ] **Step 4: Fix call sites that bind the player guard without `mut`**

Chained call sites like `state.player.lock().await.set_paused(true).await?`
(no named binding) keep compiling as-is — a temporary doesn't need
`let mut`. But 5 call sites bind the guard to a name first and then
call one of the now-`&mut self` methods on it, which needs the binding
itself declared `mut`. Make exactly this one-word change in each:

In `src-tauri/src/commands/mod.rs:61` (`toggle_play_pause`):
```rust
    let mut player = state.player.lock().await;
```

In `src-tauri/src/media/sponsorblock.rs:182`:
```rust
        let mut player = state.player.lock().await;
```

In `src-tauri/src/platform/mpris.rs:94` (`playback_status_for`):
```rust
    let mut player = state.player.lock().await;
```

In `src-tauri/src/platform/mpris.rs:256` (inside `seek`):
```rust
            let mut player = state.player.lock().await;
```

In `src-tauri/src/platform/mpris.rs:336` (`volume`):
```rust
        let mut player = state.player.lock().await;
```

No other lines in any of these functions change.

- [ ] **Step 5: Update the `lib.rs` `Player::new` call site**

In `src-tauri/src/lib.rs`, replace:

```rust
            let player =
                media::player::Player::new(sidecar_paths.mpv, app_dir.join("mpv-ipc.sock"));
```

with:

```rust
            let player = media::player::Player::new(
                sidecar_paths.mpv,
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );
```

- [ ] **Step 6: Update both test fixtures' `Player::new` calls**

In `src-tauri/src/commands/session.rs`'s `test_state()`, change:

```rust
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-unused.sock"),
            )),
```

to:

```rust
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-unused.sock"),
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
```

In `src-tauri/src/commands/queue.rs`'s `test_state()`, change:

```rust
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-queue-unused.sock"),
            )),
```

to:

```rust
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-queue-unused.sock"),
                std::env::temp_dir(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            )),
```

- [ ] **Step 7: Update the two existing `#[ignore]` smoke tests' constructors**

In `src-tauri/src/media/player.rs`'s `mod smoke_tests`, change:

```rust
        let mut player = Player::new(PathBuf::from("mpv"), socket_path.clone());
```

to:

```rust
        let mut player = Player::new(
            PathBuf::from("mpv"),
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
```

and:

```rust
        let mut player = Player::new(PathBuf::from("mpv"), dev_socket_path());
```

to:

```rust
        let mut player = Player::new(
            PathBuf::from("mpv"),
            dev_socket_path(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
```

- [ ] **Step 8: Add a new `#[ignore]` smoke test for the crash path itself**

In the same `mod smoke_tests` block, add:

```rust
    #[tokio::test]
    #[ignore]
    async fn mpv_dying_unexpectedly_is_recorded_as_a_sidecar_crash() {
        // Fully-qualified `Arc`/`AtomicBool`/`Duration` here rather than
        // relying on `use super::*` to have brought them in — matches
        // this file's existing `StdDuration` alias in the test above,
        // which does the same for the same reason.
        let socket_path = dev_socket_path();
        let app_dir = std::env::temp_dir().join(format!("echora-crash-smoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&app_dir);
        let enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let mut player = Player::new(PathBuf::from("mpv"), socket_path.clone(), app_dir.clone(), enabled);
        player.start().await.unwrap();

        // Kill mpv out-of-band — not through shutdown() — to simulate a
        // real unexpected sidecar death.
        let pid = player.child.as_ref().unwrap().id().unwrap();
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let err = player.set_volume(50).await.unwrap_err();

        assert!(matches!(err, crate::error::EchoraError::Io(_)));
        let summaries = crate::crash::list(&app_dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(matches!(summaries[0].kind, crate::crash::CrashKind::SidecarCrash));
    }
```

Note: `player.child` is a private field, readable here only because
`smoke_tests` is a submodule of `player.rs` (private items are visible
to descendant modules in Rust, same as every other test in this file
that already reaches into `Player`'s internals).

- [ ] **Step 9: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings
&& cargo test` (from `src-tauri/`)
Expected: all non-ignored tests pass, zero warnings.

Then, only if `mpv` is present on `PATH` in this environment: run
`cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
mpv_dying_unexpectedly_is_recorded_as_a_sidecar_crash` and confirm it
passes. If `mpv` isn't available here, state explicitly in the task
report that this specific test was not run and needs manual
verification wherever `mpv` is installed — do not claim it passes
without having run it.

- [ ] **Step 10: Commit**

Never run `git commit` directly — stage `src-tauri/src/media/player.rs`,
`src-tauri/src/lib.rs`, `src-tauri/src/commands/mod.rs`,
`src-tauri/src/media/sponsorblock.rs`, `src-tauri/src/platform/mpris.rs`,
`src-tauri/src/commands/session.rs`, and `src-tauri/src/commands/queue.rs`,
then invoke the `auto-commit` skill. Suggested message:
`feat(crash-report): detect and record unexpected mpv sidecar death`.

---

## Task 4: Tauri commands and settings sync

**Files:**
- Create: `src-tauri/src/commands/crash.rs`
- Modify: `src-tauri/src/commands/mod.rs:1` (register the module)
- Modify: `src-tauri/src/commands/settings.rs`
- Modify: `src-tauri/src/lib.rs` (`invoke_handler` list)

**Interfaces:**
- Consumes: `crash::list`, `crash::to_markdown`, `crash::clear`,
  `crash::CrashRecord::from_frontend`, `crash::record` (Task 1);
  `AppState.app_dir`, `AppState.crash_reporting_enabled` (Task 2).
- Produces: Tauri commands `list_crash_reports`,
  `get_crash_report_markdown`, `clear_crash_reports`,
  `report_frontend_crash` — Task 5's frontend calls these by name.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/commands/crash.rs` with the command functions'
signatures plus this test module (the implementations in Step 2 don't
exist yet, so this fails to compile — the expected RED state):

```rust
use tauri::State;

use crate::crash::{self, CrashSummary};
use crate::error::Result;
use crate::state::AppState;

#[tauri::command]
pub fn list_crash_reports(state: State<AppState>) -> Result<Vec<CrashSummary>> {
    crash::list(&state.app_dir).map_err(crate::error::EchoraError::Io)
}

#[tauri::command]
pub fn get_crash_report_markdown(state: State<AppState>, id: String) -> Result<String> {
    crash::to_markdown(&state.app_dir, &id)
}

#[tauri::command]
pub fn clear_crash_reports(state: State<AppState>) -> Result<()> {
    crash::clear(&state.app_dir).map_err(crate::error::EchoraError::Io)
}

#[tauri::command]
pub fn report_frontend_crash(
    state: State<AppState>,
    message: String,
    stack: Option<String>,
) -> Result<()> {
    if state
        .crash_reporting_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        let _ = crash::record(&state.app_dir, crash::CrashRecord::from_frontend(message, stack));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::media::player::Player;
    use crate::media::resolver::{Resolver, ResolverConfig};
    use crate::moods::MoodCatalog;
    use crate::queue::Queue;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn unique_temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "echora-crash-cmd-test-{}-{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn test_state(app_dir: PathBuf, crash_reporting_enabled: bool) -> AppState {
        AppState {
            db: Mutex::new(Db::open_in_memory().unwrap()),
            queue: Mutex::new(Queue::new()),
            moods: MoodCatalog::load().unwrap(),
            resolver: Resolver::new(ResolverConfig {
                yt_dlp_path: PathBuf::from("yt-dlp"),
                deno_path: PathBuf::from("deno"),
                timeout: Duration::from_secs(30),
            }),
            player: tokio::sync::Mutex::new(Player::new(
                PathBuf::from("mpv"),
                PathBuf::from("/tmp/echora-test-crash-cmd-unused.sock"),
                app_dir.clone(),
                Arc::new(AtomicBool::new(false)),
            )),
            mpris: None,
            sponsorblock_segments: Mutex::new(Vec::new()),
            app_dir,
            crash_reporting_enabled: Arc::new(AtomicBool::new(crash_reporting_enabled)),
        }
    }

    #[test]
    fn report_frontend_crash_is_a_no_op_when_disabled() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), false);

        report_frontend_crash_impl(&state, "should not persist".into(), None);

        assert!(crash::list(&dir).unwrap().is_empty());
    }

    #[test]
    fn report_frontend_crash_persists_when_enabled() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), true);

        report_frontend_crash_impl(&state, "persisted".into(), Some("stack trace".into()));

        let summaries = crash::list(&dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].message, "persisted");
    }

    #[test]
    fn list_and_clear_round_trip_through_the_command_functions() {
        let dir = unique_temp_dir();
        let state = test_state(dir.clone(), true);
        report_frontend_crash_impl(&state, "one".into(), None);
        report_frontend_crash_impl(&state, "two".into(), None);

        let listed = crash::list(&state.app_dir).unwrap();
        assert_eq!(listed.len(), 2);

        crash::clear(&state.app_dir).unwrap();
        assert!(crash::list(&state.app_dir).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Extract a testable `report_frontend_crash_impl`**

The test module above calls `report_frontend_crash_impl`, which
doesn't exist yet — this matches this codebase's established pattern
(see `commands/session.rs`'s `start_session_impl`) of a plain,
directly-testable function behind the thin `#[tauri::command]`
wrapper, since a `#[tauri::command]` taking `State<AppState>` can't be
called directly from a unit test without a real Tauri `App`. Replace
the `report_frontend_crash` command written in Step 1 with:

```rust
pub(crate) fn report_frontend_crash_impl(state: &AppState, message: String, stack: Option<String>) {
    if state
        .crash_reporting_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        let _ = crash::record(&state.app_dir, crash::CrashRecord::from_frontend(message, stack));
    }
}

#[tauri::command]
pub fn report_frontend_crash(state: State<AppState>, message: String, stack: Option<String>) -> Result<()> {
    report_frontend_crash_impl(&state, message, stack);
    Ok(())
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml commands::crash::`
Expected: PASS — all 3 tests green.

- [ ] **Step 4: Register the module**

In `src-tauri/src/commands/mod.rs`, add `pub mod crash;` as the first
line (alphabetically before `pub mod library;`):

```rust
pub mod crash;
pub mod library;
pub mod mood;
pub mod playback;
pub mod queue;
pub mod session;
pub mod settings;
```

- [ ] **Step 5: Sync the flag in `update_settings`**

In `src-tauri/src/commands/settings.rs`, replace:

```rust
#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<()> {
    state.db.lock().unwrap().save_settings(&settings)?;
    autostart::sync(&app, settings.autostart_enabled)
}
```

with:

```rust
#[tauri::command]
pub fn update_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<()> {
    state.db.lock().unwrap().save_settings(&settings)?;
    state
        .crash_reporting_enabled
        .store(settings.crash_report_enabled, std::sync::atomic::Ordering::Relaxed);
    autostart::sync(&app, settings.autostart_enabled)
}
```

- [ ] **Step 6: Register the 4 new commands in `lib.rs`**

In `src-tauri/src/lib.rs`'s `invoke_handler(tauri::generate_handler![...])`
list, add these 4 lines right after `commands::settings::update_settings,`:

```rust
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::crash::list_crash_reports,
            commands::crash::get_crash_report_markdown,
            commands::crash::clear_crash_reports,
            commands::crash::report_frontend_crash,
```

- [ ] **Step 7: Verify**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings
&& cargo test` (from `src-tauri/`)
Expected: all pass, zero warnings.

- [ ] **Step 8: Commit**

Never run `git commit` directly — stage `src-tauri/src/commands/crash.rs`,
`src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/settings.rs`,
and `src-tauri/src/lib.rs`, then invoke the `auto-commit` skill.
Suggested message: `feat(crash-report): add crash report Tauri commands`.

---

## Task 5: Frontend integration

**Files:**
- Modify: `src/lib/api.ts`
- Modify: `src/main.tsx`
- Modify: `src/components/SettingsView.tsx`

**Interfaces:**
- Consumes: Tauri commands `list_crash_reports`,
  `get_crash_report_markdown`, `clear_crash_reports`,
  `report_frontend_crash` (Task 4); `openUrl` from
  `@tauri-apps/plugin-opener` (already a dependency).

- [ ] **Step 1: Add the `CrashSummary` type and API methods**

In `src/lib/api.ts`, add near the other interfaces (after
`SceneSummary`, before `QueueView` — grouping is not load-bearing,
just keep it near related settings/session types):

```ts
export interface CrashSummary {
  id: string;
  kind: "Panic" | "SidecarCrash" | "FrontendError";
  timestamp: number;
  message: string;
}
```

In the `api` object, add these 4 methods near `clearHistory`:

```ts
  listCrashReports: () => call<CrashSummary[]>("list_crash_reports"),
  getCrashReportMarkdown: (id: string) => call<string>("get_crash_report_markdown", { id }),
  clearCrashReports: () => call<void>("clear_crash_reports"),
  reportFrontendCrash: (message: string, stack?: string) =>
    call<void>("report_frontend_crash", { message, stack: stack ?? null }),
```

- [ ] **Step 2: Register global error listeners**

In `src/main.tsx`, add before the `ReactDOM.createRoot(...).render(...)`
call:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { api } from "./lib/api";
import "./styles.css";

window.addEventListener("error", (event) => {
  api.reportFrontendCrash(event.message, event.error?.stack).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message = reason instanceof Error ? reason.message : String(reason);
  const stack = reason instanceof Error ? reason.stack : undefined;
  api.reportFrontendCrash(message, stack).catch(() => {});
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

(`src/main.tsx` currently has no `api` import at all — the
`import { api } from "./lib/api";` line above is new, added alongside
the other imports at the top of the file. `src/components/SettingsView.tsx`
imports the same module as `"../lib/api"` — the different relative
path is expected, `main.tsx` lives one directory up from `components/`.)

- [ ] **Step 3: Add the crash list UI to Settings**

In `src/components/SettingsView.tsx`, this file currently imports only
`{ api }` from `"../lib/api"` (no types from that module yet). Update
that import line and add the `openUrl` import, alongside the other
imports at the top of the file:

```tsx
import { openUrl } from "@tauri-apps/plugin-opener";
import { api, type CrashSummary } from "../lib/api";
```

Add this constant and the two functions above the main
`SettingsView` component (or alongside `UpdatesSection` if that's
where sibling section components already live in this file):

```tsx
const GITHUB_ISSUES_URL = "https://github.com/SidneyRoberto9/echora/issues/new";

function formatRelativeTime(unixMillis: number): string {
  const diffMinutes = Math.round((Date.now() - unixMillis) / 60000);
  if (diffMinutes < 1) return "just now";
  if (diffMinutes < 60) return `${diffMinutes}m ago`;
  const diffHours = Math.round(diffMinutes / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  return `${Math.round(diffHours / 24)}d ago`;
}

function CrashReportsList({ enabled }: { enabled: boolean }) {
  const [reports, setReports] = useState<CrashSummary[]>([]);

  const refresh = () => {
    if (!enabled) {
      setReports([]);
      return;
    }
    api.listCrashReports().then(setReports).catch(() => {});
  };

  useEffect(refresh, [enabled]);

  const handleReport = async (id: string) => {
    const body = await api.getCrashReportMarkdown(id);
    const url = `${GITHUB_ISSUES_URL}?title=${encodeURIComponent(`Crash report: ${id}`)}&body=${encodeURIComponent(body)}&labels=crash-report`;
    await openUrl(url);
  };

  const handleClearAll = async () => {
    await api.clearCrashReports();
    refresh();
  };

  if (!enabled) return null;

  return (
    <div className="crash-reports-list">
      {reports.length === 0 ? (
        <p className="settings-section__hint">No crashes recorded.</p>
      ) : (
        <>
          {reports.map((r) => (
            <div className="settings-row" key={r.id}>
              <span className="settings-row__label">
                {r.kind} — {formatRelativeTime(r.timestamp)}
              </span>
              <button type="button" className="text-link" onClick={() => handleReport(r.id)}>
                Report
              </button>
            </div>
          ))}
          <div className="settings-row">
            <span className="settings-row__label">Clear all crash reports</span>
            <button
              type="button"
              className="text-link"
              style={{ color: "var(--danger)" }}
              onClick={handleClearAll}
            >
              Clear all
            </button>
          </div>
        </>
      )}
    </div>
  );
}
```

Then render it right after the existing crash-reports `Toggle` row (the
one with `on={settings.crash_report_enabled}`), before the closing
`</div>` of that row's parent and before the `privacy-note` div:

```tsx
        <div className="settings-row">
          <span>
            <div className="settings-row__label">Crash reports</div>
            <div className="settings-row__hint">
              Nothing sends automatically — you review and open a GitHub issue yourself
            </div>
          </span>
          <Toggle
            on={settings.crash_report_enabled}
            label="Crash reports"
            onChange={() => update({ crash_report_enabled: !settings.crash_report_enabled })}
          />
        </div>
        <CrashReportsList enabled={settings.crash_report_enabled} />
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>
```

`useState`/`useEffect` are already imported from `react` in this file
(`UpdatesSection` already uses them), so no change needed there.

- [ ] **Step 4: Verify**

Run: `npm run lint && npm run build`
Expected: both pass, zero errors.

- [ ] **Step 5: Manual verification (document what you checked)**

This cannot be automated without a real window — do not claim it works
without actually running it. In the user's own terminal (never launch
`npm run tauri dev` yourself — see project convention), with crash
reports toggled on in Settings:
1. Open devtools and run `throw new Error("test crash")` in the
   console — confirm a "FrontendError" entry appears in the Settings
   crash list after reopening/refreshing Settings.
2. Click "Report" on that entry — confirm the browser opens a GitHub
   "New issue" page with the title and body pre-filled from that crash.
3. Click "Clear all" — confirm the list empties.
4. Toggle crash reports off, repeat step 1 — confirm no new entry
   appears.

- [ ] **Step 6: Commit**

Never run `git commit` directly — stage `src/lib/api.ts`,
`src/main.tsx`, and `src/components/SettingsView.tsx`, then invoke the
`auto-commit` skill. Suggested message:
`feat(crash-report): add crash report list and reporting UI`.
