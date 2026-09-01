import { useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { useSettings } from "../hooks/useSettings";
import { api } from "../lib/api";

const CACHE_OPTIONS: { label: string; mb: number }[] = [
  { label: "250MB", mb: 250 },
  { label: "500MB", mb: 500 },
  { label: "1GB", mb: 1024 },
  { label: "2GB", mb: 2048 },
];

const SPONSORBLOCK_CATEGORIES: { key: string; label: string }[] = [
  { key: "sponsor", label: "Sponsor segments" },
  { key: "selfpromo", label: "Self-promotion" },
  { key: "intro", label: "Intro" },
  { key: "outro", label: "Outro" },
];

interface ToggleProps {
  on: boolean;
  label: string;
  onChange: () => void;
}

function Toggle({ on, label, onChange }: ToggleProps) {
  return (
    <span className="toggle-hit">
      <button
        type="button"
        className={`toggle${on ? " is-on" : ""}`}
        role="switch"
        aria-checked={on}
        aria-label={label}
        onClick={onChange}
      />
    </span>
  );
}

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
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      setStatus("available");
      return;
    }
    setStatus("installed");
    try {
      await relaunch();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
      // Install already succeeded — don't revert to "available" (that
      // would wrongly imply the install itself needs retrying). Status
      // stays "installed"; the existing hint text ("Installed —
      // restarting…") already covers the "please restart manually" case
      // closely enough, and onError surfaces that relaunch specifically
      // failed.
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
            <div className="settings-row__hint" aria-live="polite">
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

interface SettingsViewProps {
  onError: (message: string) => void;
}

export function SettingsView({ onError }: SettingsViewProps) {
  const { settings, error, update } = useSettings();

  useEffect(() => {
    if (error) onError(error);
  }, [error, onError]);

  if (!settings) return null;

  const toggleSponsorBlockCategory = (key: string) => {
    const has = settings.sponsorblock_categories.includes(key);
    const next = has
      ? settings.sponsorblock_categories.filter((c) => c !== key)
      : [...settings.sponsorblock_categories, key];
    update({ sponsorblock_categories: next });
  };

  const handleClearHistory = () => {
    api.clearHistory().catch((err) => onError(err instanceof Error ? err.message : String(err)));
  };

  return (
    <div className="settings-view">
      <div className="settings-column">
        <h2 className="settings-section__title">SponsorBlock</h2>
        {SPONSORBLOCK_CATEGORIES.map((category) => (
          <div className="settings-row" key={category.key}>
            <span className="settings-row__label">{category.label}</span>
            <Toggle
              on={settings.sponsorblock_categories.includes(category.key)}
              label={category.label}
              onChange={() => toggleSponsorBlockCategory(category.key)}
            />
          </div>
        ))}
        <p className="settings-section__hint">
          Segment data from{" "}
          <a href="https://sponsor.ajay.app" target="_blank" rel="noreferrer">
            SponsorBlock
          </a>{" "}
          (
          <a
            href="https://creativecommons.org/licenses/by-nc-sa/4.0/"
            target="_blank"
            rel="noreferrer"
          >
            CC BY-NC-SA 4.0
          </a>
          ).
        </p>

        <h2 className="settings-section__title">Startup</h2>
        <div className="settings-row">
          <span>
            <div className="settings-row__label">Launch Echora at login</div>
            <div className="settings-row__hint">Takes effect the next time you install an update</div>
          </span>
          <Toggle
            on={settings.autostart_enabled}
            label="Launch Echora at login"
            onChange={() => update({ autostart_enabled: !settings.autostart_enabled })}
          />
        </div>

        <UpdatesSection onError={onError} />
      </div>

      <div className="settings-column">
        <h2 className="settings-section__title">Cache</h2>
        <div className="settings-row">
          <span className="settings-row__label">Limit</span>
          <div className="segmented" role="radiogroup" aria-label="Cache limit">
            {CACHE_OPTIONS.map((option) => (
              <button
                key={option.mb}
                type="button"
                className={`segment${settings.cache_limit_mb === option.mb ? " is-active" : ""}`}
                role="radio"
                aria-checked={settings.cache_limit_mb === option.mb}
                onClick={() => update({ cache_limit_mb: option.mb })}
              >
                {option.label}
              </button>
            ))}
          </div>
        </div>

        <h2 className="settings-section__title">History</h2>
        <div className="settings-row">
          <span className="settings-row__label">Save listening history</span>
          <Toggle
            on={settings.history_enabled}
            label="Save listening history"
            onChange={() => update({ history_enabled: !settings.history_enabled })}
          />
        </div>
        <div className="settings-row">
          <span className="settings-row__label">Clear all history</span>
          <button type="button" className="text-link" style={{ color: "var(--danger)" }} onClick={handleClearHistory}>
            Clear all
          </button>
        </div>

        <h2 className="settings-section__title">Privacy</h2>
        <div className="settings-row">
          <span>
            <div className="settings-row__label">Crash reports</div>
            <div className="settings-row__hint">
              Nothing sends automatically — you review and open a GitHub issue yourself
            </div>
          </span>
          <Toggle
            on={settings.crash_report_enabled}
            label="Crash reports"
            onChange={() => update({ crash_report_enabled: !settings.crash_report_enabled })}
          />
        </div>
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>
      </div>
    </div>
  );
}
