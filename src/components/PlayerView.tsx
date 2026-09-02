import { useState } from "react";
import type { Playback } from "../hooks/usePlayback";
import { useTrackFeedback } from "../hooks/useTrackFeedback";
import {
  BackIcon,
  HeartIcon,
  NextIcon,
  PauseIcon,
  PlayIcon,
  PreviousIcon,
  QueueIcon,
  SaveIcon,
  ThumbsDownIcon,
  VolumeIcon,
} from "./icons";
import { EmptyState } from "./EmptyState";
import { NameModal } from "./NameModal";
import { api } from "../lib/api";

interface PlayerViewProps {
  playback: Playback;
  moodName: string | null;
  onCollapse: () => void;
  onOpenQueue: () => void;
  onError: (message: string) => void;
  onSceneSaved: () => void;
}

function formatTime(totalSeconds: number | null): string {
  if (totalSeconds === null || Number.isNaN(totalSeconds)) return "0:00";
  const seconds = Math.max(0, Math.floor(totalSeconds));
  const mins = Math.floor(seconds / 60);
  const secs = seconds % 60;
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export function PlayerView({
  playback,
  moodName,
  onCollapse,
  onOpenQueue,
  onError,
  onSceneSaved,
}: PlayerViewProps) {
  const { queue, isPaused, position, duration, volume, setVolume, playPause, next, previous, seek } =
    playback;
  const track = queue.current;
  const { liked, like, dislike } = useTrackFeedback(track);
  const [showSaveModal, setShowSaveModal] = useState(false);

  const handleSave = async (name: string) => {
    try {
      await api.saveScene(name);
      onSceneSaved();
    } catch (err) {
      onError(err instanceof Error ? err.message : String(err));
    } finally {
      setShowSaveModal(false);
    }
  };

  return (
    <div className="player-view">
      <div className="player-view__glow" aria-hidden="true" />
      <div className="player-view__topbar">
        <button type="button" className="icon-btn" aria-label="Back" onClick={onCollapse}>
          <BackIcon />
        </button>
        {moodName ? (
          <span className="player-view__mood">
            <span className="player-view__mood-dot" aria-hidden="true" />
            {moodName}
          </span>
        ) : (
          <span />
        )}
        <span style={{ display: "flex", gap: 4 }}>
          <button
            type="button"
            className="icon-btn"
            aria-label="Save as Scene"
            disabled={!track}
            onClick={() => setShowSaveModal(true)}
          >
            <SaveIcon />
          </button>
          <button type="button" className="icon-btn" aria-label="Queue" onClick={onOpenQueue}>
            <QueueIcon />
          </button>
        </span>
      </div>

      {!track ? (
        <EmptyState
          icon={<QueueIcon size={34} />}
          title="Nothing playing"
          hint="Pick a mood from Home to start a session"
        />
      ) : (
        <div className="player-view__content">
          <div className="orb-wrap">
            <div className="orb-ring" aria-hidden="true" />
            <div className="orb" aria-hidden="true" />
          </div>

          <div className="track-meta">
            <div className="track-meta__title">{track.title}</div>
            <div className="track-meta__artist">{track.artist ?? "Unknown artist"}</div>
          </div>

          <div className="progress-block">
            <div
              className="progress-track"
              role="slider"
              aria-label="Seek"
              aria-valuemin={0}
              aria-valuemax={duration ?? 0}
              aria-valuenow={position ?? 0}
              tabIndex={0}
              onClick={(e) => {
                if (!duration) return;
                const rect = e.currentTarget.getBoundingClientRect();
                const ratio = (e.clientX - rect.left) / rect.width;
                seek(Math.max(0, Math.min(1, ratio)) * duration);
              }}
              onKeyDown={(e) => {
                if (!duration || !position) return;
                if (e.key === "ArrowRight") seek(Math.min(duration, position + 5));
                if (e.key === "ArrowLeft") seek(Math.max(0, position - 5));
              }}
            >
              <div
                className="progress-fill"
                style={{ width: `${duration ? Math.min(100, ((position ?? 0) / duration) * 100) : 0}%` }}
              />
            </div>
            <div className="progress-times">
              <span>{formatTime(position)}</span>
              <span>{formatTime(duration)}</span>
            </div>
          </div>

          <div className="transport-controls" style={{ gap: 8 }}>
            <button
              type="button"
              className="icon-btn"
              aria-label="Dislike"
              aria-pressed={liked === false}
              style={{ color: liked === false ? "var(--danger)" : "var(--text-secondary)" }}
              onClick={dislike}
            >
              <ThumbsDownIcon />
            </button>
            <button type="button" className="icon-btn" aria-label="Previous" onClick={previous}>
              <PreviousIcon size={20} />
            </button>
            <button
              type="button"
              className="icon-btn"
              aria-label={isPaused ? "Play" : "Pause"}
              style={{
                width: 64,
                height: 64,
                background: "var(--accent)",
                color: "var(--bg-base)",
                boxShadow: "0 0 28px var(--accent-glow)",
                margin: "0 6px",
              }}
              onClick={playPause}
            >
              {isPaused ? <PlayIcon size={20} /> : <PauseIcon size={20} />}
            </button>
            <button type="button" className="icon-btn" aria-label="Next" onClick={next}>
              <NextIcon size={20} />
            </button>
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

          <div className="player-view__volume">
            <VolumeIcon size={16} />
            <input
              type="range"
              min={0}
              max={100}
              value={volume}
              onChange={(e) => setVolume(Number(e.target.value))}
              aria-label="Volume"
              className="player-view__volume-slider"
            />
          </div>
        </div>
      )}

      {queue.upcoming[0] ? (
        <div className="player-view__footer">
          <span>Up next · {queue.upcoming[0].title}</span>
        </div>
      ) : null}

      {showSaveModal ? (
        <NameModal title="Save as scene" onConfirm={handleSave} onCancel={() => setShowSaveModal(false)} />
      ) : null}
    </div>
  );
}
