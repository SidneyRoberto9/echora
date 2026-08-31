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
  onSelect: (moodId: string) => void;
}

export function MoodCard({ mood, favorited, onSelect }: MoodCardProps) {
  return (
    <button
      type="button"
      className={`mood-card${favorited ? " is-favorited" : ""}`}
      onClick={() => onSelect(mood.id)}
    >
      <span
        className="mood-card__dot"
        style={{ background: CATEGORY_COLOR[mood.category] ?? "oklch(70% 0.05 290)" }}
        aria-hidden="true"
      />
      <span className="mood-card__name">{mood.name}</span>
    </button>
  );
}
