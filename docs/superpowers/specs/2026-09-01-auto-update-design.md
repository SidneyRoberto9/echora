# Auto-update — Design

Status: Approved. Third of the post-v1-audit features actually built
(SponsorBlock → Auto-update → Crash report → Packaging), per the user's
priority order. Intensity was removed from scope in the same session —
never had a concrete design, and `MoodTraits` on each catalog mood remain
unused data.

## Purpose

Wire Tauri's official updater plugin so Echora can check for and install
new releases. Scoped narrowly: this task adds the plugin, the capability
grants, the config skeleton, and a Settings UI to check/install — it does
**not** generate the real signing keypair or build the CI release pipeline
that publishes signed artifacts + a `latest.json` manifest. That belongs
to the later Packaging milestone, which is the actual producer of what
this task's client-side code consumes. Until Packaging exists, "Check for
Updates" has nothing real to find — that's expected and fine; this task's
job is to have the client-side machinery correctly in place and correctly
gated.

## Key constraint discovered during research

Tauri's updater plugin does **not** support in-place self-replacement for
`.deb`-installed apps (a Tauri bundler change from Feb 2026 accepts `.deb`
as an updater build target but explicitly does not enable self-replacing
it) — only AppImage supports true silent in-place update+restart on
Linux. Echora ships both formats (per REQUIREMENTS_FREEZE.md). Decision:
**AppImage is the real auto-update channel.** A `.deb` install shows a
static note pointing to the GitHub releases page instead of a "Check for
Updates" flow it can't actually fulfill.

## Non-goals (this task)

- No real signing keypair generation — `pubkey` in `tauri.conf.json` is a
  clearly-marked placeholder string, safe to commit (public keys aren't
  secret; the file just isn't functional yet). Packaging generates the
  real keypair and overwrites this value.
- No CI/release pipeline, no `latest.json` manifest hosting — Packaging's
  job.
- No automatic background check on launch — manual only, a "Check for
  Updates" button in Settings. Matches the project's existing "no
  automatic network call without a direct user action" posture (same
  shape as the manual crash-report flow).
- No update check or UI at all for `.deb` builds beyond a static note +
  link — the updater genuinely can't help them install in place.

## Backend (Rust)

### `src-tauri/Cargo.toml`

```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

### `src-tauri/src/lib.rs`

Register both plugins alongside the existing ones:

```rust
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```

New standalone command (no `AppState` needed — mirrors the existing
`greet` command's shape), placed alongside it:

```rust
/// Whether this process is running from an AppImage — set by the AppImage
/// runtime itself (`APPIMAGE` env var points at the mounted image). Only
/// the AppImage build supports in-place self-update; `.deb` installs get
/// a static "check the releases page" note in Settings instead (see
/// docs/superpowers/specs/2026-09-01-auto-update-design.md for why).
#[tauri::command]
fn is_appimage_build() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}
```

Register it in `invoke_handler(tauri::generate_handler![...])` alongside
`greet`.

### `src-tauri/capabilities/default.json`

Add to the `permissions` array:

```json
    "updater:default",
    "process:allow-restart"
```

### `src-tauri/tauri.conf.json`

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "createUpdaterArtifacts": true,
    "icon": [ ... unchanged ... ]
  },
  "plugins": {
    "updater": {
      "pubkey": "PLACEHOLDER_REPLACE_WITH_REAL_PUBKEY_AT_PACKAGING_TIME",
      "endpoints": [
        "https://github.com/SidneyRoberto9/echora/releases/latest/download/latest.json"
      ]
    }
  }
```

The endpoint is `tauri-action`'s standard GitHub Releases convention
(auto-generates and uploads `latest.json` when `includeUpdaterJson: true`
is set in the release workflow) — Packaging wires the workflow that
actually produces it; this task just points at where it will live.

## Frontend (React)

### `package.json`

```json
    "@tauri-apps/plugin-updater": "^2",
    "@tauri-apps/plugin-process": "^2",
```

### `src/lib/api.ts`

```ts
  isAppimageBuild: () => call<boolean>("is_appimage_build"),
```

### `src/components/SettingsView.tsx`

New `UpdatesSection` component in the same file (mirrors the existing
`Toggle` helper's pattern — not reused elsewhere, doesn't warrant its own
file), rendered as a new "Updates" section in the left column, after
"Startup":

