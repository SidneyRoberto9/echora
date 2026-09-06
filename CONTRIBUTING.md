# Contributing to Echora

Echora is source-available, not open source (see [LICENSE](./LICENSE)). You
are welcome to read the code, open issues, suggest improvements, and submit
pull requests to this repository. The license does not grant a general
right to reuse, redistribute, or build other products from this code —
only to contribute back to this project.

## Before you start

- For anything non-trivial, open an issue first describing the problem or
  idea. It avoids duplicated work and lets us agree on direction before
  you write code.
- Echora's #1 priority is being extremely lightweight (RAM, CPU, process
  count, startup time, binary size, before convenience). A contribution
  that trades meaningful weight for convenience will likely be asked to
  change direction.
- No Electron, no embedding the YouTube web UI, no heavy dependency added
  "just in case." See `CLAUDE.md` and `docs/adr/` for the standing
  architectural decisions.

## Making a contribution (fork workflow)

1. Fork the repository.
2. Create a branch for your change.
3. Make your change. Keep the fork private-purpose: it exists only to
   prepare this contribution (see the License Addendum, section 2) — it
   isn't a place to publish or maintain an independent copy of Echora.
4. Run the checks that apply to what you touched before opening the PR
   (see below).
5. Open a pull request against `main` describing what changed and why.
6. Once the PR is merged (or rejected), delete or make your fork private.

## Checks to run before opening a PR

The Rust commands below need placeholder sidecar binaries first. Tauri's
build script checks, on **every** `cargo build`/`clippy`/`test` (not just
packaging), that every `externalBin` and `resources` glob in
`src-tauri/tauri.conf.json` resolves to a real file on disk. A clean
clone has no binaries in `src-tauri/binaries/` — they're gitignored, and
normally built/downloaded by CI — so without this step the first `cargo
test` or `cargo clippy` fails immediately with `glob pattern binaries/
lib/* path not found or didn't match any files`. Run once per clone, from
the repo root:

```
./scripts/stub-sidecar-binaries.sh x86_64-unknown-linux-gnu src-tauri/binaries
```

On ARM64, use `aarch64-unknown-linux-gnu` instead. This is exactly what
CI runs before its own fmt/clippy/test/build steps (see
`.github/workflows/ci.yml`). The placeholders are no-op stand-ins — that's
enough because none of the commands below actually execute the sidecars;
the tests that do are `#[ignore]`d.

Frontend (from repo root):
```
npm run lint
npm run build      # tsc typecheck + vite build
npm test           # vitest
```

Rust (from `src-tauri/`):
```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Don't open a PR with failing checks "to get feedback" — open it as a draft
instead and say so.

## Bumping sidecar versions

`scripts/fetch-sidecar-binaries.sh` pins yt-dlp and Deno to an explicit
version and an expected SHA-256, both recorded near the top of the
script — deliberately, not fetched from `latest` at build/release time
(see docs/adr/0002-yt-dlp-js-runtime-deno.md). Bumping a pin is a normal,
expected act, not something to avoid: yt-dlp especially needs to stay
current or YouTube extraction breaks. It just has to be done, not left
to happen silently.

To bump yt-dlp (the more volatile one — check this first when YouTube
extraction starts failing) or Deno:

1. Find the new version's release page (e.g.
   `https://github.com/yt-dlp/yt-dlp/releases/latest` or
   `https://github.com/denoland/deno/releases/latest`) and note its tag.
2. Download that release's checksum file(s) and read off the SHA-256 for
   each asset this script uses — yt-dlp's `SHA2-256SUMS` covers
   `yt-dlp_linux` and `yt-dlp_linux_aarch64`; Deno publishes a
   `<asset>.sha256sum` file per zip (`deno-x86_64-unknown-linux-gnu.zip`,
   `deno-aarch64-unknown-linux-gnu.zip`). Don't compute the hash from a
   locally downloaded file and call it "verified" — read it from
   upstream's own published checksum, so the pin reflects what upstream
   says it shipped, not just what you happened to receive.
3. Update `YT_DLP_VERSION`/`DENO_VERSION` and the corresponding
   `*_SHA256` values in `scripts/fetch-sidecar-binaries.sh`, and the
   "Pinned versions (checked ...)" comment's date and links.
4. Run the script locally against a scratch directory (not
   `src-tauri/binaries` if others are using it) and confirm it downloads
   and verifies cleanly:
   `scripts/fetch-sidecar-binaries.sh x86_64 x86_64-unknown-linux-gnu /tmp/some-scratch-dir`
5. Open the PR as normal. The release workflow prints the pinned
   mpv/yt-dlp/Deno versions and attaches them as a text file to the
   GitHub release, so "what shipped in vX.Y.Z" stays answerable later.

## What happens to your contribution

By submitting a pull request or patch, you agree to the Contribution
License Grant in the [LICENSE](./LICENSE) addendum (section 3): you're
licensing your contribution to the maintainer so it can be merged and
distributed as part of Echora under these same terms. No separate CLA
form is required.

## Code style

Enforced by CI (`cargo fmt`, `cargo clippy -D warnings`, ESLint,
`tsc --noEmit`). Match the existing structure — see `CLAUDE.md`.

## Reporting bugs

Open a GitHub issue. Include your OS/distro, Echora version, and steps to
reproduce. For security issues, see [SECURITY.md](./SECURITY.md) instead
of opening a public issue.
