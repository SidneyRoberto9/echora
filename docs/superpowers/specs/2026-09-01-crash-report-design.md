# Crash report — Design

Status: Approved. Fourth of the post-v1-audit features (SponsorBlock →
Auto-update → Crash report → Packaging), per the user's priority order.

## Purpose

Implement the crash-report requirement from `docs/REQUIREMENTS_FREEZE.md`:
"a fully manual, opt-in crash report — a local log plus a button that
opens a pre-filled GitHub issue in the user's browser. No automatic
network call, no third-party SDK." Today only a cosmetic Settings toggle
(`crash_report_enabled`, default `false`) exists — no capture, no log,
no button logic behind it. This task builds all of it.

## Scope decisions

- **What counts as a crash**: Rust panics (backend), an mpv sidecar
  dying unexpectedly, and unhandled JS errors/promise rejections
  (frontend). yt-dlp resolve failures do **not** count — they already
  surface through the normal `EchoraError` → UI error path and are
  routine, not exceptional.
- **Toggle semantics**: `crash_report_enabled` gates capture itself, not
  just the report button. Off = nothing is ever written to disk. On =
  all three capture points write to `crashes/` and the report UI
  appears.
- **Storage**: one JSON file per crash event in
  `<app_data_dir>/crashes/`, capped at the 10 most recent — writing an
  11th deletes the oldest.
- **Content**: crash kind, message, timestamp, app version, OS/arch,
  and (for panics) a captured backtrace. No user data — no search
  history, no played URLs, no listening history.
- **Report UI**: Settings lists every stored crash (kind + relative
  time), each with its own "Report" button that opens a pre-filled
  GitHub issue for that one event, plus a "Clear all" action mirroring
  the existing History section's pattern.

## Non-goals

- No automatic crash recovery or mpv auto-restart — detection and
  logging only.
- No telemetry, no automatic network call. Opening the browser to a
  pre-filled GitHub issue is a direct result of the user clicking
  "Report" — same posture as Auto-update's manual "Check for Updates".
- No general-purpose application log (every IPC call, etc.) — out of
  scope; would need a new logging crate and wasn't asked for. Only
  crash events are captured.
- No watcher task / `Child` ownership refactor for mpv. Detection is
  reactive (see below), not a background poller — avoids a real
  deadlock risk (a watcher holding the child's lock across `.wait()`
  would block `shutdown()`'s `.kill()`) for a case that only needs to
  be caught on the next interaction, not instantly.

## Backend (Rust)

No new crates. Uses `std::fs`, `std::backtrace::Backtrace` (stable since
1.65), and `serde_json`, all already available.

### `src-tauri/src/crash.rs` (new module)

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

const MAX_RETAINED: usize = 10;
const MAX_MARKDOWN_BODY_CHARS: usize = 4000;

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub timestamp: u64, // unix millis — filename uses this directly, so
                         // second-resolution would let two fast crashes
                         // of the same kind collide and overwrite
    pub app_version: String,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CrashSummary {
    pub id: String, // filename stem, e.g. "1798400000000-panic"
    pub kind: CrashKind,
    pub timestamp: u64,
    pub message: String,
}

impl CrashRecord {
    fn new(kind: CrashKind, message: String, location: Option<String>, backtrace: Option<String>) -> Self {
        CrashRecord {
            kind,
            message,
            location,
            backtrace,
            timestamp: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0),
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
/// or bubble up as a user-facing error, so callers discard the `Result`.
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
    entries.sort(); // filenames are timestamp-prefixed, so lexicographic == chronological
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
            Some(CrashSummary { id, kind: record.kind, timestamp: record.timestamp, message: record.message })
        })
        .collect();
    summaries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp)); // newest first
    Ok(summaries)
}

