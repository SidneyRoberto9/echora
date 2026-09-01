# Auto-update Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Tauri's official updater plugin into Echora so an AppImage
build can check for and install new releases from Settings; a `.deb`
build shows a static "check the releases page" note instead, since Tauri's
updater can't self-replace a `.deb` install.

**Architecture:** Two plugins (`tauri-plugin-updater`, `tauri-plugin-process`
for the post-install restart) get registered in `lib.rs`, granted minimal
capabilities, and configured in `tauri.conf.json` with a placeholder
`pubkey` (real key generation and the CI release pipeline that publishes
`latest.json` belong to the later Packaging milestone — out of scope
here). A tiny new command (`is_appimage_build`, checking the `APPIMAGE`
env var the AppImage runtime itself sets) lets the frontend gate the
whole update UI to the one distribution format that can actually use it.
Backend lands first (Rust whole-crate compilation requires the plugin
registration and the new command to land together); frontend depends on
`is_appimage_build` existing.

**Tech Stack:** Tauri 2 (`tauri-plugin-updater`, `tauri-plugin-process`),
Rust, React + TypeScript (`@tauri-apps/plugin-updater`,
`@tauri-apps/plugin-process`).

**Spec:** `docs/superpowers/specs/2026-09-01-auto-update-design.md`

## Global Constraints

- Package manager: npm only (repo-wide, see CLAUDE.md).
- No real signing keypair generation, no CI/release pipeline in this
  plan — the `pubkey` in `tauri.conf.json` is a placeholder string,
  clearly named, safe to commit (public keys aren't secret; this one just
  isn't functional yet). Packaging replaces it with a real one.
- No automatic background update check — manual only, a button in
  Settings. Never call `check()` except in direct response to that
  button.
- AppImage is the only build that gets a working check/install flow;
  `.deb`/dev builds get a static note + link, never a non-functional
  button.
- Capabilities stay minimal: `updater:default` and `process:allow-restart`
  only — no broader `process:default`.
- Every command run before claiming a task done: `cargo fmt --check &&
  cargo clippy --all-targets -- -D warnings && cargo test` (from
  `src-tauri/`) and `npm run lint && npm run build` (frontend task).

---

### Task 1: Backend — updater/process plugins and the AppImage-detection command

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/tauri.conf.json`

**Interfaces:**
- Produces: Tauri command `is_appimage_build() -> bool`; the `updater`/`process`
  plugins registered and reachable from the frontend via their own JS
  packages (added in Task 2).
- Consumes: nothing new from outside this task.

- [ ] **Step 1: Add the two plugin dependencies**

In `src-tauri/Cargo.toml`, add to `[dependencies]` (alongside
`tauri-plugin-autostart`):

```toml
tauri-plugin-updater = "2"
tauri-plugin-process = "2"
```

- [ ] **Step 2: Write the failing test for `is_appimage_build`**

In `src-tauri/src/lib.rs`, add a stub command and its test together:

```rust
/// Whether this process is running from an AppImage — set by the
/// AppImage runtime itself (`APPIMAGE` env var points at the mounted
/// image). Only the AppImage build supports in-place self-update; `.deb`
/// installs get a static "check the releases page" note in Settings
/// instead (see docs/superpowers/specs/2026-09-01-auto-update-design.md).
#[tauri::command]
fn is_appimage_build() -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_appimage_build_reflects_the_appimage_env_var() {
        // SAFETY: this test mutates process-global env state
        // (`APPIMAGE`); no other test in this crate reads or writes that
        // variable, and Rust's test harness doesn't run this specific
        // test concurrently with itself, so there's no cross-test race —
        // but do not add another test touching `APPIMAGE` without
        // giving both a `#[serial]`-style guard or merging them into one.
        unsafe {
            std::env::remove_var("APPIMAGE");
        }
        assert!(!is_appimage_build());

        unsafe {
            std::env::set_var("APPIMAGE", "/tmp/echora.AppImage");
        }
        assert!(is_appimage_build());

        unsafe {
            std::env::remove_var("APPIMAGE");
        }
    }
}
```

Place the command near the existing `greet` command (top of the file,
before `#[cfg_attr(mobile, tauri::mobile_entry_point)]`); place the
`#[cfg(test)] mod tests` block at the end of the file.

- [ ] **Step 3: Run the test to verify it fails**

Run: `cd src-tauri && cargo test is_appimage_build_reflects_the_appimage_env_var`
Expected: FAIL — panics with `not yet implemented` from the `todo!()`.

