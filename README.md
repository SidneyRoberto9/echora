<h1 align="center">Echora</h1>
<p align="center"><i>Choose a mood. Press play.</i></p>

Echora is an extremely lightweight, mood-first desktop audio player for
Linux. Pick how you want to feel — *Villain*, *Focus*, *Night Drive*,
*Still in Love* — and Echora finds and plays matching audio, without
keeping a browser or the full YouTube interface open just to listen to
music.

> **Status: pre-release development.** Core loop (mood → search → play →
> queue → history), the React UI, and desktop integration (tray, MPRIS
> media keys, autostart) work in dev builds; there is no packaged
> `.deb`/AppImage yet. See `docs/` for the current architecture and
> decisions.

## Why

Searching things like *"POV: I'm a Villain"* or *"POV: I'm Still in
Love"* playlists on YouTube works, but it means keeping a full browser
tab (and the whole YouTube web app) resident just to play audio in the
background. Echora's only job is to make that lighter.

## Priorities, in order

1. **Lightness** — RAM, CPU, process count, startup time, package size,
   cache footprint, before anything else.
2. Stability
3. Correctness
4. Security
5. User experience
6. Maintainability
7. Aesthetics
8. Implementation convenience

## Architecture

```
React / TypeScript  (display layer only)
        │  Tauri IPC (typed commands)
        ▼
Rust core            — source of truth for all state
 ├─ Mood Engine
 ├─ Recommendation Engine
 ├─ Search / Discovery
 ├─ Queue Manager
 ├─ Playback Controller
 ├─ History, Favorites, Settings, Cache
 ├─ Persistence (SQLite)
 └─ Platform integrations (tray, MPRIS, autostart, updater)
        │
        ├─ yt-dlp (sidecar subprocess)  → resolves/searches media
        └─ mpv    (sidecar subprocess)  → plays audio, via IPC socket
```

Rust owns playback, queue, mood selection, history, recommendations,
cache, and settings. React never duplicates that state — it only
displays it and sends commands.

mpv and yt-dlp are never linked into Echora's own binary — they run as
separate sidecar processes, controlled over their own IPC/CLI
interfaces. This is a deliberate licensing decision, not just style: see
[`docs/adr/0001-mpv-sidecar-not-libmpv.md`](docs/adr/0001-mpv-sidecar-not-libmpv.md)
and [`docs/adr/0006-third-party-licensing-and-bundling.md`](docs/adr/0006-third-party-licensing-and-bundling.md).

Full list of decisions: [`docs/adr/`](docs/adr/). Locked product
requirements: [`docs/REQUIREMENTS_FREEZE.md`](docs/REQUIREMENTS_FREEZE.md).

## Stack

Tauri 2 · React · TypeScript · Rust · mpv (sidecar) · yt-dlp (sidecar) ·
Deno (sidecar, JS runtime required by yt-dlp for YouTube) · SQLite
(`rusqlite`) · MPRIS (`mpris-server`, Linux)

No Electron. No embedded YouTube web UI. No extra WebViews.

## Platforms

Linux only for v1 (Ubuntu/Zorin-based), `x86_64` and `ARM64`, as `.deb`
and AppImage. Fully self-contained: no manual `apt install mpv`,
`pip install yt-dlp`, or any runtime install is ever asked of the user —
everything Echora needs ships inside its own package.

## Privacy

No account, no cloud, no telemetry by default. The one opt-in exception
is a fully manual crash report: on crash, Echora writes a local log and
offers a button that opens a pre-filled GitHub issue in your browser —
nothing is ever sent automatically.

## Development

Requires Node.js + npm, and the Rust toolchain (see `rust-toolchain.toml`
for the pinned version). Linux build prerequisites:
<https://tauri.app/start/prerequisites/#linux>.

```bash
npm install
npm run tauri dev      # run in development
```

Checks before committing:
```bash
npm run lint && npm run build      # frontend
cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

## Performance

Benchmarks (startup, idle, tray, playback RAM/CPU, package size) will be
published here once there's a build to measure. No consumption numbers
are claimed before they're measured.

## License

Source-available under the PolyForm Strict License 1.0.0 plus a custom
addendum — **not open source**. You may read the code, open issues, and
submit pull requests to this repository; the license does not grant a
general right to reuse, redistribute, or build other products from this
code. See [`LICENSE`](LICENSE) and [`CONTRIBUTING.md`](CONTRIBUTING.md).

Third-party components bundled at runtime (mpv, yt-dlp, Deno, etc.) keep
their own licenses — see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
