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
}
