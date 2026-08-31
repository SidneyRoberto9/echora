# Echora — Project Instructions

Mood-first, ultra-lightweight desktop audio player. Linux only (Ubuntu/
Zorin-based), x86_64 + ARM64. Tauri 2 + React + TypeScript + Rust + MPV
(sidecar) + yt-dlp (sidecar). See `docs/adr/` for why each dependency
exists and `docs/REQUIREMENTS_FREEZE.md` for the locked product decisions.

## Priority order (never reorder without the user's explicit say-so)

1. Lightness & efficiency (RAM, CPU, process count, startup time, binary
   size, cache)
2. Stability
3. Correctness
4. Security
5. UX
6. Maintainability
7. Aesthetics
8. Implementation convenience

## Hard rules

- **No Electron.** No embedding the YouTube web UI. No extra WebViews
  beyond the one Tauri window.
- **Rust is the source of truth.** Playback, queue, mood selection,
  history, recommendations, cache, settings, and all sidecar process
  lifecycle live in Rust. React is a display layer — no business logic,
  no duplicated global state for things Rust already owns.
- **mpv and yt-dlp run only as sidecar subprocesses**, controlled via
  their documented IPC/CLI interfaces. Never FFI-link `libmpv` or invoke
  yt-dlp's Python internals directly — this is a licensing decision (see
  ADR 0001, 0006), not just a style preference.
- **Package manager: npm only.** No pnpm, yarn, or bun, anywhere in this
  repo.
- **End users install nothing manually.** No "please install mpv/yt-dlp/
  Python/Node/Deno" instructions ever reach the user. Everything needed
  ships inside Echora's own `.deb`/AppImage.
- **No account, no cloud, no telemetry by default.** The one exception is
  a fully manual, opt-in crash report (local log + a button that opens a
  pre-filled GitHub issue in the browser) — never an automatic network
  call.
- **Don't add a dependency "just in case."** Before adding one, check:
  does the standard library, an already-installed dependency, or a
  native platform feature already do this?

## Security

- Never build shell commands by concatenating strings from search
  queries, titles, URLs, or metadata. Pass arguments as argument arrays
  to sidecar processes, always.
- Treat all external metadata (search results, titles, thumbnails,
  durations, channel names) as untrusted; normalize and validate before
  display or persistence.
- Tauri capabilities: grant the frontend only the specific IPC commands
  it needs. No generic/catch-all commands.

## Before claiming something works

Run what's relevant and say what you actually verified — don't say
"should work." At minimum before claiming a change is done:
```
# frontend
npm run lint && npm run build

# rust (from src-tauri/)
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```
Full build/package verification (`cargo tauri build`) before claiming a
release-shaped milestone is done. If something can't be verified in the
current environment, say exactly what wasn't checked.

## License

Source-available under PolyForm Strict License 1.0.0 + a custom
addendum (see `LICENSE`) — not open source. Don't add code whose license
would force a copyleft or redistribution obligation onto Echora's own
source; anything with that risk goes through `licensing-compliance-reviewer`
first and gets recorded in `THIRD_PARTY_NOTICES.md`.

## Subagents

Defined in `.claude/agents/`. Don't spend Sonnet on mechanical fact-finding
(that's `ecosystem-researcher` / `media-integration-researcher`, Haiku).
Don't let Haiku make architectural or licensing calls alone. Subagents
don't spawn other subagents — that stays with the main agent.
