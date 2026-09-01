use std::path::PathBuf;

/// Where to find the yt-dlp/Deno/mpv binaries.
///
/// **Dev-only for now.** yt-dlp and Deno are read from
/// `src-tauri/binaries/dev/` (downloaded manually per the project's dev
/// setup, checksummed against upstream releases); mpv falls back to
/// whatever `mpv` resolves to on `PATH` (the system package during
/// development). Fase 8 replaces all of this with Tauri's real sidecar
/// resource mechanism (`externalBin`), building a portable mpv in CI per
/// docs/adr/0007 instead of relying on `PATH` or the build directory.
pub struct SidecarPaths {
    pub yt_dlp: PathBuf,
    pub deno: PathBuf,
}

impl SidecarPaths {
    pub fn discover_dev() -> Self {
        let dev_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        SidecarPaths {
            yt_dlp: dev_dir.join("yt-dlp_linux"),
            deno: dev_dir.join("deno"),
        }
    }
}
