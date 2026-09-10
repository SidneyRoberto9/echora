import { useEffect, useRef, useState } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useSettings } from "../hooks/useSettings";
import { api, type CrashSummary, type LicenseEntry } from "../lib/api";
import { EmptyState } from "./EmptyState";
import { EmptyQueueIcon } from "./icons";

const SPONSORBLOCK_CATEGORIES: { key: string; label: string }[] = [
  { key: "sponsor", label: "Sponsor segments" },
  { key: "selfpromo", label: "Self-promotion" },
  { key: "intro", label: "Intro" },
  { key: "outro", label: "Outro" },
];

const GITHUB_ISSUES_URL = "https://github.com/SidneyRoberto9/echora/issues/new";

function formatRelativeTime(unixMillis: number): string {
  const diffMinutes = Math.round((Date.now() - unixMillis) / 60000);
  if (diffMinutes < 1) return "just now";
  if (diffMinutes < 60) return `${diffMinutes}m ago`;
  const diffHours = Math.round(diffMinutes / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  return `${Math.round(diffHours / 24)}d ago`;
}

function CrashReportsList({
  enabled,
  onError,
}: {
  enabled: boolean;
  onError: (message: string) => void;
}) {
  const [reports, setReports] = useState<CrashSummary[]>([]);

  useEffect(() => {
    if (!enabled) {
      return;
    }
    api.listCrashReports().then(setReports).catch(() => {});
  }, [enabled]);

  const handleReport = async (id: string) => {
    try {
      const body = await api.getCrashReportMarkdown(id);
      const url = `${GITHUB_ISSUES_URL}?title=${encodeURIComponent(`Crash report: ${id}`)}&body=${encodeURIComponent(body)}&labels=crash-report`;
      await openUrl(url);
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  };

  const handleClearAll = async () => {
    try {
      await api.clearCrashReports();
      api.listCrashReports().then(setReports).catch(() => {});
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    }
  };

  if (!enabled) return null;

  return (
    <div className="crash-reports-list">
      {reports.length === 0 ? (
        <p className="settings-section__hint">No crashes recorded.</p>
      ) : (
        <>
          {reports.map((r) => (
            <div className="settings-row" key={r.id}>
              <span className="settings-row__label">
                {r.kind} — {formatRelativeTime(r.timestamp)}
              </span>
              <button type="button" className="text-link" onClick={() => handleReport(r.id)}>
                Report
              </button>
            </div>
          ))}
          <div className="settings-row">
            <span className="settings-row__label">Clear all crash reports</span>
            <button
              type="button"
              className="text-link"
              style={{ color: "var(--danger)" }}
              onClick={handleClearAll}
            >
              Clear all
            </button>
          </div>
        </>
      )}
    </div>
  );
}

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

function LicensesSection() {
  const [licenses, setLicenses] = useState<LicenseEntry[]>([]);

  useEffect(() => {
    api.getThirdPartyLicenses().then(setLicenses).catch(() => {});
  }, []);

  if (licenses.length === 0) return null;

  return (
    <>
      <h2 className="settings-section__title">Third-Party Licenses</h2>
      {licenses.map((entry) => (
        <details className="license-entry" key={entry.component}>
          <summary>
            {entry.component} — {entry.license}
          </summary>
          <pre className="license-entry__text">{entry.text}</pre>
        </details>
      ))}
    </>
  );
}

interface SettingsViewProps {
  onError: (message: string) => void;
}

export function SettingsView({ onError }: SettingsViewProps) {
  const { settings, loading, error, update } = useSettings();

  useEffect(() => {
    if (error) onError(error);
  }, [error, onError]);

  if (loading) {
    return (
      <div className="settings-view">
        <div className="skeleton" style={{ height: 44, borderRadius: 14, marginBottom: 8 }} />
        <div className="skeleton" style={{ height: 44, borderRadius: 14 }} />
      </div>
    );
  }

  // Not loading and still no settings means the fetch failed (the error
  // itself is surfaced by the app's global banner) — don't leave a blank
  // screen instead.
  if (!settings) {
    return (
      <div className="settings-view">
        <EmptyState icon={<EmptyQueueIcon />} title="Couldn't load settings" />
      </div>
    );
  }

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
            <div className="settings-row__hint">Applied immediately — takes effect next time you log in</div>
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
        {/* ponytail: cache limit control removed (P1-2) — there's no disk
            cache layer yet for it to govern, so it was a setting that did
            nothing. Bring this section back once caching lands; per
            docs/REQUIREMENTS_FREEZE.md it also needs an "Unlimited"
            option this old control never had. */}

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
        <CrashReportsList enabled={settings.crash_report_enabled} onError={onError} />

        <div className="settings-row">
          <span>
            <div className="settings-row__label">Discord Rich Presence</div>
            <div className="settings-row__hint">
              Shows what's playing as your Discord status — only sent while Discord is running
            </div>
          </span>
          <Toggle
            on={settings.discord_presence_enabled}
            label="Discord Rich Presence"
            onChange={() =>
              update({ discord_presence_enabled: !settings.discord_presence_enabled })
            }
          />
        </div>
        <div className="privacy-note">No account · No cloud · No telemetry by default</div>

        <LicensesSection />
      </div>
    </div>
  );
}
