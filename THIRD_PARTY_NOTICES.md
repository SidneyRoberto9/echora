# Third-Party Notices

Echora's own source is proprietary/source-available (see [LICENSE](./LICENSE)).
It bundles or invokes the third-party components below. This file is a
living document — see `docs/adr/0006-third-party-licensing-and-bundling.md`
for the reasoning behind the bundling strategy. This is not legal advice;
professional legal review is recommended before wide distribution.

Status: **Fase 3 (Media Integration) validated the real sidecar chain in
dev** (search → resolve → mpv playback, live, with actual audio). Exact
pinned versions/checksums for the *shipped* artifact, and the portable
mpv build itself, are finalized in Fase 8 (Packaging) — see
`docs/adr/0007-arm64-native-ci-and-mpv-build.md`.

**Update (2026-09-01) — the three items below are resolved:**
1. mpv's GPL-2.0+ text, yt-dlp's Unlicense + GPLv3+ (`THIRD_PARTY_LICENSES.txt`,
   covering the standalone binary's bundled Readline), Deno's MIT notice, and
   Rust MPL-2.0 dependencies' license text are all embedded directly into
   Echora's compiled binary (`include_str!`, same pattern as
   `docs/adr/0008`'s bundled `moods.json` — see `src-tauri/src/licenses.rs`)
   and reachable in-app via Settings → Third-Party Licenses. Baked in at
   compile time, so it's present regardless of `.deb`/AppImage packaging
   details — no `tauri.conf.json` `resources` entry needed for this part.
2. The `cargo-about`/`cargo-deny` and npm license-checker audit promised
   above has run against the real dependency tree — see the two tables
   below.

**Still open, pre-release blocker:** the mpv **binary itself** (not its
license text) still needs its `.so` runtime dependencies correctly bundled
into the `.deb`/AppImage via `tauri.conf.json`'s `resources` field — see
the "Open item" in `docs/adr/0007-arm64-native-ci-and-mpv-build.md`.

## Bundled sidecar binaries

| Component | Role | License | Distribution form | Obligation |
|---|---|---|---|---|
| **mpv** | Audio-only playback, spawned as a subprocess, controlled via its JSON IPC socket (`--input-ipc-server`). Never linked into Echora's binary. Validated in dev with the distro package (0.37.0); the shipped artifact instead builds mpv from source per architecture in CI (see ADR 0007) — not this distro package. | GPL-2.0-or-later (unmodified upstream build) | Built by Echora's own CI per architecture (x86_64, aarch64), bundled as a relocatable binary + its runtime `.so` dependencies (rpath `$ORIGIN`). | Ship mpv's own license text; do not modify the binary; point to mpv's public source for the exact version built. Safe as "mere aggregation" per the GPL FAQ — see ADR 0001 and 0006. |
| **yt-dlp** | Resolves/searches YouTube media, spawned as a subprocess. Never linked into Echora's binary. Validated in dev at version `2026.08.19` (official `yt-dlp_linux` standalone binary, SHA-256 checksum-verified against the release's `SHA2-256SUMS`). | Source: Unlicense. **Official standalone Linux binary release is GPLv3-or-later as a combined work** (bundles GNU Readline into the frozen CPython interpreter). | Official standalone binaries from yt-dlp's GitHub Releases (`yt-dlp_linux`, `yt-dlp_linux_aarch64`), unmodified, checksum-verified. | Ship yt-dlp's GPLv3 license text and its own `THIRD_PARTY_LICENSES.txt`; do not modify the binary; point to yt-dlp's public source/release for the exact version bundled. |
| **Deno** | JS runtime required by yt-dlp's EJS mechanism (YouTube anti-bot/signature challenges), invoked by yt-dlp itself via `--js-runtimes deno:<path>`. Validated in dev at version `2.9.6` (official release zip). | MIT | Official prebuilt `deno` binary, unmodified, checksum-verified. | Ship Deno's MIT license/copyright notice. |
| **PO-Token provider** (e.g. `bgutil-ytdlp-pot-provider`) | Would supply YouTube PO-Tokens to yt-dlp. **Not currently bundled or needed** — plain Deno via `--js-runtimes` resolved all public-video test cases in Fase 3 without one. Revisit only if resolution failures in the wild indicate YouTube is enforcing PO-Tokens for the videos Echora needs. | N/A — not shipped | N/A | If ever added: confirm its license, and prefer its Deno mode over Node to avoid bundling a second JS runtime. |

## External APIs queried at runtime (not bundled)

