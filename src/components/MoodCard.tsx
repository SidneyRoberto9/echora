import type { MoodSummary } from "../lib/api";

const CATEGORY_COLOR: Record<string, string> = {
  power: "oklch(75% 0.15 300)",
  dark: "oklch(55% 0.05 290)",
  love: "oklch(72% 0.12 340)",
  sad: "oklch(65% 0.05 250)",
  "energy-lifestyle": "oklch(70% 0.16 20)",
  cinematic: "oklch(72% 0.14 250)",
};

interface MoodCardProps {
  mood: MoodSummary;
  favorited?: boolean;
  loading?: boolean;
  disabled?: boolean;
  onSelect: (moodId: string) => void;
}

export function MoodCard({ mood, favorited, loading, disabled, onSelect }: MoodCardProps) {
  return (
    <button
      type="button"
      className={`mood-card${favorited ? " is-favorited" : ""}${loading ? " is-loading" : ""}`}
      disabled={disabled}
      onClick={() => onSelect(mood.id)}
    >
      {loading ? (
        <span className="mood-card__spinner" aria-hidden="true" />
      ) : (
        <span
          className="mood-card__dot"
          style={{ background: CATEGORY_COLOR[mood.category] ?? "oklch(70% 0.05 290)" }}
          aria-hidden="true"
        />
      )}
      <span className="mood-card__name">{mood.name}</span>
    </button>
  );
}
