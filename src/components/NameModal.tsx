import { useState } from "react";

interface NameModalProps {
  title: string;
  initialValue?: string;
  onConfirm: (name: string) => void;
  onCancel: () => void;
}

export function NameModal({ title, initialValue = "", onConfirm, onCancel }: NameModalProps) {
  const [value, setValue] = useState(initialValue);
  const [busy, setBusy] = useState(false);
  const trimmed = value.trim();

  const confirm = () => {
    if (busy || trimmed.length === 0) return;
    setBusy(true);
    onConfirm(trimmed);
  };

  return (
    <div className="modal-scrim" onClick={onCancel}>
      <div
        className="modal-panel"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-panel__title">{title}</div>
        <input
          type="text"
          className="modal-input"
          value={value}
          autoFocus
          onChange={(e) => setValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") confirm();
            if (e.key === "Escape") onCancel();
          }}
        />
        <div className="modal-panel__actions">
          <button type="button" className="text-link" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="text-link" disabled={trimmed.length === 0 || busy} onClick={confirm}>
            Save
          </button>
        </div>
      </div>
    </div>
  );
}
