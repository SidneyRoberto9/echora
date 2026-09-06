use serde::Serialize;

/// One bundled third-party component's license, embedded at compile time
/// (see docs/adr/0008 for the same pattern used for moods.json) — no
/// runtime resource-path resolution needed, unlike mpv's shared libraries
/// which must be real files next to the sidecar binary (see ADR 0007).
#[derive(Debug, Clone, Serialize)]
pub struct LicenseEntry {
    pub component: &'static str,
    pub license: &'static str,
    pub text: &'static str,
}

pub fn all() -> Vec<LicenseEntry> {
    vec![
        LicenseEntry {
            component: "mpv",
            license: "GPL-2.0-or-later",
            text: include_str!("../resources/licenses/mpv-LICENSE.GPL.txt"),
        },
        LicenseEntry {
            component: "yt-dlp",
            license: "Unlicense",
            text: include_str!("../resources/licenses/yt-dlp-LICENSE.txt"),
        },
        LicenseEntry {
            component: "yt-dlp (bundled binary)",
            license: "GPL-3.0-or-later (combined work, via bundled GNU Readline)",
            text: include_str!("../resources/licenses/yt-dlp-THIRD_PARTY_LICENSES.txt"),
        },
        LicenseEntry {
            component: "Deno",
            license: "MIT",
            text: include_str!("../resources/licenses/deno-LICENSE.md"),
        },
        LicenseEntry {
            component: "Rust dependencies (MPL-2.0)",
            license: "MPL-2.0",
            text: include_str!("../resources/licenses/MPL-2.0.txt"),
        },
        LicenseEntry {
            component: "Sora (font)",
            license: "OFL-1.1",
            text: include_str!("../resources/licenses/sora-OFL.txt"),
        },
        // Linked by the mpv sidecar, never by Echora's own binary, and
        // built by our own CI rather than taken from Ubuntu -- which is
        // what makes shipping this text our obligation instead of
        // Canonical's. See scripts/build-ffmpeg.sh; the matching source
        // tarball and configure line ship as release assets.
        LicenseEntry {
            component: "FFmpeg",
            license: "LGPL-2.1-or-later",
            text: include_str!("../resources/licenses/ffmpeg-LICENSE.LGPLv2.1.txt"),
        },
        // TLS backend for FFmpeg's https protocol handler, so mpv can
        // fetch the resolved stream URL itself. Bundled as a runtime
        // dependency of the mpv sidecar.
        LicenseEntry {
            component: "OpenSSL",
            license: "Apache-2.0",
            text: include_str!("../resources/licenses/openssl-LICENSE.Apache-2.0.txt"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_returns_one_entry_per_bundled_component_with_non_empty_text() {
        let entries = all();

        let components: Vec<&str> = entries.iter().map(|e| e.component).collect();
        assert_eq!(
            components,
            vec![
                "mpv",
                "yt-dlp",
                "yt-dlp (bundled binary)",
                "Deno",
                "Rust dependencies (MPL-2.0)",
                "Sora (font)",
                "FFmpeg",
                "OpenSSL",
            ]
        );
        for entry in &entries {
            assert!(
                !entry.text.trim().is_empty(),
                "{} license text should not be empty",
                entry.component
            );
        }
    }

    #[test]
    fn ffmpeg_entry_carries_the_lgpl_not_the_gpl() {
        let entries = all();
        let ffmpeg = entries.iter().find(|e| e.component == "FFmpeg").unwrap();

        assert_eq!(ffmpeg.license, "LGPL-2.1-or-later");
        // The whole point of building without --enable-gpl/--enable-version3:
        // catch a build that silently escalated to a stricter license.
        assert!(ffmpeg.text.contains("GNU LESSER GENERAL PUBLIC LICENSE"));
        assert!(ffmpeg.text.contains("Version 2.1, February 1999"));
    }

    #[test]
    fn mpv_entry_is_gpl_and_yt_dlp_bundled_binary_entry_is_gplv3() {
        let entries = all();

        let mpv = entries.iter().find(|e| e.component == "mpv").unwrap();
        assert_eq!(mpv.license, "GPL-2.0-or-later");

        let bundled = entries
            .iter()
            .find(|e| e.component == "yt-dlp (bundled binary)")
            .unwrap();
        assert!(bundled.text.contains("GNU GENERAL PUBLIC LICENSE"));
        assert!(bundled.text.contains("Version 3"));
    }

    #[test]
    fn sora_font_entry_is_ofl_and_carries_upstream_copyright() {
        let entries = all();

        let sora = entries
            .iter()
            .find(|e| e.component == "Sora (font)")
            .unwrap();
        assert_eq!(sora.license, "OFL-1.1");
        assert!(
            sora.text
                .contains("Copyright 2019 The Sora Project Authors")
        );
        assert!(sora.text.contains("SIL OPEN FONT LICENSE"));
    }
}
