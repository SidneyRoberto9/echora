import { useCallback, useEffect, useState } from "react";
import { api, type Settings } from "../lib/api";

export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .getSettings()
      .then((loaded) => {
        if (!cancelled) setSettings(loaded);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Optimistic: the toggle flips immediately, the write happens in the
  // background — Settings changes are low-stakes enough not to wait on.
  const update = useCallback((patch: Partial<Settings>) => {
    setSettings((prev) => {
      if (!prev) return prev;
      const next = { ...prev, ...patch };
      api.updateSettings(next).catch((err) => {
        setError(err instanceof Error ? err.message : String(err));
      });
      return next;
    });
  }, []);

  return { settings, loading, error, update };
}
