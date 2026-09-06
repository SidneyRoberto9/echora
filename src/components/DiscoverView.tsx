import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useDiscover } from "../hooks/useDiscover";
import { LibraryTab } from "./LibraryTab";
import { StatsTab } from "./StatsTab";
import type { useMoods } from "../hooks/useMoods";
import type { SessionMood, Track } from "../lib/api";

type DiscoverTab = "library" | "stats";

interface DiscoverViewProps {
  /** Reuses `App`'s already-loaded catalog — Discover must not re-fetch it. */
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onStartMix: (moods: SessionMood[]) => void;
  onPlayTrack: (track: Track) => void;
  onPlayScene: (sceneId: number) => void;
  sceneSaveTick: number;
}

export function DiscoverView({
  moodsData,
  onError,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onStartMix,
  onPlayTrack,
  onPlayScene,
  sceneSaveTick,
}: DiscoverViewProps) {
  const [tab, setTab] = useState<DiscoverTab>("library");
  const discover = useDiscover();
  const tabOrder: DiscoverTab[] = ["library", "stats"];
  const tabRefs = useRef<Record<DiscoverTab, HTMLButtonElement | null>>({ library: null, stats: null });

  const focusTab = (next: DiscoverTab) => {
    setTab(next);
    tabRefs.current[next]?.focus();
  };

  const onTabKeyDown = (e: KeyboardEvent<HTMLButtonElement>) => {
    const idx = tabOrder.indexOf(tab);
    if (e.key === "ArrowRight") {
      e.preventDefault();
      focusTab(tabOrder[(idx + 1) % tabOrder.length]);
    } else if (e.key === "ArrowLeft") {
      e.preventDefault();
      focusTab(tabOrder[(idx - 1 + tabOrder.length) % tabOrder.length]);
    } else if (e.key === "Home") {
      e.preventDefault();
      focusTab(tabOrder[0]);
    } else if (e.key === "End") {
      e.preventDefault();
      focusTab(tabOrder[tabOrder.length - 1]);
    }
  };

  useEffect(() => {
    if (discover.error) onError(discover.error);
  }, [discover.error, onError]);

  useEffect(() => {
    if (moodsData.error) onError(moodsData.error);
  }, [moodsData.error, onError]);

  const isFirstRender = useRef(true);
  useEffect(() => {
    if (isFirstRender.current) {
      isFirstRender.current = false;
      return;
    }
    discover.refreshScenes();
    // Keyed on the stable `refreshScenes` callback, not the whole
    // `discover` object — `useDiscover` returns a fresh object literal on
    // every render, which would refetch scenes constantly instead of only
    // when `sceneSaveTick` actually changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sceneSaveTick, discover.refreshScenes]);

  return (
    <div className="discover-view">
      <div className="segmented" role="tablist" aria-label="Discover">
        <button
          ref={(el) => {
            tabRefs.current.library = el;
          }}
          type="button"
          role="tab"
          id="discover-tab-library"
          aria-selected={tab === "library"}
          aria-controls="discover-panel-library"
          tabIndex={tab === "library" ? 0 : -1}
          className={`segment${tab === "library" ? " is-active" : ""}`}
          onClick={() => setTab("library")}
          onKeyDown={onTabKeyDown}
        >
          Library
        </button>
        <button
          ref={(el) => {
            tabRefs.current.stats = el;
          }}
          type="button"
          role="tab"
          id="discover-tab-stats"
          aria-selected={tab === "stats"}
          aria-controls="discover-panel-stats"
          tabIndex={tab === "stats" ? 0 : -1}
          className={`segment${tab === "stats" ? " is-active" : ""}`}
          onClick={() => setTab("stats")}
          onKeyDown={onTabKeyDown}
        >
          Statistics
        </button>
      </div>

      {tab === "library" ? (
        <div id="discover-panel-library" role="tabpanel" aria-labelledby="discover-tab-library" tabIndex={0}>
          <LibraryTab
            discover={discover}
            moods={moodsData.moods}
            startingMoodId={startingMoodId}
            startingTrackId={startingTrackId}
            onStartMood={onStartMood}
            onStartMix={onStartMix}
            onPlayTrack={onPlayTrack}
            onPlayScene={onPlayScene}
            onError={onError}
          />
        </div>
      ) : (
        <div id="discover-panel-stats" role="tabpanel" aria-labelledby="discover-tab-stats" tabIndex={0}>
          <StatsTab stats={discover.stats} moods={moodsData.moods} loading={discover.loading} />
        </div>
      )}
    </div>
  );
}
