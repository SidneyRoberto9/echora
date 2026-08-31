import type { Playback } from "../hooks/usePlayback";
import { useTrackFeedback } from "../hooks/useTrackFeedback";
import { HeartIcon, NextIcon, PauseIcon, PlayIcon, PreviousIcon } from "./icons";

interface MiniPlayerBarProps {
  playback: Playback;
  onExpand: () => void;
}

export function MiniPlayerBar({ playback, onExpand }: MiniPlayerBarProps) {
  const { queue, isPaused, position, duration, playPause, next, previous } = playback;
  const track = queue.current;
  const { liked, like } = useTrackFeedback(track);
  if (!track) return null;

  const progressPct = duration && duration > 0 ? Math.min(100, ((position ?? 0) / duration) * 100) : 0;

  return (
    <div className="mini-player">
      <div className="mini-player__progress-track">
        <div className="mini-player__progress-fill" style={{ width: `${progressPct}%` }} />
      </div>
      <div className="mini-player__row">
        <button type="button" className="mini-player__info" onClick={onExpand} aria-label="Expand player">
          <span className="mini-player__orb" aria-hidden="true" />
          <span style={{ minWidth: 0, textAlign: "left" }}>
            <span className="mini-player__title">{track.title}</span>
            <br />
            <span className="mini-player__subtitle">{track.artist ?? "Unknown artist"}</span>
          </span>
        </button>

        <div className="transport-controls">
          <button type="button" className="icon-btn" aria-label="Previous" onClick={previous}>
            <PreviousIcon />
          </button>
          <button
            type="button"
            className="icon-btn play-btn"
            aria-label={isPaused ? "Play" : "Pause"}
            style={{ color: "var(--bg-base)" }}
            onClick={playPause}
          >
            {isPaused ? <PlayIcon size={16} /> : <PauseIcon size={16} />}
          </button>
          <button type="button" className="icon-btn" aria-label="Next" onClick={next}>
            <NextIcon />
          </button>
        </div>

        <button
          type="button"
          className="icon-btn"
          aria-label={liked === true ? "Liked" : "Like"}
          aria-pressed={liked === true}
          style={{ color: liked === true ? "var(--accent)" : "var(--text-secondary)" }}
          onClick={like}
        >
          <HeartIcon filled={liked === true} />
        </button>
      </div>
    </div>
  );
}
