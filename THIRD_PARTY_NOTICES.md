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
`docs/adr/0007-arm64-native-ci-and-mpv-build.md`. Rust and npm
compile-time dependencies (pulled via Cargo/npm, not separately
redistributed as binaries) will be audited with `cargo-about`/`cargo-deny`
and `npm ls`/license-checker before the first release and listed here.

## Bundled sidecar binaries

| Component | Role | License | Distribution form | Obligation |
|---|---|---|---|---|
| **mpv** | Audio-only playback, spawned as a subprocess, controlled via its JSON IPC socket (`--input-ipc-server`). Never linked into Echora's binary. Validated in dev with the distro package (0.37.0); the shipped artifact instead builds mpv from source per architecture in CI (see ADR 0007) — not this distro package. | GPL-2.0-or-later (unmodified upstream build) | Built by Echora's own CI per architecture (x86_64, aarch64), bundled as a relocatable binary + its runtime `.so` dependencies (rpath `$ORIGIN`). | Ship mpv's own license text; do not modify the binary; point to mpv's public source for the exact version built. Safe as "mere aggregation" per the GPL FAQ — see ADR 0001 and 0006. |
| **yt-dlp** | Resolves/searches YouTube media, spawned as a subprocess. Never linked into Echora's binary. Validated in dev at version `2026.08.19` (official `yt-dlp_linux` standalone binary, SHA-256 checksum-verified against the release's `SHA2-256SUMS`). | Source: Unlicense. **Official standalone Linux binary release is GPLv3-or-later as a combined work** (bundles GNU Readline into the frozen CPython interpreter). | Official standalone binaries from yt-dlp's GitHub Releases (`yt-dlp_linux`, `yt-dlp_linux_aarch64`), unmodified, checksum-verified. | Ship yt-dlp's GPLv3 license text and its own `THIRD_PARTY_LICENSES.txt`; do not modify the binary; point to yt-dlp's public source/release for the exact version bundled. |
| **Deno** | JS runtime required by yt-dlp's EJS mechanism (YouTube anti-bot/signature challenges), invoked by yt-dlp itself via `--js-runtimes deno:<path>`. Validated in dev at version `2.9.6` (official release zip). | MIT | Official prebuilt `deno` binary, unmodified, checksum-verified. | Ship Deno's MIT license/copyright notice. |
| **PO-Token provider** (e.g. `bgutil-ytdlp-pot-provider`) | Would supply YouTube PO-Tokens to yt-dlp. **Not currently bundled or needed** — plain Deno via `--js-runtimes` resolved all public-video test cases in Fase 3 without one. Revisit only if resolution failures in the wild indicate YouTube is enforcing PO-Tokens for the videos Echora needs. | N/A — not shipped | N/A | If ever added: confirm its license, and prefer its Deno mode over Node to avoid bundling a second JS runtime. |

## Compile-time dependencies (Rust crates, npm packages)

Not separately redistributed as standalone binaries — compiled/bundled
into the Echora application itself. To be enumerated here via automated
license-audit tooling (`cargo-about` for Rust, an npm license checker for
JS) before the first tagged release, with any copyleft-licensed crate
flagged and resolved before it ships.

## Notes

- No component above is statically linked or FFI-linked into Echora's own
  Rust binary; all are invoked as separate OS processes. See
  `docs/adr/0001-mpv-sidecar-not-libmpv.md` and
  `docs/adr/0006-third-party-licensing-and-bundling.md` for why this
  matters for keeping Echora's own source proprietary.
- If any future dependency cannot be bundled under these conditions, that
  conflict will be documented here and resolved (or the dependency
  dropped) before release — not hidden.
