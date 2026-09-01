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
