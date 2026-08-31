import { EmptyState } from "./EmptyState";
import { EmptyQueueIcon } from "./icons";
import type { useDiscover } from "../hooks/useDiscover";
import type { MoodSummary, Track } from "../lib/api";

interface LibraryTabProps {
  discover: ReturnType<typeof useDiscover>;
  moods: MoodSummary[];
  startingMoodId: string | null;
  startingTrackId: string | null;
  onStartMood: (moodId: string) => void;
  onPlayTrack: (track: Track) => void;
}

function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}

export function LibraryTab({
  discover,
  moods,
  startingMoodId,
  startingTrackId,
  onStartMood,
  onPlayTrack,
}: LibraryTabProps) {
  const { history, favoriteMoodIds, favoriteTracks, mostPlayedMoods, loading } = discover;
  const moodsById = new Map(moods.map((m) => [m.id, m]));
  const moodName = (moodId: string) => moodsById.get(moodId)?.name ?? "Unknown mood";
  const busy = startingMoodId !== null || startingTrackId !== null;

  if (loading) {
    return (
      <div className="library-tab">
        <div className="skeleton" style={{ height: 44, borderRadius: 14, marginBottom: 8 }} />
        <div className="skeleton" style={{ height: 44, borderRadius: 14 }} />
      </div>
    );
  }

  return (
    <div className="library-tab">
      <section className="library-section">
        <h2 className="mood-row__title">Most Played Moods</h2>
        {mostPlayedMoods.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No moods played yet" />
        ) : (
          mostPlayedMoods.map((entry) => (
            <button
              key={entry.mood_id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(entry.mood_id)}
            >
              <span className="library-row__title">{moodName(entry.mood_id)}</span>
              <span className="library-row__meta">{entry.play_count} sessions</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Favorite Moods</h2>
        {favoriteMoodIds.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No favorited moods yet" />
        ) : (
          favoriteMoodIds.map((moodId) => (
            <button
              key={moodId}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(moodId)}
            >
              <span className="library-row__title">{moodName(moodId)}</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Favorite Tracks</h2>
        {favoriteTracks.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No favorited tracks yet" />
        ) : (
          favoriteTracks.map((track) => (
            <button
              key={track.id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onPlayTrack(track)}
            >
              {startingTrackId === track.id ? (
                <span className="mood-card__spinner" aria-hidden="true" />
              ) : null}
              <span className="library-row__title">{track.title}</span>
              <span className="library-row__meta">{track.artist ?? ""}</span>
            </button>
          ))
        )}
      </section>

      <section className="library-section">
        <h2 className="mood-row__title">Session History</h2>
        {history.length === 0 ? (
          <EmptyState icon={<EmptyQueueIcon />} title="No sessions yet" />
        ) : (
          history.map((session) => (
            <button
              key={session.id}
              type="button"
              className="library-row"
              disabled={busy}
              onClick={() => onStartMood(session.mood_id)}
            >
              <span className="library-row__title">{moodName(session.mood_id)}</span>
              <span className="library-row__meta">
                {formatDate(session.started_at)} · {session.track_count} tracks
              </span>
            </button>
          ))
        )}
      </section>
    </div>
  );
}
