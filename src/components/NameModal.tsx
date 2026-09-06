import { useEffect, useRef, useState, type KeyboardEvent } from "react";

interface NameModalProps {
  title: string;
  initialValue?: string;
  onConfirm: (name: string) => void;
  onCancel: () => void;
}

const FOCUSABLE_SELECTOR = 'button:not(:disabled), input:not(:disabled), [href], [tabindex]:not([tabindex="-1"])';

export function NameModal({ title, initialValue = "", onConfirm, onCancel }: NameModalProps) {
  const [value, setValue] = useState(initialValue);
  const [busy, setBusy] = useState(false);
  const trimmed = value.trim();
  const panelRef = useRef<HTMLDivElement>(null);
  // Captured during render, before this modal's own autoFocus can move
  // focus — this is the element that opened the modal, restored on close.
  const previouslyFocused = useRef(document.activeElement instanceof HTMLElement ? document.activeElement : null);

  useEffect(() => {
    const toRestore = previouslyFocused.current;
    return () => {
      toRestore?.focus();
    };
  }, []);

  const confirm = () => {
    if (busy || trimmed.length === 0) return;
    setBusy(true);
    onConfirm(trimmed);
  };

  const trapFocus = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      onCancel();
      return;
    }
    if (e.key !== "Tab" || !panelRef.current) return;
    const focusable = panelRef.current.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR);
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  };

  return (
    <div className="modal-scrim" onClick={onCancel}>
      <div
        ref={panelRef}
        className="modal-panel"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={trapFocus}
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
