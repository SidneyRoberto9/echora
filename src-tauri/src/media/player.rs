use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::crash;
use crate::error::{EchoraError, Result};

/// Wraps the mpv sidecar: spawned as a subprocess (never linked — see
/// docs/adr/0001), audio-only, controlled entirely over its own JSON IPC
/// socket. One connection per command is intentionally simple for Fase 3;
/// a persistent connection with `observe_property` events is a Fase 6/9
/// refinement once smooth progress reporting is actually needed.
pub struct Player {
    socket_path: PathBuf,
    child: Option<CommandChild>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
    /// Whether the `astats` metering filter has been successfully attached
    /// for this mpv process yet — attached once, lazily, not per track (see
    /// `media::audio_level::watch`), so later calls to
    /// `enable_level_metering` are a cheap no-op instead of re-adding the
    /// filter every track change.
    level_metering_ready: bool,
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
            level_metering_ready: false,
        }
    }

    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    pub async fn start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);
        self.level_metering_ready = false;

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

    async fn wait_for_socket(&self) -> Result<()> {
        for _ in 0..50 {
            if self.socket_path.exists() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(EchoraError::SidecarTimeout("mpv".into()))
    }

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
        let (read_half, mut write_half) = stream.into_split();

        let payload = json!({ "command": command });
        write_half
            .write_all(format!("{payload}\n").as_bytes())
            .await
            .map_err(EchoraError::Io)?;

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = reader.read_line(&mut line).await.map_err(EchoraError::Io)?;
            if bytes_read == 0 {
                return Err(EchoraError::SidecarTimeout("mpv".into()));
            }
            let value: Value = serde_json::from_str(line.trim())?;
            // mpv multiplexes property-change events on the same socket;
            // skip anything that isn't the reply to the command we sent.
            if value.get("error").is_some() {
                return Ok(value);
            }
        }
    }

    pub async fn load(&mut self, stream_url: &str) -> Result<()> {
        self.send_command(json!(["loadfile", stream_url])).await?;
        Ok(())
    }

    pub async fn set_paused(&mut self, paused: bool) -> Result<()> {
        self.send_command(json!(["set_property", "pause", paused]))
            .await?;
        Ok(())
    }

    pub async fn is_paused(&mut self) -> Result<Option<bool>> {
        let reply = self.send_command(json!(["get_property", "pause"])).await?;
        Ok(reply.get("data").and_then(Value::as_bool))
    }

    pub async fn set_volume(&mut self, volume_percent: u8) -> Result<()> {
        self.send_command(json!(["set_property", "volume", volume_percent]))
            .await?;
        Ok(())
    }

    /// mpv's own volume percentage (0-100+), for callers that need to read
    /// it back rather than only set it — e.g. MPRIS's `Volume` property.
    pub async fn volume_percent(&mut self) -> Result<Option<u8>> {
        let reply = self.send_command(json!(["get_property", "volume"])).await?;
        Ok(reply.get("data").and_then(Value::as_f64).map(|v| v as u8))
    }

    pub async fn seek_to(&mut self, seconds: f64) -> Result<()> {
        self.send_command(json!(["seek", seconds, "absolute"]))
            .await?;
        Ok(())
    }

    pub async fn position_seconds(&mut self) -> Result<Option<f64>> {
        let reply = self
            .send_command(json!(["get_property", "time-pos"]))
            .await?;
        Ok(reply.get("data").and_then(Value::as_f64))
    }

    pub async fn duration_seconds(&mut self) -> Result<Option<f64>> {
        let reply = self
            .send_command(json!(["get_property", "duration"]))
            .await?;
        Ok(reply.get("data").and_then(Value::as_f64))
    }

    /// Attaches the RMS-level metering filter, once per mpv process. Later
    /// calls are a no-op — safe to call unconditionally from every poll
    /// tick in `media::audio_level::watch` rather than tracking "have I
    /// called this yet" at the call site too.
    ///
    /// Verified against a real mpv 0.37.0 process: `af add` with this
    /// exact filter spec succeeds once a file is loaded, and its metadata
    /// (read via `audio_level_db`) updates continuously afterward without
    /// needing to be re-added.
    pub async fn enable_level_metering(&mut self) -> Result<()> {
        if self.level_metering_ready {
            return Ok(());
        }
        let reply = self
            .send_command(json!([
                "af",
                "add",
                "@echora_level:lavfi=[astats=metadata=1:reset=1]"
            ]))
            .await?;
        if reply.get("error").and_then(Value::as_str) == Some("success") {
            self.level_metering_ready = true;
        }
        Ok(())
    }

    /// Current RMS level in dBFS from the `astats` filter `enable_level_metering`
    /// attaches, or `None` if metering isn't enabled yet, the filter hasn't
    /// produced a reading yet, or the reading was non-finite (e.g. true
    /// digital silence reads as `-inf`, which this treats the same as "no
    /// data" rather than propagating an infinite value to the frontend).
    ///
    /// mpv's `af-metadata/<label>` property replies with `data` as a flat
    /// object keyed by strings like `"lavfi.astats.Overall.RMS_level"`,
    /// with **string-typed** values (e.g. `"-21.123621"`) — confirmed
    /// against a real mpv process, not assumed from FFmpeg's docs alone.
    pub async fn audio_level_db(&mut self) -> Result<Option<f64>> {
        if !self.level_metering_ready {
            return Ok(None);
        }
        let reply = self
            .send_command(json!(["get_property", "af-metadata/echora_level"]))
            .await?;
        Ok(reply
            .get("data")
            .and_then(|d| d.get("lavfi.astats.Overall.RMS_level"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite()))
    }

    /// Ends the mpv process cleanly (asks it to quit, then kills it if it
    /// doesn't). Must be called on app shutdown and on Player drop — never
    /// leave an orphaned mpv process behind.
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
}

