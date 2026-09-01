import { useCallback } from "react";
import type { MoodSummary } from "../lib/api";

interface MoodMixBarProps {
  moods: MoodSummary[];
  weights: number[];
  onChangeWeights: (weights: number[]) => void;
  onStart: () => void;
  busy: boolean;
}

export function MoodMixBar({ moods, weights, onChangeWeights, onStart, busy }: MoodMixBarProps) {
  const handleFirstChange = useCallback(
    (value: number) => {
      if (moods.length === 2) {
        onChangeWeights([value, 100 - value]);
        return;
      }
      const remainder = 100 - value;
      const previousRemainder = weights[1] + weights[2];
      const secondShare = previousRemainder > 0 ? weights[1] / previousRemainder : 0.5;
      const second = Math.max(1, Math.min(remainder - 1, Math.round(remainder * secondShare)));
      onChangeWeights([value, second, remainder - second]);
    },
    [moods.length, weights, onChangeWeights],
  );

  const handleSecondChange = useCallback(
    (value: number) => {
      const remainder = 100 - weights[0];
      onChangeWeights([weights[0], value, remainder - value]);
    },
    [weights, onChangeWeights],
  );

  return (
    <div className="mood-mix-bar">
      <div className="mood-mix-bar__chips">
        {moods.map((mood, i) => (
          <span className="mood-mix-bar__chip" key={mood.id}>
            {mood.name} · {weights[i]}%
          </span>
        ))}
      </div>

      <input
        type="range"
        min={1}
        max={moods.length === 2 ? 99 : 98}
        value={weights[0]}
        disabled={busy}
        onChange={(e) => handleFirstChange(Number(e.target.value))}
        aria-label={`${moods[0]?.name ?? "first mood"} weight`}
        className="mood-mix-bar__slider"
      />
      {moods.length === 3 ? (
        <input
          type="range"
          min={1}
          max={100 - weights[0] - 1}
          value={weights[1]}
          disabled={busy}
          onChange={(e) => handleSecondChange(Number(e.target.value))}
          aria-label={`${moods[1]?.name ?? "second mood"} vs ${moods[2]?.name ?? "third mood"} split`}
          className="mood-mix-bar__slider"
        />
      ) : null}

      <button type="button" className="mood-mix-bar__start" disabled={busy} onClick={onStart}>
        {busy ? "Starting…" : "Start Mix"}
      </button>
    </div>
  );
}
