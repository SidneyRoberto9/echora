use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{EchoraError, Result};

#[allow(dead_code)]
const MAX_RETAINED: usize = 10;
#[allow(dead_code)]
const MAX_MARKDOWN_BODY_CHARS: usize = 4000;

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CrashKind {
    Panic,
    SidecarCrash,
    FrontendError,
}

#[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct CrashSummary {
    pub id: String,
    pub kind: CrashKind,
    pub timestamp: u64,
    pub message: String,
}

impl CrashRecord {
    #[allow(dead_code)]
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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
    pub fn from_sidecar(process: &str, detail: &str) -> Self {
        CrashRecord::new(
            CrashKind::SidecarCrash,
            format!("{process} sidecar died unexpectedly: {detail}"),
            None,
            None,
        )
    }

    #[allow(dead_code)]
    pub fn from_frontend(message: String, stack: Option<String>) -> Self {
        CrashRecord::new(CrashKind::FrontendError, message, None, stack)
    }
}

#[allow(dead_code)]
fn crashes_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("crashes")
}

#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn record(app_dir: &Path, event: CrashRecord) -> std::io::Result<()> {
    let dir = crashes_dir(app_dir);
    fs::create_dir_all(&dir)?;
    let mut filename = format!("{}-{}.json", event.timestamp, kind_slug(&event.kind));
    let mut path = dir.join(&filename);
    let mut counter = 1;
    // ponytail: linear probe for collision; upgrade to UUID suffix if collisions become common
    while path.exists() {
        filename = format!(
            "{}-{}-{}.json",
            event.timestamp,
            kind_slug(&event.kind),
            counter
        );
        path = dir.join(&filename);
        counter += 1;
    }
    fs::write(path, serde_json::to_vec_pretty(&event)?)?;
    enforce_retention(&dir)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
    summaries.sort_by_key(|s| std::cmp::Reverse(s.timestamp));
    Ok(summaries)
}

#[allow(dead_code)]
fn read_one(app_dir: &Path, id: &str) -> Result<CrashRecord> {
    let dir = crashes_dir(app_dir);
    let mut path = dir.join(format!("{id}.json"));
    // ponytail: if the base name doesn't exist, try collision-suffixed versions
    if !path.exists() {
        let mut counter = 1;
        loop {
            path = dir.join(format!("{id}-{counter}.json"));
            if !path.exists() {
                // Fallback to original path for error reporting
                path = dir.join(format!("{id}.json"));
                break;
            }
            counter += 1;
            if counter > 100 {
                // Safety limit to prevent infinite loops
                path = dir.join(format!("{id}.json"));
                break;
            }
        }
    }
    let bytes = fs::read(path).map_err(EchoraError::Io)?;
    Ok(serde_json::from_slice(&bytes)?)
}

#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
pub fn install_panic_hook(app_dir: PathBuf, enabled: Arc<AtomicBool>) {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if enabled.load(Ordering::Relaxed) {
            let _ = record(&app_dir, CrashRecord::from_panic(info));
        }
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    static PANIC_HOOK_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _guard = PANIC_HOOK_TEST_LOCK.lock().unwrap();
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
        let _guard = PANIC_HOOK_TEST_LOCK.lock().unwrap();
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
