use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tokio::time::timeout;

use super::metadata;
use crate::error::{EchoraError, Result};
use crate::models::{ResolvedStream, Track};

#[derive(Debug, Clone)]
pub struct ResolverConfig {
    pub yt_dlp_path: PathBuf,
    pub deno_path: PathBuf,
    pub timeout: Duration,
}

/// Wraps the yt-dlp sidecar. Rust owns the process: structured output only
/// (`--dump-json`), a hard timeout per call, arguments always passed as an
/// argument array (never a shell string), and the child is killed if the
/// calling future is dropped/cancelled.
pub struct Resolver {
    config: ResolverConfig,
}

impl Resolver {
    pub fn new(config: ResolverConfig) -> Self {
        Resolver { config }
    }

    /// Fast, metadata-light search — used to build mood candidates. Not
    /// full resolution: no playable stream URL yet (see `resolve`).
    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<Track>> {
        let search_spec = format!("ytsearch{limit}:{query}");
        let stdout = self
            .run(vec![
                "--js-runtimes".into(),
                format!("deno:{}", self.config.deno_path.display()),
                search_spec,
                "--flat-playlist".into(),
                "--dump-json".into(),
                "--no-warnings".into(),
            ])
            .await?;

        stdout
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(metadata::parse_search_result)
            .collect()
    }

    /// Resolves one track to a playable direct audio stream URL. Retries
    /// once on a transient failure (timeout/IO) — never on a classified
    /// permanent unavailability (private/removed/region-blocked), since
    /// retrying those just wastes time before the queue moves on.
    pub async fn resolve_with_retry(&self, track_id: &str) -> Result<ResolvedStream> {
        match self.resolve(track_id).await {
            Err(EchoraError::SidecarTimeout(_) | EchoraError::Io(_)) => {
                self.resolve(track_id).await
            }
            other => other,
        }
    }

    async fn resolve(&self, track_id: &str) -> Result<ResolvedStream> {
        let url = format!("https://www.youtube.com/watch?v={track_id}");
        let stdout = self
            .run(vec![
                "--js-runtimes".into(),
                format!("deno:{}", self.config.deno_path.display()),
                url,
                "-f".into(),
                "bestaudio".into(),
                "--dump-json".into(),
                "--no-warnings".into(),
                "--no-playlist".into(),
            ])
            .await?;
        metadata::parse_resolved(&stdout, track_id)
    }

    async fn run(&self, args: Vec<String>) -> Result<String> {
        let mut cmd = Command::new(&self.config.yt_dlp_path);
        cmd.args(&args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child = cmd.spawn().map_err(EchoraError::Io)?;
        let output = timeout(self.config.timeout, child.wait_with_output())
            .await
            .map_err(|_| EchoraError::SidecarTimeout("yt-dlp".into()))?
            .map_err(EchoraError::Io)?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(metadata::classify_ytdlp_failure(&String::from_utf8_lossy(
                &output.stderr,
            )))
        }
    }
}

/// Real, network- and binary-dependent smoke tests. Not run by default —
/// `cargo test -- --ignored` after placing yt-dlp/deno at the dev paths
/// below (see src-tauri/binaries/dev/). Deterministic parsing behavior is
/// covered without any of this in `metadata.rs`.
#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn dev_config() -> ResolverConfig {
        let dev_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        ResolverConfig {
            yt_dlp_path: dev_dir.join("yt-dlp_linux"),
            deno_path: dev_dir.join("deno"),
            timeout: Duration::from_secs(30),
        }
    }

    #[tokio::test]
    #[ignore]
    async fn searching_a_real_mood_query_returns_tracks() {
        let resolver = Resolver::new(dev_config());
        let tracks = resolver.search("villain arc playlist", 3).await.unwrap();
        assert!(!tracks.is_empty());
        assert!(!tracks[0].id.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn resolving_a_real_track_returns_a_playable_url() {
        let resolver = Resolver::new(dev_config());
        let tracks = resolver.search("villain arc playlist", 1).await.unwrap();
        let resolved = resolver.resolve_with_retry(&tracks[0].id).await.unwrap();
        assert_eq!(resolved.track_id, tracks[0].id);
        assert!(resolved.stream_url.starts_with("https://"));
    }

    #[tokio::test]
    #[ignore]
    async fn resolving_a_private_or_removed_video_is_a_track_unavailable_not_a_crash() {
        let resolver = Resolver::new(dev_config());
        // A withdrawn/likely-private video id; if YouTube ever recycles this
        // id this test may need a fresher one — that's fine, it's a manual
        // smoke test, not part of the deterministic suite.
        let err = resolver
            .resolve_with_retry("aaaaaaaaaaa")
            .await
            .unwrap_err();
        assert!(matches!(err, EchoraError::TrackUnavailable(_)));
    }
}
