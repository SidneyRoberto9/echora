import { useState } from "react";
import type { Playback } from "../hooks/usePlayback";
import { CloseIcon, EmptyQueueIcon, PlayIcon, SaveIcon } from "./icons";
import { EmptyState } from "./EmptyState";
import { NameModal } from "./NameModal";
import { api } from "../lib/api";

interface QueueViewProps {
  playback: Playback;
  onError: (message: string) => void;
  onSceneSaved: () => void;
}

function formatDuration(seconds: number | null): string {
  if (seconds === null) return "";
  const mins = Math.floor(seconds / 60);
  const secs = Math.floor(seconds % 60);
  return `${mins}:${secs.toString().padStart(2, "0")}`;
}

export function QueueView({ playback, onError, onSceneSaved }: QueueViewProps) {
  const { queue, skipTo, remove } = playback;
  const [showSaveModal, setShowSaveModal] = useState(false);

  const hasQueue = queue.current !== null || queue.upcoming.length > 0;

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

  if (!hasQueue) {
    return (
      <EmptyState
        icon={<EmptyQueueIcon />}
        title="Nothing queued yet"
        hint="Pick a mood to start a session"
      />
    );
  }

  return (
    <div className="queue-view">
      <div style={{ display: "flex", justifyContent: "flex-end", marginBottom: 4 }}>
        <button
          type="button"
          className="text-link"
          disabled={!hasQueue}
          onClick={() => setShowSaveModal(true)}
        >
          <SaveIcon size={14} /> Save as Scene
        </button>
      </div>

      {queue.current ? (
        <>
          <h2 className="queue-view__label">Now Playing</h2>
          <div className="queue-row is-current">
            <span className="queue-row__art" aria-hidden="true" />
            <span className="queue-row__meta">
              <div className="queue-row__title">{queue.current.title}</div>
              <div className="queue-row__artist">{queue.current.artist ?? "Unknown artist"}</div>
            </span>
            <PlayIcon size={12} />
            <span className="queue-row__duration">{formatDuration(queue.current.duration_seconds)}</span>
          </div>
        </>
      ) : null}

      {queue.upcoming.length > 0 ? (
        <>
          <h2 className="queue-view__label" style={{ marginTop: 18 }}>
            Up Next
          </h2>
          {queue.upcoming.map((track, i) => {
            const absoluteIndex = (queue.position ?? -1) + 1 + i;
            return (
              <div className="queue-row" key={`${track.id}-${i}`}>
                <button
                  type="button"
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: 14,
                    flex: 1,
                    minWidth: 0,
                    padding: 0,
                    background: "none",
                    border: "none",
                    textAlign: "left",
                    color: "inherit",
                    font: "inherit",
                    cursor: "pointer",
                  }}
                  onClick={() => skipTo(absoluteIndex)}
                  aria-label={`Play ${track.title}`}
                >
                  <span className="queue-row__art" aria-hidden="true" />
                  <span className="queue-row__meta">
                    <div className="queue-row__title">{track.title}</div>
                    <div className="queue-row__artist">{track.artist ?? "Unknown artist"}</div>
                  </span>
                </button>
                <span className="queue-row__duration">{formatDuration(track.duration_seconds)}</span>
                <button
                  type="button"
                  className="queue-row__remove"
                  aria-label={`Remove ${track.title} from queue`}
                  onClick={() => remove(absoluteIndex)}
                >
                  <CloseIcon />
                </button>
              </div>
            );
          })}
        </>
      ) : null}

      {showSaveModal ? (
        <NameModal title="Save as scene" onConfirm={handleSave} onCancel={() => setShowSaveModal(false)} />
      ) : null}
    </div>
  );
}