fn read_one(app_dir: &Path, id: &str) -> crate::error::Result<CrashRecord> {
    let path = crashes_dir(app_dir).join(format!("{id}.json"));
    let bytes = fs::read(path).map_err(crate::error::EchoraError::Io)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn to_markdown(app_dir: &Path, id: &str) -> crate::error::Result<String> {
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
        // char-boundary-safe: byte-index String::truncate can panic mid-UTF8
        // sequence (a backtrace can contain non-ASCII paths).
        let bt: String = bt.chars().take(MAX_MARKDOWN_BODY_CHARS).collect();
        body.push_str(&format!("\n**Backtrace**{}:\n```\n{}\n```\n", if truncated { " (truncated)" } else { "" }, bt));
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
```

### `src-tauri/src/state.rs`

`AppState` gains two fields, both cheap to clone into closures/other
structs that need them:

```rust
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;

pub struct AppState {
    // ...existing fields unchanged...
    pub app_dir: PathBuf,
    pub crash_reporting_enabled: Arc<AtomicBool>,
}
```

### `src-tauri/src/lib.rs`

Read settings once, before `app.manage(...)`, instead of the current
post-manage-only read — needed because the initial atomic flag value
has to exist before `AppState` is constructed. This also lets the
existing autostart-sync call below reuse the same read instead of
issuing a second `get_settings()` query:

```rust
            let moods = moods::MoodCatalog::load().expect("bundled moods.json should load");
            let initial_settings = db.get_settings()?;
            let crash_reporting_enabled = std::sync::Arc::new(
                std::sync::atomic::AtomicBool::new(initial_settings.crash_report_enabled),
            );

            // ...sidecar_paths / resolver setup unchanged...

            let player = media::player::Player::new(
                sidecar_paths.mpv,
                app_dir.join("mpv-ipc.sock"),
                app_dir.clone(),
                crash_reporting_enabled.clone(),
            );

            // ...mpris build unchanged...

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
            platform::autostart::sync(app.handle(), initial_settings.autostart_enabled)?;

            // Chain onto Rust's default panic hook (keeps stderr output for
            // `cargo tauri dev`) and additionally persist a crash record —
            // best-effort, must never itself panic.
            let default_hook = std::panic::take_hook();
            let panic_app_dir = app_dir.clone();
            let panic_flag = crash_reporting_enabled.clone();
            std::panic::set_hook(Box::new(move |info| {
                if panic_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    let _ = crash::record(&panic_app_dir, crash::CrashRecord::from_panic(info));
                }
                default_hook(info);
            }));

            tauri::async_runtime::spawn(media::sponsorblock::watch(app.handle().clone()));

            Ok(())
```

Add `mod crash;` to the top-level `mod` list, and register the four new
commands (see below) in `invoke_handler(tauri::generate_handler![...])`.

### `src-tauri/src/media/player.rs`

`Player` gains two constructor params and a reactive crash check inside
`send_command`:

```rust
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
        Player { mpv_path, socket_path, child: None, app_dir, crash_reporting_enabled }
    }

    // ...start(), wait_for_socket() unchanged...

    async fn send_command(&mut self, command: Value) -> Result<Value> {
        let stream = match UnixStream::connect(&self.socket_path).await {
            Ok(s) => s,
            Err(err) => {
                if self.child.is_some() {
                    // mpv is supposed to be running but its socket is gone —
                    // it died outside our own shutdown() path.
                    if self.crash_reporting_enabled.load(Ordering::Relaxed) {
                        let _ = crash::record(&self.app_dir, crash::CrashRecord::from_sidecar("mpv", &err.to_string()));
                    }
                    self.child = None; // it's actually gone; stop treating it as started
                }
                return Err(EchoraError::Io(err));
            }
        };
        // ...rest unchanged...
    }
}
```

Note: `send_command` currently takes `&self`; this changes it to `&mut
self` so it can clear `self.child` on detected crash. That ripples to
every method that forwards into it — `load`, `set_paused`, `is_paused`,
`set_volume`, `volume_percent`, `seek_to`, `position_seconds`,
`duration_seconds` — all become `&mut self` too. Chained call sites
(`state.player.lock().await.set_paused(true).await?` with no named
binding) keep compiling unchanged — a temporary's mutability doesn't
depend on a `let mut`. But 5 call sites bind the guard to a name
*without* `mut` and then call one of the now-`&mut self` methods on it,
which does need the binding itself declared `mut`:
`commands/mod.rs:61` (`toggle_play_pause`), `media/sponsorblock.rs:182`,
and `platform/mpris.rs:94`, `256`, `336`. Each just needs `let player =`
changed to `let mut player =`, no other logic changes. The two
`#[cfg(test)] mod smoke_tests` constructors (`Player::new(PathBuf::from("mpv"),
socket_path.clone())` and the one in the real-playback test) also need
the two new `app_dir`/`crash_reporting_enabled` args added, or they
won't compile even though they're `#[ignore]`d.

### `src-tauri/src/commands/crash.rs` (new)

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
pub fn report_frontend_crash(state: State<AppState>, message: String, stack: Option<String>) -> Result<()> {
    if state.crash_reporting_enabled.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = crash::record(&state.app_dir, crash::CrashRecord::from_frontend(message, stack));
    }
    Ok(())
}
```

Register `pub mod crash;` in `src-tauri/src/commands/mod.rs`.

### `src-tauri/src/commands/settings.rs`

`update_settings` also keeps the atomic flag truthful, same shape as
the existing `autostart::sync` call:

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

No `capabilities/default.json` change — these are plain `#[tauri::command]`
app commands, not plugin-provided ones, so they need no permission
entry (matches every other existing app command).

## Frontend (React)

### `src/lib/api.ts`

