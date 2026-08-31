interface IconProps {
  size?: number;
}

const stroke = {
  fill: "none" as const,
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export const HomeIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M4 11 12 4l8 7" />
    <path d="M6 10v9h12v-9" />
  </svg>
);

export const QueueIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M4 7h16M4 12h16M4 17h10" />
  </svg>
);

export const SettingsIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 13.5a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1 1.55V20a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1-1.55 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.55-1H4a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.55-1 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34H10a1.7 1.7 0 0 0 1-1.55V4a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1 1.55 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87V10a1.7 1.7 0 0 0 1.55 1H20a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.55 1Z" />
  </svg>
);

export const PlayIcon = ({ size = 14 }: IconProps) => (
  <svg width={size * 0.9} height={size} viewBox="0 0 16 18" fill="currentColor" aria-hidden="true">
    <path d="M0 0 16 9 0 18Z" />
  </svg>
);

export const PauseIcon = ({ size = 14 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
    <rect x="5" y="4" width="5" height="16" rx="1.5" />
    <rect x="14" y="4" width="5" height="16" rx="1.5" />
  </svg>
);

export const PreviousIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M19 20 9 12l10-8v16Z" />
    <path d="M6 4v16" />
  </svg>
);

export const NextIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M5 4l10 8-10 8V4Z" />
    <path d="M18 4v16" />
  </svg>
);

export const HeartIcon = ({ size = 17, filled = false }: IconProps & { filled?: boolean }) =>
  filled ? (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M12 21s-7-4.35-9.5-8.5C.6 8.9 2.2 5 6 5c2 0 3.4 1.1 4 2 .6-.9 2-2 4-2 3.8 0 5.4 3.9 3.5 7.5C19 16.65 12 21 12 21Z" />
    </svg>
  ) : (
    <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
      <path d="M20.8 8.6c0 5-8.8 10.4-8.8 10.4S3.2 13.6 3.2 8.6a4.8 4.8 0 0 1 8.8-2.6 4.8 4.8 0 0 1 8.8 2.6Z" />
    </svg>
  );

export const ThumbsDownIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M10 15v4a3 3 0 0 0 3 3l4-9V2H5.7a2 2 0 0 0-2 1.7l-1.3 8A2 2 0 0 0 4.4 14H10Z" />
    <path d="M17 15h3a2 2 0 0 0 2-2V4a2 2 0 0 0-2-2h-3" />
  </svg>
);

export const BackIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M19 12H5M11 18l-6-6 6-6" />
  </svg>
);

export const SparkleIcon = ({ size = 22 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M12 3v3M12 18v3M4.2 4.2l2.1 2.1M17.7 17.7l2.1 2.1M3 12h3M18 12h3M4.2 19.8l2.1-2.1M17.7 6.3l2.1-2.1" />
    <circle cx="12" cy="12" r="3.2" />
  </svg>
);

export const ChevronRightIcon = ({ size = 18 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M9 6l6 6-6 6" />
  </svg>
);

export const CloseIcon = ({ size = 15 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M6 6l12 12M18 6 6 18" />
  </svg>
);

export const WarningIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M12 9v4M12 17h.01" />
    <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z" />
  </svg>
);

export const OfflineIcon = ({ size = 16 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M1 9a15 15 0 0 1 22 0M5 13a9 9 0 0 1 14 0M8.5 16.5a4.5 4.5 0 0 1 7 0" />
    <path d="M2 2l20 20" />
  </svg>
);

export const EmptyQueueIcon = ({ size = 34 }: IconProps) => (
  <svg width={size} height={size} viewBox="0 0 24 24" {...stroke} aria-hidden="true">
    <path d="M4 6h16M4 12h10M4 18h6" />
  </svg>
);
