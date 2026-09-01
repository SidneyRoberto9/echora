import { useCallback, useState } from "react";
import { TopBar } from "./components/TopBar";
import { HomeView } from "./components/HomeView";
import { QueueView } from "./components/QueueView";
import { SettingsView } from "./components/SettingsView";
import { MiniPlayerBar } from "./components/MiniPlayerBar";
import { PlayerView } from "./components/PlayerView";
import { ErrorBanner } from "./components/ErrorBanner";
import { DiscoverView } from "./components/DiscoverView";
import { usePlayback } from "./hooks/usePlayback";
import { useMoods } from "./hooks/useMoods";
import { api } from "./lib/api";
import type { SessionMood, Track } from "./lib/api";

export type View = "home" | "queue" | "discover" | "settings";

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function App() {
  const [view, setView] = useState<View>("home");
  const [playerExpanded, setPlayerExpanded] = useState(false);
  const [startingMoodId, setStartingMoodId] = useState<string | null>(null);
  const [startingTrackId, setStartingTrackId] = useState<string | null>(null);
  const [currentMoods, setCurrentMoods] = useState<SessionMood[] | null>(null);
  const [globalError, setGlobalError] = useState<string | null>(null);
  const [sceneSaveTick, setSceneSaveTick] = useState(0);

  const playback = usePlayback();
  const moodsData = useMoods();

  const reportError = useCallback((message: string) => setGlobalError(message), []);
  const onSceneSaved = useCallback(() => setSceneSaveTick((t) => t + 1), []);

  const handleStartMood = useCallback(
    async (moodId: string) => {
      setStartingMoodId(moodId);
      try {
        const session = await api.startMoodSession(moodId);
        setCurrentMoods(session.moods);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingMoodId(null);
      }
    },
    [playback, reportError],
  );

  const handleStartMix = useCallback(
    async (moods: SessionMood[]) => {
      setStartingMoodId("mix");
      try {
        const session = await api.startMixedSession(moods);
        setCurrentMoods(session.moods);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingMoodId(null);
      }
    },
    [playback, reportError],
  );

  const handlePlayTrack = useCallback(
    async (track: Track) => {
      setStartingTrackId(track.id);
      try {
        await api.playSingleTrack(track);
        setCurrentMoods(null);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingTrackId(null);
      }
    },
    [playback, reportError],
  );

  const handlePlayScene = useCallback(
    async (sceneId: number) => {
      setStartingTrackId(`scene-${sceneId}`);
      try {
        await api.playScene(sceneId);
        setCurrentMoods(null);
        await playback.refreshQueue();
        setPlayerExpanded(true);
      } catch (err) {
        reportError(messageOf(err));
      } finally {
        setStartingTrackId(null);
      }
    },
    [playback, reportError],
  );

  const handleSurpriseMe = useCallback(async () => {
    setStartingMoodId("surprise");
    try {
      const session = await api.surpriseMe();
      setCurrentMoods(session.moods);
      await playback.refreshQueue();
      setPlayerExpanded(true);
    } catch (err) {
      reportError(messageOf(err));
    } finally {
      setStartingMoodId(null);
    }
  }, [playback, reportError]);

  const currentMoodName = currentMoods
    ? currentMoods
        .map((m) => moodsData.moods.find((mood) => mood.id === m.mood_id)?.name ?? "Unknown mood")
        .join(" + ")
    : null;

  return (
    <div className="app-shell">
      <TopBar view={view} onChangeView={setView} />

      {globalError ? (
        <div style={{ paddingTop: 12 }}>
          <ErrorBanner message={globalError} />
        </div>
      ) : null}

      <div className="view-content">
        {view === "home" ? (
          <HomeView
            moodsData={moodsData}
            onError={reportError}
            startingMoodId={startingMoodId}
            onStartMood={handleStartMood}
            onStartMix={handleStartMix}
            onSurpriseMe={handleSurpriseMe}
          />
        ) : null}
        {view === "queue" ? (
          <QueueView playback={playback} onError={reportError} onSceneSaved={onSceneSaved} />
        ) : null}
        {view === "discover" ? (
          <DiscoverView
            moodsData={moodsData}
            onError={reportError}
            startingMoodId={startingMoodId}
            startingTrackId={startingTrackId}
            onStartMood={handleStartMood}
            onStartMix={handleStartMix}
            onPlayTrack={handlePlayTrack}
            onPlayScene={handlePlayScene}
            sceneSaveTick={sceneSaveTick}
          />
        ) : null}
        {view === "settings" ? <SettingsView onError={reportError} /> : null}
      </div>

      {playback.queue.current ? (
        <MiniPlayerBar playback={playback} onExpand={() => setPlayerExpanded(true)} />
      ) : null}

      {playerExpanded ? (
        <PlayerView
          playback={playback}
          moodName={currentMoodName}
          onCollapse={() => setPlayerExpanded(false)}
          onOpenQueue={() => {
            setPlayerExpanded(false);
            setView("queue");
          }}
          onError={reportError}
          onSceneSaved={onSceneSaved}
        />
      ) : null}
    </div>
  );
}

export default App;
