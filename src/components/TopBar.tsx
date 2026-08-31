import { HomeIcon, QueueIcon, SettingsIcon } from "./icons";
import type { View } from "../App";

interface TopBarProps {
  view: View;
  onChangeView: (view: View) => void;
}

export function TopBar({ view, onChangeView }: TopBarProps) {
  return (
    <div className="top-bar">
      <div className="top-bar__brand">
        <span className="brand-mark" aria-hidden="true" />
        <span>echora</span>
      </div>
      <nav className="top-bar__nav" aria-label="Main">
        <button
          type="button"
          className={`nav-icon-btn${view === "home" ? " is-active" : ""}`}
          aria-current={view === "home" || undefined}
          aria-label="Home"
          onClick={() => onChangeView("home")}
        >
          <HomeIcon />
        </button>
        <button
          type="button"
          className={`nav-icon-btn${view === "queue" ? " is-active" : ""}`}
          aria-current={view === "queue" || undefined}
          aria-label="Queue"
          onClick={() => onChangeView("queue")}
        >
          <QueueIcon />
        </button>
        <button
          type="button"
          className={`nav-icon-btn${view === "settings" ? " is-active" : ""}`}
          aria-current={view === "settings" || undefined}
          aria-label="Settings"
          onClick={() => onChangeView("settings")}
        >
          <SettingsIcon />
        </button>
      </nav>
    </div>
  );
}
