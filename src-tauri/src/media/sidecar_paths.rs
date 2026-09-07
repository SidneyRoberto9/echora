use std::path::PathBuf;

/// The `externalBin` entry name for Deno. Prefixed so the `.deb` — which
/// installs every sidecar into `/usr/bin/` alongside the main binary —
/// cannot collide with the distro's own `deno`, `mpv` or `yt-dlp`
/// packages. An unprefixed name makes `apt install` abort outright:
/// "a tentar sobre-escrever '/usr/bin/yt-dlp', que também está no pacote
/// yt-dlp".
const DENO_BIN: &str = "echora-deno";

/// Deno's resolved path for yt-dlp's `--js-runtimes` argument — yt-dlp
/// spawns Deno itself as its own child process, so Echora needs Deno's
/// real filesystem path (not just the ability to spawn it), unlike mpv
/// and yt-dlp which Echora spawns directly by sidecar name.
///
/// No documented Tauri 2.x API resolves an `externalBin` entry to a
/// bare path (only `app.shell().sidecar(name)`, which spawns rather
/// than resolves), so this reasons from each package format's own
/// layout. Verified against real built packages: **both bundlers strip
/// the target triple**, so `deno-x86_64-unknown-linux-gnu` in
/// `src-tauri/binaries/` ships as plain `usr/bin/echora-deno` in the
/// `.deb` and in the AppImage's AppDir. Only dev mode keeps the suffix.
///
/// Getting this wrong is silent: yt-dlp falls back to no JS runtime and
/// only warns ("No supported JavaScript runtime could be found ... some
/// formats may be missing"), degrading extraction instead of failing.
pub fn resolve_deno_path() -> PathBuf {
    if let Ok(appdir) = std::env::var("APPDIR") {
        // Running from a mounted AppImage — Deno sits at the same
        // level Tauri places every externalBin inside the AppDir.
        return PathBuf::from(appdir).join("usr/bin").join(DENO_BIN);
    }

    if let Ok(exe) = std::env::current_exe()
        && exe.starts_with("/usr/")
    {
        // A .deb install places externalBin binaries alongside the
        // main executable, both under /usr/bin/.
        if let Some(dir) = exe.parent() {
            return dir.join(DENO_BIN);
        }
    }

    // Dev mode (cargo run / cargo tauri dev, not a bundled build): here
    // Tauri still expects the target-triple suffix on disk.
    let triple = format!("{}-unknown-linux-gnu", std::env::consts::ARCH);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("{DENO_BIN}-{triple}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both cases live in one test on purpose: they share the `APPDIR`
    /// env var, and `cargo test` runs test fns on parallel threads.
    #[test]
    fn resolves_bundled_without_triple_and_dev_with_it() {
        // SAFETY: this test owns APPDIR — no other test in this crate
        // reads or writes it, so the mutation cannot race.
        unsafe {
            std::env::set_var("APPDIR", "/mnt/appimage");
        }
        let bundled = resolve_deno_path();
        assert_eq!(bundled, PathBuf::from("/mnt/appimage/usr/bin/echora-deno"));

        unsafe {
            std::env::remove_var("APPDIR");
        }
        let dev = resolve_deno_path();
        assert!(dev.starts_with(env!("CARGO_MANIFEST_DIR")));
        assert!(
            dev.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("echora-deno-")),
            "dev path keeps the triple suffix: {dev:?}"
        );
    }
}
