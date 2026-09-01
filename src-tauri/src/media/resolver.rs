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

/// Wraps the yt-dlp sidecar. Rust owns the process: structured output only
/// (`--dump-json`), a hard timeout per call, arguments always passed as an
/// argument array (never a shell string), and the child is killed if that
/// timeout fires (`CommandChild` has no kill-on-drop, so this is explicit).
pub struct Resolver {
    config: ResolverConfig,
}

impl Resolver {
    pub fn new(config: ResolverConfig) -> Self {
        Resolver { config }
    }

    /// Fast, metadata-light search — used to build mood candidates. Not
    /// full resolution: no playable stream URL yet (see `resolve`).
    pub async fn search<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        query: &str,
        limit: u32,
    ) -> Result<Vec<Track>> {
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

    /// Resolves one track to a playable direct audio stream URL. Retries
    /// once on a transient failure (timeout/IO) — never on a classified
    /// permanent unavailability (private/removed/region-blocked), since
    /// retrying those just wastes time before the queue moves on.
    pub async fn resolve_with_retry<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        track_id: &str,
    ) -> Result<ResolvedStream> {
        match self.resolve(app, track_id).await {
            Err(EchoraError::SidecarTimeout(_) | EchoraError::Io(_)) => {
                self.resolve(app, track_id).await
            }
            other => other,
        }
    }

    async fn resolve<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        track_id: &str,
    ) -> Result<ResolvedStream> {
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

    async fn run<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        args: Vec<String>,
    ) -> Result<String> {
        let (mut rx, child) = app
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

        let (stdout, stderr, exit_success) = match timeout(self.config.timeout, collect).await {
            Ok(result) => result,
            Err(_) => {
                // `CommandChild` has no kill-on-drop -- dropping it here would
                // leak yt-dlp (and the Deno process it spawns) as an orphan.
                let _ = child.kill();
                return Err(EchoraError::SidecarTimeout("yt-dlp".into()));
            }
        };

        if exit_success {
            Ok(String::from_utf8_lossy(&stdout).into_owned())
        } else {
            Err(metadata::classify_ytdlp_failure(&String::from_utf8_lossy(
                &stderr,
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
            deno_path: dev_dir.join("deno"),
            timeout: Duration::from_secs(30),
        }
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
    async fn searching_a_real_mood_query_returns_tracks() {
        let app = test_app_handle();
        let resolver = Resolver::new(dev_config());
        let tracks = resolver
            .search(&app, "villain arc playlist", 3)
            .await
            .unwrap();
        assert!(!tracks.is_empty());
        assert!(!tracks[0].id.is_empty());
    }

    #[tokio::test]
    #[ignore]
    async fn resolving_a_real_track_returns_a_playable_url() {
        let app = test_app_handle();
        let resolver = Resolver::new(dev_config());
        let tracks = resolver
            .search(&app, "villain arc playlist", 1)
            .await
            .unwrap();
        let resolved = resolver
            .resolve_with_retry(&app, &tracks[0].id)
            .await
            .unwrap();
        assert_eq!(resolved.track_id, tracks[0].id);
        assert!(resolved.stream_url.starts_with("https://"));
    }

    #[tokio::test]
    #[ignore]
    async fn resolving_a_private_or_removed_video_is_a_track_unavailable_not_a_crash() {
        let app = test_app_handle();
        let resolver = Resolver::new(dev_config());
        // A withdrawn/likely-private video id; if YouTube ever recycles this
        // id this test may need a fresher one — that's fine, it's a manual
        // smoke test, not part of the deterministic suite.
        let err = resolver
            .resolve_with_retry(&app, "aaaaaaaaaaa")
            .await
            .unwrap_err();
        assert!(matches!(err, EchoraError::TrackUnavailable(_)));
    }
}
