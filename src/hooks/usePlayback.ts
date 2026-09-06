import { useCallback, useEffect, useRef, useState } from "react";
import { api, type QueueView, type SkippedTrack } from "../lib/api";

const EMPTY_QUEUE: QueueView = { current: null, upcoming: [], position: null };

// How long a volume slider drag can go quiet before the setting actually
// gets saved to SQLite (P1-4) — the on-screen slider and mpv's real volume
// are never delayed by this, only the persisted preference is.
const VOLUME_PERSIST_DEBOUNCE_MS = 400;

const SKIPPED_REASON_LABEL: Record<SkippedTrack["reason"], string> = {
  private: "private",
  region_blocked: "blocked in your region",
  removed: "removed",
  unknown: "unavailable",
};

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function trackUnavailableMessage(tracks: SkippedTrack[]): string {
  if (tracks.length === 1) {
    const [track] = tracks;
    return `Skipped "${track.title}" (${SKIPPED_REASON_LABEL[track.reason]}).`;
  }
  return `Skipped ${tracks.length} unavailable tracks: ${tracks.map((t) => t.title).join(", ")}.`;
}

/**
 * Owns the queue snapshot and playback transport. Rust is still the source
 * of truth for all of this — this hook just fetches it and re-fetches
 * after each mutation, it never invents state Rust doesn't have.
 *
 * Position/duration are polled once a second while a track is loaded and
 * playing. Rust itself now observes both continuously over mpv's IPC
 * connection (P2-1 — see `media::player`), but neither
 * `get_playback_position`/`get_playback_duration` below nor this poll
 * changed as part of that: they're already just in-process `Tauri::State`
 * reads of Rust's own cache (no socket, effectively free), and pushing
 * sub-second position to the frontend would mean extending
 * `platform::mpris::PLAYBACK_CHANGED_EVENT`'s payload with a value that
 * changes every tick — turning an event meant for actual state
 * transitions into a de facto second poll, just renamed. A 1Hz pull from
 * here is the simpler thing that does the same job, and it stops entirely
 * while paused or idle.
 *
 * Everything else that can change playback/queue state from outside this
 * hook's own calls — a track finishing on its own, the tray menu, MPRIS/
 * media keys — is pushed, not polled: `api.onPlaybackChanged` (queue +
 * paused state) and `api.onTrackAutoAdvanced` (the one transition that
 * additionally needs a local reset of position/paused before its own
 * `playback-changed` arrives) cover that.
 */
