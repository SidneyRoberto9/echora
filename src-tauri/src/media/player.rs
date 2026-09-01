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
            yt_dlp_path: dev_dir.join("yt-dlp_linux"),
            deno_path: dev_dir.join("deno"),
            timeout: StdDuration::from_secs(30),
        });
        let tracks = resolver.search("villain arc playlist", 1).await.unwrap();
        let resolved = resolver.resolve_with_retry(&tracks[0].id).await.unwrap();

        let app = test_app_handle();
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
}
