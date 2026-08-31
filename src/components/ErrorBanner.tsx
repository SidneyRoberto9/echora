import { WarningIcon } from "./icons";

interface ErrorBannerProps {
  message: string;
}

/** A quiet inline banner for recoverable errors — never a blocking popup,
 * per the product brief's "don't interrupt the user for recoverable errors." */
export function ErrorBanner({ message }: ErrorBannerProps) {
  return (
    <div className="banner is-error" role="alert">
      <WarningIcon size={16} />
      <span>{message}</span>
    </div>
  );
}