```tsx
import { useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";

type UpdateStatus = "idle" | "checking" | "up-to-date" | "available" | "downloading" | "installed";

function UpdatesSection({ onError }: { onError: (message: string) => void }) {
  const [isAppimage, setIsAppimage] = useState<boolean | null>(null);
  const [version, setVersion] = useState("");
  const [status, setStatus] = useState<UpdateStatus>("idle");
  const [availableVersion, setAvailableVersion] = useState<string | null>(null);
  const pendingUpdate = useRef<Update | null>(null);

  useEffect(() => {
    api.isAppimageBuild().then(setIsAppimage).catch(() => setIsAppimage(false));
    getVersion().then(setVersion).catch(() => {});
  }, []);

  const handleCheck = async () => {
    setStatus("checking");
    try {
      const update = await check();
      if (update) {
        pendingUpdate.current = update;
        setAvailableVersion(update.version);
        setStatus("available");
      } else {
        setStatus("up-to-date");
      }
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      setStatus("idle");
    }
  };

  const handleInstall = async () => {
    if (!pendingUpdate.current) return;
    setStatus("downloading");
    try {
      await pendingUpdate.current.downloadAndInstall();
      setStatus("installed");
      await relaunch();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      setStatus("available");
    }
  };

  if (isAppimage === null) return null;

  return (
    <>
      <h2 className="settings-section__title">Updates</h2>
      {isAppimage ? (
        <div className="settings-row">
          <span>
            <div className="settings-row__label">Version {version}</div>
            <div className="settings-row__hint">
              {status === "up-to-date" ? "You're on the latest version" : null}
              {status === "available" && availableVersion ? `Version ${availableVersion} is available` : null}
              {status === "downloading" ? "Downloading…" : null}
              {status === "installed" ? "Installed — restarting…" : null}
            </div>
          </span>
          {status === "available" ? (
            <button type="button" className="text-link" onClick={handleInstall}>
              Download &amp; Install
            </button>
          ) : (
            <button
              type="button"
              className="text-link"
              onClick={handleCheck}
              disabled={status === "checking" || status === "downloading"}
            >
              {status === "checking" ? "Checking…" : "Check for Updates"}
            </button>
          )}
        </div>
      ) : (
        <p className="settings-section__hint">
          Auto-update is only available in the AppImage build. Running the
          .deb package? Check the{" "}
          <a
            href="https://github.com/SidneyRoberto9/echora/releases"
            target="_blank"
            rel="noreferrer"
          >
            releases page
          </a>{" "}
          for the latest version.
        </p>
      )}
    </>
  );
}
```

Rendered inside the existing left `settings-column`, after the "Startup"
section's closing `</div>`:

```tsx
        <UpdatesSection onError={onError} />
      </div>
```

No new CSS — reuses `settings-section__title`, `settings-row`,
`settings-row__label`, `settings-row__hint`, `text-link`, and
`settings-section__hint`, all already defined in `styles.css`.

## Error handling / edge cases

- `check()` throwing (network failure, malformed manifest once one
  exists, no manifest yet since Packaging hasn't shipped it) — caught,
  surfaced via the existing `onError` prop (same toast/banner path every
  other Settings action already uses), status resets to `idle`.
- `downloadAndInstall()` failing partway — caught, status reverts to
  `available` so the user can retry without re-checking.
- `.deb`/dev builds (no `APPIMAGE` env var): `isAppimageBuild` command
  returns `false`, the whole check/install UI never renders — no dead
  buttons, no confusing "Check for Updates" that can't work.
- Before Packaging ships a real `latest.json`: `check()` will fail
  (404/network error) or find nothing — this task doesn't need to handle
  "found an update" as a real, currently-reachable state; the code path
  is written and tested against the plugin's documented shape, not
  against a live manifest that doesn't exist yet.

## Testing

- Rust: `is_appimage_build` is a one-line env-var check — a unit test
  setting/clearing the `APPIMAGE` env var and asserting the return value
  is enough (note: Rust tests run in threads sharing process env vars, so
  this test must not run in parallel with anything else touching
  `APPIMAGE` — none of this codebase's other tests do, so no isolation
  helper needed).
- Frontend: no test framework change — `npm run lint && npm run build`
  covers `UpdatesSection`. Manual verification (documented, not
  automated) belongs to whoever next runs `npm run tauri dev`: confirm
  the Settings screen renders the "Updates" section without crashing
  (since `isAppimageBuild` will return `false` in dev, this exercises the
  static-note branch), and confirm the AppImage-branch UI at least
  compiles/type-checks correctly even though it can't be exercised for
  real without a signed `latest.json` from Packaging.
