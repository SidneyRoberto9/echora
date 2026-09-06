import { useCallback, useEffect, useState } from "react";
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
  // Destructured out, not read as `playback.dismissError` below: `playback`
  // itself gets a new object reference on every poll tick while a track is
  // playing, but the function this property holds is stable (empty-deps
  // `useCallback` in `usePlayback`) -- referencing the stable function
  // directly, instead of through the churning object, keeps
  // `dismissDisplayedError` itself stable, so the auto-dismiss effect below
  // doesn't reset its timer every second during playback.
  const { dismissError: dismissPlaybackError } = playback;

  const reportError = useCallback((message: string) => setGlobalError(message), []);
  const onSceneSaved = useCallback(() => setSceneSaveTick((t) => t + 1), []);

  // usePlayback owns its own transient `error` (failed pause/seek/volume
  // IPC calls, plus the `track-unavailable` message) but has no banner of
  // its own — merged at render time (not synced through an effect, which
  // would just be React state mirroring React state) into the same global
  // banner every other view already reports into via `onError`.
  const displayedError = playback.error ?? globalError;
  const dismissDisplayedError = useCallback(() => {
    setGlobalError(null);
    dismissPlaybackError();
  }, [dismissPlaybackError]);

  // A transient error shouldn't sit on screen forever waiting for some
  // unrelated action to overwrite it -- auto-dismiss, restarting the timer
  // whenever a new message replaces the old one.
  useEffect(() => {
    if (!displayedError) return;
    const id = window.setTimeout(dismissDisplayedError, 6000);
    return () => window.clearTimeout(id);
  }, [displayedError, dismissDisplayedError]);

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

      {displayedError ? (
        <div style={{ paddingTop: 12 }}>
          <ErrorBanner message={displayedError} onDismiss={dismissDisplayedError} />
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
