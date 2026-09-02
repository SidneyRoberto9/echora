import { useCallback, useEffect, useState } from "react";
import { api, type QueueView } from "../lib/api";

const EMPTY_QUEUE: QueueView = { current: null, upcoming: [], position: null };

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/**
 * Owns the queue snapshot and playback transport. Rust is still the source
 * of truth for all of this — this hook just fetches it and re-fetches
 * after each mutation, it never invents state Rust doesn't have.
 *
 * Position/duration are polled once a second while a track is loaded and
 * playing. Rust doesn't push playback events yet (see
 * `media::player`'s Fase 3 note on `observe_property`), so a modest poll
 * is the pragmatic stand-in — not the aggressive polling the project
 * brief warns against, and it stops entirely while paused or idle.
 *
 * The one exception is a track finishing on its own: nothing here polls
 * the queue itself, so without a push from Rust the mini-player would
 * keep showing the just-finished track indefinitely (not just "for up to
 * a second") until some other user action happened to call
 * `refreshQueue()`. `api.onTrackAutoAdvanced` covers that one gap.
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

  const setVolume = useCallback(async (percent: number) => {
    setVolumeState(percent);
    try {
      await api.setPlaybackVolume(percent);
    } catch (err) {
      setError(messageOf(err));
    }
  }, []);

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
    dismissError: () => setError(null),
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
