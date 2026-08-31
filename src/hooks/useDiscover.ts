import { useEffect, useState } from "react";
import {
  api,
  type ListeningStats,
  type MoodPlayCount,
  type SessionSummary,
  type Track,
} from "../lib/api";

function messageOf(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/**
 * Fetches everything Discover needs, once on mount — matches
 * `useSettings`/`useMoods`'s existing fetch-on-mount pattern. Rust stays
 * the source of truth; this hook never invents data it doesn't have.
 */
export function useDiscover() {
  const [history, setHistory] = useState<SessionSummary[]>([]);
  const [favoriteMoodIds, setFavoriteMoodIds] = useState<string[]>([]);
  const [favoriteTracks, setFavoriteTracks] = useState<Track[]>([]);
  const [mostPlayedMoods, setMostPlayedMoods] = useState<MoodPlayCount[]>([]);
  const [stats, setStats] = useState<ListeningStats | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [historyList, favoriteMoods, favTracks, mostPlayed, listeningStats] = await Promise.all([
          api.listHistory(20, 0),
          api.listFavoriteMoods(),
          api.listFavoriteTracks(),
          api.listMostPlayedMoods(),
          api.getListeningStats(),
        ]);
        if (!cancelled) {
          setHistory(historyList);
          setFavoriteMoodIds(favoriteMoods);
          setFavoriteTracks(favTracks);
          setMostPlayedMoods(mostPlayed);
          setStats(listeningStats);
        }
      } catch (err) {
        if (!cancelled) setError(messageOf(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return { history, favoriteMoodIds, favoriteTracks, mostPlayedMoods, stats, loading, error };
}