```ts
export interface CrashSummary {
  id: string;
  kind: "Panic" | "SidecarCrash" | "FrontendError";
  timestamp: number;
  message: string;
}
```

```ts
  listCrashReports: () => call<CrashSummary[]>("list_crash_reports"),
  getCrashReportMarkdown: (id: string) => call<string>("get_crash_report_markdown", { id }),
  clearCrashReports: () => call<void>("clear_crash_reports"),
  reportFrontendCrash: (message: string, stack?: string) =>
    call<void>("report_frontend_crash", { message, stack: stack ?? null }),
```

### `src/main.tsx`

Register global handlers before rendering, so they're active for the
whole app lifetime. Fire-and-forget — a failure to report a crash must
never throw a second error:

```tsx
window.addEventListener("error", (event) => {
  api.reportFrontendCrash(event.message, event.error?.stack).catch(() => {});
});
window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason;
  const message = reason instanceof Error ? reason.message : String(reason);
  const stack = reason instanceof Error ? reason.stack : undefined;
  api.reportFrontendCrash(message, stack).catch(() => {});
});
```

(Backend decides whether to actually persist, based on the toggle — the
frontend always calls, matching "Rust is the source of truth.")

### `src/components/SettingsView.tsx`

Extend the existing "Privacy" section's crash-reports row with a list
+ report/clear UI. New small component in the same file (same rationale
as `UpdatesSection` — not reused elsewhere):

```tsx
import { openUrl } from "@tauri-apps/plugin-opener";

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
    if (!enabled) { setReports([]); return; }
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
            <button type="button" className="text-link" style={{ color: "var(--danger)" }} onClick={handleClearAll}>
              Clear all
            </button>
          </div>
        </>
      )}
    </div>
  );
}
```

Rendered right after the existing crash-reports `Toggle` row:

```tsx
        <div className="settings-row">
          {/* ...existing Toggle row unchanged... */}
        </div>
        <CrashReportsList enabled={settings.crash_report_enabled} />
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>
```

No new CSS — reuses `settings-row`, `settings-row__label`,
`settings-section__hint`, `text-link`, same as every other Settings
list in this file.

## Error handling / edge cases

- Panic hook write failure (e.g. disk full, permissions) — `let _ =`
  discards the `Result`; the default hook still runs after, so the
  panic itself is never suppressed or altered.
- `send_command`'s crash detection only fires when `self.child.is_some()`
  — a connect failure before `start()` was ever called (nothing to
  detect) or after a clean `shutdown()` (which already cleared `child`
  and removed the socket) never records a false crash.
- `get_crash_report_markdown` / `clear_crash_reports` racing a
  retention trim mid-list — `list()` reads whatever files exist at call
  time; a file disappearing between `list` and a subsequent `Report`
  click surfaces as a normal `Io` error the existing error-toast path
  already handles, no special case needed.
- Long backtraces — truncated to `MAX_MARKDOWN_BODY_CHARS` (4000) with
  a "(truncated)" marker before URL-encoding, keeping the constructed
  GitHub URL well under practical browser/URL length limits.
- Toggle flipped off while reports already exist on disk — existing
  files are **not** deleted automatically (only "Clear all" or the
  10-item retention cap removes them); the list UI just stops
  rendering/fetching (`enabled` false → `CrashReportsList` returns
  `null`), matching "toggle gates capture and the button," not "toggle
  deletes history."

## Testing

- Rust unit tests (`src-tauri/src/crash.rs`): retention caps at 10 and
  evicts oldest-first; `to_markdown` truncates and marks truncation
  correctly; `list` returns newest-first and tolerates a missing
  `crashes/` dir (empty `Vec`, not an error); a record written then
  read round-trips.
- Rust unit test (`commands/crash.rs` or inline): `report_frontend_crash`
  is a no-op (no file created) when the flag is `false` — exercises the
  "off means zero I/O" requirement directly.
- Rust: extend `player.rs`'s existing `#[ignore]` real-mpv smoke test
  group with one that starts a real mpv process, kills it out-of-band
  (not via `shutdown()`), then asserts the next `send_command` call
  both returns an `Io` error and leaves a `sidecar` crash file behind —
  same "real process, not a diff-only assumption" discipline the
  session already applied twice this cycle (SponsorBlock's TLS
  regression, Auto-update's bundler regression).
- Frontend: no test framework change — `npm run lint && npm run build`
  covers `CrashReportsList` and the `main.tsx` listeners type-check and
  build. Manual verification (documented, not automated) for whoever
  next runs `npm run tauri dev`: toggle crash reports on, trigger a
  frontend error from the devtools console, confirm it appears in the
  Settings list and "Report" opens a correctly pre-filled GitHub issue
  in the browser.
