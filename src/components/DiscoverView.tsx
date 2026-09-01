import { useEffect, useState } from "react";
import { useDiscover } from "../hooks/useDiscover";
import { LibraryTab } from "./LibraryTab";
import { StatsTab } from "./StatsTab";
import type { useMoods } from "../hooks/useMoods";
import type { Track } from "../lib/api";

type DiscoverTab = "library" | "stats";

interface DiscoverViewProps {
  /** Reuses `App`'s already-loaded catalog — Discover must not re-fetch it. */
  moodsData: ReturnType<typeof useMoods>;
  onError: (message: string) => void;
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onPlayTrack: (track: Track) => void;
  onPlayScene: (sceneId: number) => void;
}

export function DiscoverView({
  moodsData,
  onError,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onPlayTrack,
  onPlayScene,
}: DiscoverViewProps) {
  const [tab, setTab] = useState<DiscoverTab>("library");
  const discover = useDiscover();

  useEffect(() => {
    if (discover.error) onError(discover.error);
  }, [discover.error, onError]);

  useEffect(() => {
    if (moodsData.error) onError(moodsData.error);
  }, [moodsData.error, onError]);

  return (
    <div className="discover-view">
      <div className="segmented" role="tablist" aria-label="Discover">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "library"}
          className={`segment${tab === "library" ? " is-active" : ""}`}
          onClick={() => setTab("library")}
        >
          Library
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "stats"}
          className={`segment${tab === "stats" ? " is-active" : ""}`}
          onClick={() => setTab("stats")}
        >
          Statistics
        </button>
      </div>

      {tab === "library" ? (
        <LibraryTab
          discover={discover}
          moods={moodsData.moods}
          startingMoodId={startingMoodId}
          startingTrackId={startingTrackId}
          onStartMood={onStartMood}
          onPlayTrack={onPlayTrack}
          onPlayScene={onPlayScene}
          onError={onError}
        />
      ) : (
        <StatsTab stats={discover.stats} moods={moodsData.moods} loading={discover.loading} />
      )}
    </div>
  );
}
