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

  // Optimistic: the toggle flips immediately in the UI, but the value
  // that sticks is whatever Rust's `update_settings` merges the patch
  // onto -- not a local read-modify-write of a copy loaded at mount time,
  // which is exactly the lost-update bug a partial-patch command exists
  // to avoid (e.g. the volume changed via the player slider in between).
  const update = useCallback((patch: Partial<Settings>) => {
    setSettings((prev) => (prev ? { ...prev, ...patch } : prev));
    api
      .updateSettings(patch)
      .then((merged) => setSettings(merged))
      .catch((err) => {
        setError(err instanceof Error ? err.message : String(err));
      });
  }, []);

  return { settings, loading, error, update };
}
