/** Display metadata for the bundled mood categories, shared by every view
 * that renders a category (HomeView's rows, MoodCard's dot, StatsTab's
 * breakdown) so adding a category means editing one file, not three. */

export const CATEGORY_LABEL: Record<string, string> = {
  power: "Power",
  dark: "Dark",
  love: "Love",
  sad: "Sad",
  "energy-lifestyle": "Energy & Lifestyle",
  cinematic: "Cinematic",
};

export const CATEGORY_COLOR: Record<string, string> = {
  power: "oklch(75% 0.15 300)",
  dark: "oklch(55% 0.05 290)",
  love: "oklch(72% 0.12 340)",
  sad: "oklch(65% 0.05 250)",
  "energy-lifestyle": "oklch(70% 0.16 20)",
  cinematic: "oklch(72% 0.14 250)",
};