/// Real mpv-process smoke tests. Not run by default — `cargo test --
/// --ignored` with `mpv` on PATH (see docs/adr/0007: the CI-built portable
/// mpv ships in the real package; local dev uses the system package).
#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn dev_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("echora-player-test-{}.sock", std::process::id()))
    }

    /// A real (if fake-runtime) `AppHandle` with the shell plugin
    /// registered — `app.shell().sidecar(...)` looks up `Shell<R>` from
    /// managed state, so a bare `MockRuntime` app without the plugin
    /// registered would fail at `.sidecar()`, not just at the (expected,
    /// per tauri-apps/tauri#13767) real spawn.
    fn test_app_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_shell::init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock tauri app should build")
            .handle()
            .clone()
    }

    #[tokio::test]
    #[ignore]
    async fn starting_and_shutting_down_leaves_no_process_or_socket() {
        let socket_path = dev_socket_path();
        let app = test_app_handle();
        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        assert!(socket_path.exists());

        player.shutdown().await.unwrap();
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    #[ignore]
    async fn loading_a_real_stream_reports_matching_duration() {
        // Resolve a real track first (requires the yt-dlp/Deno dev
        // binaries — see resolver.rs's smoke tests), then hand its stream
        // URL to mpv and confirm mpv's own reported duration lines up with
        // what yt-dlp reported: proof the whole sidecar chain actually
        // plays real audio end to end, not just that each piece runs.
        //
        // Deliberately searches for a normal video, not a livestream — a
        // livestream's DASH manifest behaves differently in mpv and won't
        // report a stable time-pos/duration the way a VOD does.
        use super::super::resolver::{Resolver, ResolverConfig};
        use std::time::Duration as StdDuration;

        let dev_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        let resolver = Resolver::new(ResolverConfig {
            deno_path: dev_dir.join("deno"),
            timeout: StdDuration::from_secs(30),
        });
        let app = test_app_handle();
        let tracks = resolver
            .search(&app, "villain arc playlist", 1)
            .await
            .unwrap();
        let resolved = resolver
            .resolve_with_retry(&app, &tracks[0].id)
            .await
            .unwrap();
        let mut player = Player::new(
            dev_socket_path(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player.set_volume(10).await.unwrap();
        player.load(&resolved.stream_url).await.unwrap();
        tokio::time::sleep(StdDuration::from_secs(4)).await;

        let position = player.position_seconds().await.unwrap();
        assert!(position.unwrap_or(0.0) > 0.0, "playback did not advance");

        let duration = player.duration_seconds().await.unwrap();
        let expected = tracks[0].duration_seconds.unwrap() as f64;
        assert!(
            (duration.unwrap_or(0.0) - expected).abs() < 5.0,
            "mpv duration {duration:?} does not match yt-dlp's reported {expected}"
        );

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn mpv_dying_unexpectedly_is_recorded_as_a_sidecar_crash() {
        // Fully-qualified `Arc`/`AtomicBool`/`Duration` here rather than
        // relying on `use super::*` to have brought them in — matches
        // this file's existing `StdDuration` alias in the test above,
        // which does the same for the same reason.
        let socket_path = dev_socket_path();
        let app_dir =
            std::env::temp_dir().join(format!("echora-crash-smoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&app_dir);
        let enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let app = test_app_handle();
        let mut player = Player::new(socket_path.clone(), app_dir.clone(), enabled);
        player.start(&app).await.unwrap();

        // Kill mpv out-of-band — not through shutdown() — to simulate a
        // real unexpected sidecar death.
        let pid = player.child.as_ref().unwrap().pid();
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let err = player.set_volume(50).await.unwrap_err();

        assert!(matches!(err, crate::error::EchoraError::Io(_)));
        let summaries = crate::crash::list(&app_dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(matches!(
            summaries[0].kind,
            crate::crash::CrashKind::SidecarCrash
        ));
    }

    #[tokio::test]
    #[ignore]
    async fn enable_level_metering_is_a_noop_the_second_time() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();

        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        // Second call must not error even though the filter is already
        // attached — this is what makes it safe to call unconditionally
        // from audio_level::watch every tick before reading a level.
        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn audio_level_db_returns_none_before_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-none-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();

        assert_eq!(player.audio_level_db().await.unwrap(), None);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn audio_level_db_reads_a_real_rms_level_once_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-real-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();

        // Give mpv a moment to actually start decoding/filtering audio —
        // mirrors the existing smoke tests' pattern of a short sleep after
        // a state-changing IPC call before reading it back.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let level = player.audio_level_db().await.unwrap();
        assert!(level.is_some());
        assert!(level.unwrap().is_finite());

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn level_metering_flag_resets_on_process_restart() {
        let socket_path = dev_socket_path();
        let app = test_app_handle();
        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        // First start: enable metering
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        // Simulate crash by killing the process
        let pid = player.child.as_ref().unwrap().pid();
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Trigger error to clear child state
        let _ = player.set_volume(50).await;

        // Restart: flag should be reset to false
        player.start(&app).await.unwrap();
        assert!(
            !player.level_metering_ready,
            "level_metering_ready must reset on start()"
        );

        // Load and re-enable metering on the new process
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();
        assert!(
            player.level_metering_ready,
            "enable_level_metering must work on new process"
        );

        player.shutdown().await.unwrap();
    }
}
