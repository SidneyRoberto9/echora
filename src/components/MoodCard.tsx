import { CATEGORY_COLOR } from "../lib/categories";
import type { MoodSummary } from "../lib/api";

interface MoodCardProps {
  mood: MoodSummary;
  favorited?: boolean;
  loading?: boolean;
  disabled?: boolean;
  selected?: boolean;
  onSelect: (moodId: string) => void;
}

export function MoodCard({ mood, favorited, loading, disabled, selected, onSelect }: MoodCardProps) {
  return (
    <button
      type="button"
      className={`mood-card${favorited ? " is-favorited" : ""}${loading ? " is-loading" : ""}${selected ? " is-selected" : ""}`}
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
