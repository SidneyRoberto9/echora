import { useEffect, useState } from "react";
import { api, type MoodSummary } from "../lib/api";

export function useMoods() {
  const [moods, setMoods] = useState<MoodSummary[]>([]);
  const [favoriteMoodIds, setFavoriteMoodIds] = useState<Set<string>>(new Set());
  const [recentMoodIds, setRecentMoodIds] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [list, favorites, history] = await Promise.all([
          api.listMoods(),
          api.listFavoriteMoods(),
          api.listHistory(10, 0),
        ]);
        if (!cancelled) {
          setMoods(list);
          const favoriteSet = new Set(favorites);
          setFavoriteMoodIds(favoriteSet);
          const recents: string[] = [];
          for (const session of history) {
            for (const { mood_id } of session.moods) {
              if (!favoriteSet.has(mood_id) && !recents.includes(mood_id)) {
                recents.push(mood_id);
              }
            }
          }
          setRecentMoodIds(recents.slice(0, 4));
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return { moods, favoriteMoodIds, recentMoodIds, loading, error };
}
