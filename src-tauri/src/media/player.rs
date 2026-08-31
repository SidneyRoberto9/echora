use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::{Child, Command};

use crate::error::{EchoraError, Result};

/// Wraps the mpv sidecar: spawned as a subprocess (never linked — see
/// docs/adr/0001), audio-only, controlled entirely over its own JSON IPC
/// socket. One connection per command is intentionally simple for Fase 3;
/// a persistent connection with `observe_property` events is a Fase 6/9
/// refinement once smooth progress reporting is actually needed.
pub struct Player {
    mpv_path: PathBuf,
    socket_path: PathBuf,
    child: Option<Child>,
}

impl Player {
    pub fn new(mpv_path: PathBuf, socket_path: PathBuf) -> Self {
        Player {
            mpv_path,
            socket_path,
            child: None,
        }
    }

    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    pub async fn start(&mut self) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);

        let child = Command::new(&self.mpv_path)
            .arg("--idle=yes")
            .arg("--no-video")
            .arg("--no-terminal")
            .arg(format!("--input-ipc-server={}", self.socket_path.display()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(EchoraError::Io)?;
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

    async fn send_command(&self, command: Value) -> Result<Value> {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .map_err(EchoraError::Io)?;
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

    pub async fn load(&self, stream_url: &str) -> Result<()> {
        self.send_command(json!(["loadfile", stream_url])).await?;
        Ok(())
    }

    pub async fn set_paused(&self, paused: bool) -> Result<()> {
        self.send_command(json!(["set_property", "pause", paused]))
            .await?;
        Ok(())
    }

    pub async fn set_volume(&self, volume_percent: u8) -> Result<()> {
        self.send_command(json!(["set_property", "volume", volume_percent]))
            .await?;
        Ok(())
    }

    pub async fn seek_to(&self, seconds: f64) -> Result<()> {
        self.send_command(json!(["seek", seconds, "absolute"]))
            .await?;
        Ok(())
    }

    pub async fn position_seconds(&self) -> Result<Option<f64>> {
        let reply = self
            .send_command(json!(["get_property", "time-pos"]))
            .await?;
        Ok(reply.get("data").and_then(Value::as_f64))
    }

    pub async fn duration_seconds(&self) -> Result<Option<f64>> {
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
        if let Some(mut child) = self.child.take() {
            let _ = child.kill().await;
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

    #[tokio::test]
    #[ignore]
    async fn starting_and_shutting_down_leaves_no_process_or_socket() {
        let socket_path = dev_socket_path();
        let mut player = Player::new(PathBuf::from("mpv"), socket_path.clone());
        player.start().await.unwrap();
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

        let mut player = Player::new(PathBuf::from("mpv"), dev_socket_path());
        player.start().await.unwrap();
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
}
