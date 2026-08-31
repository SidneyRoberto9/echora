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

Frontend (from repo root):
```
npm run lint
npm run build      # tsc typecheck + vite build
npm test           # once test suites exist
```

Rust (from `src-tauri/`):
```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Don't open a PR with failing checks "to get feedback" — open it as a draft
instead and say so.

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
