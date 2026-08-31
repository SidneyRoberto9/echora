import { useEffect, useState } from "react";
import { useDiscover } from "../hooks/useDiscover";
import { useMoods } from "../hooks/useMoods";
import { LibraryTab } from "./LibraryTab";
import { StatsTab } from "./StatsTab";
import type { Track } from "../lib/api";

type DiscoverTab = "library" | "stats";

interface DiscoverViewProps {
  onError: (message: string) => void;
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onPlayTrack: (track: Track) => void;
}

export function DiscoverView({
  onError,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onPlayTrack,
}: DiscoverViewProps) {
  const [tab, setTab] = useState<DiscoverTab>("library");
  const discover = useDiscover();
  const moodsData = useMoods();

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
        />
      ) : (
        <StatsTab stats={discover.stats} moods={moodsData.moods} loading={discover.loading} />
      )}
    </div>
  );
}