- [ ] **Step 4: Implement `is_appimage_build`**

Replace the `todo!()` body:

```rust
#[tauri::command]
fn is_appimage_build() -> bool {
    std::env::var_os("APPIMAGE").is_some()
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cd src-tauri && cargo test is_appimage_build_reflects_the_appimage_env_var`
Expected: PASS

- [ ] **Step 6: Register the plugins and the command**

In `src-tauri/src/lib.rs`, inside `tauri::Builder::default()`'s chain, add
the two new `.plugin(...)` calls after the existing ones:

```rust
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
```

In `invoke_handler(tauri::generate_handler![...])`, add `is_appimage_build`
alongside `greet`:

```rust
            greet,
            is_appimage_build,
```

- [ ] **Step 7: Grant the capabilities**

In `src-tauri/capabilities/default.json`, add to the `permissions` array:

```json
    "updater:default",
    "process:allow-restart"
```

(Resulting array, for reference — confirm the exact current contents
before editing, since line numbers drift: `core:default`,
`core:window:allow-close`, `core:window:allow-start-dragging`,
`opener:default`, then the two new ones.)

- [ ] **Step 8: Add the updater config**

In `src-tauri/tauri.conf.json`, add `"createUpdaterArtifacts": true` to
the existing `"bundle"` object, and add a new top-level `"plugins"` key
(sibling of `"bundle"`):

```json
  "bundle": {
    "active": true,
    "targets": "all",
    "createUpdaterArtifacts": true,
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
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

- [ ] **Step 9: Run the full backend test suite**

Run: `cd src-tauri && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: PASS, zero warnings

- [ ] **Step 10: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/lib.rs \
  src-tauri/capabilities/default.json src-tauri/tauri.conf.json
git commit -m "feat(auto-update): register updater and process plugins"
```

---

### Task 2: Frontend — Settings UI to check for and install updates

**Files:**
- Modify: `package.json`
- Modify: `src/lib/api.ts`
- Modify: `src/components/SettingsView.tsx`

**Interfaces:**
- Consumes: Tauri command `is_appimage_build` (Task 1); `@tauri-apps/plugin-updater`'s
  `check()` and the `Update` object's `.downloadAndInstall()`;
  `@tauri-apps/plugin-process`'s `relaunch()`; `@tauri-apps/api/app`'s
  `getVersion()`.
- Produces: `api.isAppimageBuild() => Promise<boolean>`; a rendered
  "Updates" section in Settings.

- [ ] **Step 1: Add the npm dependencies**

In `package.json`, add to `"dependencies"` (alongside
`"@tauri-apps/plugin-opener": "^2"`):

```json
    "@tauri-apps/plugin-updater": "^2",
    "@tauri-apps/plugin-process": "^2",
```

Run `npm install` to update `package-lock.json`.

- [ ] **Step 2: Add the API wrapper**

In `src/lib/api.ts`, add to the `api` object (near other simple
no-argument boolean-returning wrappers):

```ts
  isAppimageBuild: () => call<boolean>("is_appimage_build"),
```

- [ ] **Step 3: Add the `UpdatesSection` component and wire it in**

In `src/components/SettingsView.tsx`, add imports at the top:

```tsx
import { useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { useSettings } from "../hooks/useSettings";
import { api } from "../lib/api";
```

(`useEffect` already exists in the current import line — merge into the
existing `import { useEffect } from "react";` rather than duplicating it;
add `useRef, useState` to that same import.)

Add the component (place it after the `Toggle` component, before
`SettingsViewProps`):

```tsx
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

Render it in the left `settings-column`, right after the "Startup"
section's closing `</div>` and before that column's closing `</div>`:

```tsx
        <UpdatesSection onError={onError} />
      </div>
```

- [ ] **Step 4: Verify frontend build**

Run: `npm run lint && npm run build`
Expected: PASS, zero errors

- [ ] **Step 5: Manual check (documented, not automated)**

No Tauri window in this sandbox. Whoever next runs `npm run tauri dev`
should confirm: the Settings screen renders an "Updates" section without
crashing (in dev, `is_appimage_build` returns `false`, so this exercises
the static-note branch — the releases-page link should open in the
browser); no console errors from the new imports. The AppImage branch
(Check for Updates → Download & Install → relaunch) can't be exercised
for real until Packaging publishes a signed `latest.json` — that's
expected, not a gap in this task.

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json src/lib/api.ts src/components/SettingsView.tsx
git commit -m "feat(auto-update): add update check and install UI to Settings"
```
