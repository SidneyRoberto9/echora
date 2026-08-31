import { getCurrentWindow } from "@tauri-apps/api/window";
import { CloseIcon, DiscoverIcon, HomeIcon, QueueIcon, SettingsIcon } from "./icons";
import type { View } from "../App";

interface TopBarProps {
  view: View;
  onChangeView: (view: View) => void;
}

export function TopBar({ view, onChangeView }: TopBarProps) {
  return (
    <div className="top-bar" data-tauri-drag-region>
      <div className="top-bar__brand" data-tauri-drag-region>
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
          className={`nav-icon-btn${view === "discover" ? " is-active" : ""}`}
          aria-current={view === "discover" || undefined}
          aria-label="Discover"
          onClick={() => onChangeView("discover")}
        >
          <DiscoverIcon />
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
        <span className="top-bar__nav-divider" aria-hidden="true" />
        <button
          type="button"
          className="nav-icon-btn nav-icon-btn--close"
          aria-label="Close Echora"
          onClick={() => {
            getCurrentWindow().close();
          }}
        >
          <CloseIcon size={15} />
        </button>
      </nav>
    </div>
  );
}
