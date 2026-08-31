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

/** Compact stand-in for a single empty section. The full `EmptyState` is
 * reserved for a wholly empty tab — four of them at 48px of padding each
 * would fill the viewport with near-identical icons on a fresh profile. */
function EmptySection({ message }: { message: string }) {
  return (
    <div className="library-row library-row--static">
      <span className="library-row__meta">{message}</span>
    </div>
  );
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

  if (
    history.length === 0 &&
    favoriteMoodIds.length === 0 &&
    favoriteTracks.length === 0 &&
    mostPlayedMoods.length === 0
  ) {
    return (
      <div className="library-tab">
        <EmptyState
          icon={<EmptyQueueIcon />}
          title="Nothing here yet"
          hint="Play a mood and your history, favorites and rankings show up here."
        />
      </div>
    );
  }

  return (
    <div className="library-tab">
      <section className="library-section">
        <h2 className="mood-row__title">Most Played Moods</h2>
        {mostPlayedMoods.length === 0 ? (
          <EmptySection message="No moods played yet" />
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
          <EmptySection message="No favorited moods yet" />
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
          <EmptySection message="No favorited tracks yet" />
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
          <EmptySection message="No sessions yet" />
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
