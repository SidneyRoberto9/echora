import type { ReactNode } from "react";

interface EmptyStateProps {
  icon: ReactNode;
  title: string;
  hint?: string;
}

export function EmptyState({ icon, title, hint }: EmptyStateProps) {
  return (
    <div className="empty-state" role="status">
      {icon}
      <div className="empty-state__title">{title}</div>
      {hint ? <div className="empty-state__hint">{hint}</div> : null}
    </div>
  );
}