| Service | Role | License/Terms | Data handling | Obligation |
|---|---|---|---|---|
| **SponsorBlock API** (`sponsor.ajay.app`) | Fetches community-submitted sponsor/intro/self-promo segment timestamps for the YouTube video ID currently playing, via a plain HTTPS GET using the documented K-Anonymity hash-prefix scheme (no API key). Timestamps are used only to issue mpv `seek` IPC commands during the current playback session, over Echora's existing mpv IPC connection. Nothing from SponsorBlock's own codebase (GPLv3+) is vendored; nothing from its database is bundled, cached beyond the current session, exported, or re-served. | Database & API content: CC BY-NC-SA 4.0 (see [SponsorBlock's Database and API License wiki](https://github.com/ajayyy/SponsorBlock/wiki/Database-and-API-License)). This is a content license, not a code/copyleft license — no linking or build implication for Echora's own source. | Ephemeral, per-video, in-memory only; not persisted past the playback session; not displayed as a raw list, exported, or re-served to any other user/system; no bulk/database download. | Attribution per SponsorBlock's requested template (credit "SponsorBlock" + link, surfaced in Settings/About); use treated as strictly noncommercial and personal, consistent with Echora being free, ad-free, and licensed for noncommercial/personal use (see `LICENSE`); re-review this entry if Echora's distribution model is ever monetized. **Status: RISK (low), accepted without seeking written confirmation from the SponsorBlock maintainer — see `docs/adr/0009-sponsorblock-api-noncommercial-license.md`. Re-review if monetization is ever introduced.** |

## Compile-time dependencies (Rust crates, npm packages)

Not separately redistributed as standalone binaries — compiled/bundled into
the Echora application itself.

**Rust dependency tree**: audited 2026-09-01 via `cargo deny check licenses`
(default-deny config) against the full resolved tree. Every license string
found is permissive (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, 0BSD,
Unlicense, CC0-1.0, MIT-0, CDLA-Permissive-2.0, Unicode-3.0, and their OR
combinations) except the two entries below — no GPL/AGPL anywhere in the
compiled-in Rust tree.

| Component | License | Verdict | Obligation |
|---|---|---|---|
| **attohttpc, cssparser, cssparser-macros, dtoa-short, mpris-server** (Echora's own direct dependency, MPRIS desktop integration), **option-ext, selectors** | MPL-2.0 (plain, no OR) | **SAFE** — unmodified crates.io dependencies, statically compiled into Echora's binary. MPL-2.0 is weak, file-level copyleft: its "Larger Work" clause (§3.3) explicitly permits combining MPL-covered code with proprietary code without forcing the combined binary under MPL — unlike GPL/AGPL, ADR 0001/0006's linking-forces-copyleft finding does not apply here. | License text bundled and reachable via the app's Settings → Third-Party Licenses view (one copy, shared across all seven — see `src-tauri/src/licenses.rs`). Each crate's exact version is public, unmodified source on crates.io/upstream, which already satisfies MPL-2.0 §3.2's source-availability requirement. If any of these seven is ever forked/patched, its modified files must be republished under MPL-2.0 — re-review before that ships. |
| **r-efi** (v5.3.0, v6.0.0) | MIT OR Apache-2.0 OR LGPL-2.1-or-later | **SAFE** — disjunctive multi-license; Echora elects **MIT**, complies solely with MIT's terms. LGPL-2.1-or-later is one of three alternatives offered, not a mandatory term; electing a different offered license carries no LGPL obligation (no dynamic-linking requirement, no source disclosure). | Standard MIT attribution (crate name + copyright notice) only. |

**npm dependency tree**: audited 2026-09-01 via `npx license-checker
--summary` against 145 packages. All permissive: MIT (125), Apache-2.0
(14), ISC (10), BSD-2-Clause (6), Apache-2.0 OR MIT (4), MIT OR Apache-2.0
(3), BSD-3-Clause (2), CC-BY-4.0 (1), BlueOak-1.0.0 (1). The one
"UNLICENSED" entry is `echora@0.1.0` itself (the scanned project, not a
dependency) — not a real finding. No GPL/AGPL/LGPL anywhere in the npm
tree.

## Notes

- No component above is statically linked or FFI-linked into Echora's own
  Rust binary; all are invoked as separate OS processes. See
  `docs/adr/0001-mpv-sidecar-not-libmpv.md` and
  `docs/adr/0006-third-party-licensing-and-bundling.md` for why this
  matters for keeping Echora's own source proprietary.
- If any future dependency cannot be bundled under these conditions, that
  conflict will be documented here and resolved (or the dependency
  dropped) before release — not hidden.
