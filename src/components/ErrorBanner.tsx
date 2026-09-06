import { CloseIcon, WarningIcon } from "./icons";

interface ErrorBannerProps {
  message: string;
  onDismiss: () => void;
}

/** A quiet inline banner for recoverable errors — never a blocking popup,
 * per the product brief's "don't interrupt the user for recoverable errors."
 * `onDismiss` is a real `<button>`, so it's reachable and activatable by
 * keyboard (Tab, then Enter/Space) with no extra wiring needed. */
export function ErrorBanner({ message, onDismiss }: ErrorBannerProps) {
  return (
    <div className="banner is-error" role="alert">
      <WarningIcon size={16} />
      <span style={{ flex: 1 }}>{message}</span>
      <button
        type="button"
        className="icon-btn"
        aria-label="Dismiss"
        style={{ width: 28, height: 28 }}
        onClick={onDismiss}
      >
        <CloseIcon size={14} />
      </button>
    </div>
  );
}
