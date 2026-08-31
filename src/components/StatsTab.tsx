import type { ListeningStats, MoodSummary } from "../lib/api";

interface StatsTabProps {
  stats: ListeningStats | null;
  moods: MoodSummary[];
  loading: boolean;
}

function formatDuration(totalSeconds: number): string {
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  if (hours === 0) return `${minutes}m`;
  return `${hours}h ${minutes}m`;
}

const CATEGORY_LABEL: Record<string, string> = {
  power: "Power",
  dark: "Dark",
  love: "Love",
  sad: "Sad",
  "energy-lifestyle": "Energy & Lifestyle",
  cinematic: "Cinematic",
};

export function StatsTab({ stats, moods, loading }: StatsTabProps) {
  if (loading || !stats) {
    return (
      <div className="stats-tab">
        <div className="skeleton" style={{ height: 88, borderRadius: 16 }} />
      </div>
    );
  }

  const topMoodName = stats.top_mood_id
    ? (moods.find((m) => m.id === stats.top_mood_id)?.name ?? "Unknown mood")
    : "—";

  return (
    <div className="stats-tab">
      <div className="stats-grid">
        <div className="stats-card">
          <div className="stats-card__value">{formatDuration(stats.total_seconds_listened)}</div>
          <div className="stats-card__label">Time listened</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{stats.total_sessions}</div>
          <div className="stats-card__label">Sessions</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{stats.total_tracks_played}</div>
          <div className="stats-card__label">Tracks played</div>
        </div>
        <div className="stats-card">
          <div className="stats-card__value">{topMoodName}</div>
          <div className="stats-card__label">Top mood</div>
        </div>
      </div>

      {stats.category_breakdown.length > 0 ? (
        <section className="library-section">
          <h2 className="mood-row__title">By Category</h2>
          {stats.category_breakdown.map((entry) => (
            <div className="library-row library-row--static" key={entry.category}>
              <span className="library-row__title">{CATEGORY_LABEL[entry.category] ?? entry.category}</span>
              <span className="library-row__meta">{entry.session_count} sessions</span>
            </div>
          ))}
        </section>
      ) : null}
    </div>
  );
}
