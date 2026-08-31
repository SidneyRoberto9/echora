import { useCallback, useEffect, useState } from "react";
import { api, type Track } from "../lib/api";

/** `liked === null` means no feedback recorded yet — not "neutral" as a
 * third state to render, just "nothing to show as pressed." */
export function useTrackFeedback(track: Track | null) {
  const [liked, setLiked] = useState<boolean | null>(null);

  // No reset-to-null branch for a missing track: every consumer of this
  // hook already stops rendering the like/dislike buttons once `track` is
  // null, so a stale `liked` value from the previous track is never shown.
  useEffect(() => {
    if (!track) return;
    let cancelled = false;
    api
      .getTrackFeedback(track.id)
      .then((value) => {
        if (!cancelled) setLiked(value);
      })
      .catch(() => {
        // Feedback state is a nice-to-have display detail — a fetch
        // failure here just leaves both buttons unpressed.
      });
    return () => {
      cancelled = true;
    };
    // Keyed on the track id, not the whole `track` object, so a fresh
    // object with the same id (e.g. re-fetched from the queue) doesn't
    // re-trigger this fetch.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [track?.id]);

  const like = useCallback(() => {
    if (!track) return;
    setLiked(true);
    api.setTrackFeedback(track, true).catch(() => {});
  }, [track]);

  const dislike = useCallback(() => {
    if (!track) return;
    setLiked(false);
    api.setTrackFeedback(track, false).catch(() => {});
  }, [track]);

  return { liked, like, dislike };
}