export function usePlayback() {
  const [queue, setQueue] = useState<QueueView>(EMPTY_QUEUE);
  const [queueLoaded, setQueueLoaded] = useState(false);
  const [isPaused, setIsPaused] = useState(false);
  const [position, setPosition] = useState<number | null>(null);
  const [duration, setDuration] = useState<number | null>(null);
  const [volume, setVolumeState] = useState(100);
  const [error, setError] = useState<string | null>(null);

  const refreshQueue = useCallback(async () => {
    try {
      const view = await api.getQueue();
      setQueue(view);
    } catch (err) {
      setError(messageOf(err));
    } finally {
      setQueueLoaded(true);
    }
  }, []);

  // A local async closure, not a direct call to the memoized `refreshQueue`
  // — keeps the initial fetch out of `refreshQueue`'s dependency chain so
  // this only ever runs once, on mount.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const view = await api.getQueue();
        if (!cancelled) setQueue(view);
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      } finally {
        if (!cancelled) setQueueLoaded(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Rust pushes this the moment it auto-advances the queue for a track
  // that finished on its own -- resets local playback state and refetches
  // the queue exactly like a manual `next()` does, since from the
  // frontend's point of view it's the same transition, just not
  // user-triggered.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const stop = await api.onTrackAutoAdvanced(() => {
        setIsPaused(false);
        setPosition(0);
        void refreshQueue();
      });
      if (cancelled) {
        stop();
      } else {
        unlisten = stop;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [refreshQueue]);

  // Rust pushes this on every playback/queue mutation, wherever it came
  // from -- including outside the frontend's own IPC calls (tray menu,
  // MPRIS/media keys), which otherwise wouldn't show up here until the
  // next 1Hz poll (P1-1). The payload already carries the fresh queue and
  // paused state, so this never needs a follow-up round-trip.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const stop = await api.onPlaybackChanged((payload) => {
        setQueue(payload.queue);
        setIsPaused(payload.is_paused);
        setQueueLoaded(true);
      });
      if (cancelled) {
        stop();
      } else {
        unlisten = stop;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Rust pushes this whenever an advance had to skip one or more dead
  // tracks (private/removed/region-blocked) -- the only feedback the user
  // gets that something in their queue was silently dropped, including the
  // case where nothing at all ended up playing. All skipped tracks from
  // one advance arrive together in a single event, so this never spams one
  // banner per track.
  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    (async () => {
      const stop = await api.onTrackUnavailable((tracks) => {
        if (tracks.length > 0) setError(trackUnavailableMessage(tracks));
      });
      if (cancelled) {
        stop();
      } else {
        unlisten = stop;
      }
    })();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Seeds the volume slider from the last saved value -- runs once, same
  // reasoning as the queue's own initial-fetch effect above.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const settings = await api.getSettings();
        if (!cancelled) setVolumeState(settings.volume);
      } catch {
        // Falls back to the 100 default already in state -- not worth
        // surfacing as a playback error.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!queue.current || isPaused) return;
    let cancelled = false;

    const tick = async () => {
      try {
        const [pos, dur] = await Promise.all([api.getPlaybackPosition(), api.getPlaybackDuration()]);
        if (!cancelled) {
          setPosition(pos);
          setDuration(dur);
        }
      } catch {
        // A transient IPC hiccup here isn't worth surfacing — the next
        // tick retries on its own.
      }
    };

    tick();
    const id = window.setInterval(tick, 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
    // Deliberately keyed on the track id, not the whole `queue` object —
    // `queue` gets a new reference on every fetch, which would restart
    // this interval far more often than the track actually changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [queue.current?.id, isPaused]);

  const playPause = useCallback(async () => {
    try {
      if (isPaused) {
        await api.resumePlayback();
      } else {
        await api.pausePlayback();
      }
      setIsPaused((paused) => !paused);
    } catch (err) {
      setError(messageOf(err));
    }
  }, [isPaused]);

  const next = useCallback(async () => {
    try {
      await api.queueNext();
      setIsPaused(false);
      setPosition(0);
      await refreshQueue();
    } catch (err) {
      setError(messageOf(err));
    }
  }, [refreshQueue]);

  const previous = useCallback(async () => {
    try {
      const track = await api.queuePrevious();
      if (!track) {
        await api.seekPlayback(0);
      }
      setIsPaused(false);
      setPosition(0);
      await refreshQueue();
    } catch (err) {
      setError(messageOf(err));
    }
  }, [refreshQueue]);

  const skipTo = useCallback(
    async (index: number) => {
      try {
        await api.queueSkipTo(index);
        setIsPaused(false);
        setPosition(0);
        await refreshQueue();
      } catch (err) {
        setError(messageOf(err));
      }
    },
    [refreshQueue],
  );

  const remove = useCallback(
    async (index: number) => {
      try {
        await api.queueRemove(index);
        await refreshQueue();
      } catch (err) {
        setError(messageOf(err));
      }
    },
    [refreshQueue],
  );

  const seek = useCallback(async (seconds: number) => {
    try {
      await api.seekPlayback(seconds);
      setPosition(seconds);
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);

  // Ref, not state -- this is a timer handle, not something that should
  // ever trigger a re-render.
  const volumePersistTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => window.clearTimeout(volumePersistTimer.current), []);

  const setVolume = useCallback((percent: number) => {
    // Visual + mpv's real volume apply on every call, immediately -- only
    // the SQLite write is debounced (P1-4). A drag fires this once per
    // tick (~100 times), which would otherwise be ~100 SQLite writes.
    setVolumeState(percent);
    api.setPlaybackVolume(percent, false).catch((err) => setError(messageOf(err)));

    window.clearTimeout(volumePersistTimer.current);
    volumePersistTimer.current = window.setTimeout(() => {
      api.setPlaybackVolume(percent, true).catch((err) => setError(messageOf(err)));
    }, VOLUME_PERSIST_DEBOUNCE_MS);
  }, []);

  const dismissError = useCallback(() => setError(null), []);

  return {
    queue,
    queueLoaded,
    isPaused,
    setIsPaused,
    position,
    duration,
    volume,
    setVolume,
    error,
    dismissError,
    refreshQueue,
    playPause,
    next,
    previous,
    skipTo,
    remove,
    seek,
  };
}

export type Playback = ReturnType<typeof usePlayback>;
