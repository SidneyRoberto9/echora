# Design

Direction: **Ambient Glow** — soft, immersive, dark-only. Chosen from three
explored directions (Terminal Ritual, Ambient Glow, Editorial Sharp); the
full comparison and the built-out screens live in the design canvas:
<https://claude.ai/code/artifact/d7eb72e6-96f6-457c-b6c7-cab4d2e6365f>
(Home, Player, Queue, Settings, component states).

This file is the source of truth for implementation — the canvas is a
reference, not a spec that updates itself.

## Typography

- **Sora** (Google Fonts, weights 400/500/600/700) for all UI text.
- Fallback stack: `ui-sans-serif, system-ui, sans-serif`.

## Color tokens (oklch)

| Token | Value | Use |
|---|---|---|
| `--bg-base` | `oklch(15% 0.015 290)` | App background |
| `--bg-elevated` | `oklch(19% 0.015 290)` | Cards, mood tiles |
| `--bg-elevated-hover` | `oklch(23% 0.02 290)` | Card hover |
| `--border` | `oklch(24%–26% 0.015–0.02 290)` | Dividers, top/bottom bar borders |
| `--text-primary` | `oklch(95% 0.01 290)` | Titles, primary labels |
| `--text-secondary` | `oklch(75% 0.03 290)` | Artist names, subtitles |
| `--text-tertiary` | `oklch(58%–65% 0.02 290)` | Section labels, timestamps |
| `--accent` | `oklch(78% 0.16 300)` | Play button, active state, progress fill, like icon |
| `--accent-glow` | `oklch(70% 0.16–0.18 300 / 0.35–0.55)` | Soft radial glow only — never a full-background gradient wash |
| `--danger` | `oklch(70% 0.15 25)` | Destructive actions only ("Clear all history"), never a normal idle icon |

Rule: the purple accent is a **precise signal**, not a decoration. It marks
what's active, playing, or actionable — it never washes the whole screen.

## Shape & spacing

- Radius scale: `10px` (small controls, icon buttons) · `12–14px` (rows,
  small cards) · `16–18px` (mood cards, banners) · `28px` (a full-screen
  panel corner, used sparingly).
- Icon buttons: circular for transport controls, `10px`-radius squares for
  top-bar navigation icons.
- Spacing rhythm: `4, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32` — pick from
  this scale, don't invent arbitrary values.

## Icons

Inline stroke-based SVG only — no emoji, no icon fonts. `stroke-width:
1.7–1.8`, `stroke-linecap`/`stroke-linejoin: round`, 24×24 viewBox. Filled
icons (play triangle, active heart) use `fill`, no stroke.

## Motion

One deliberate effect: a slow (`~3.5s`) pulsing ring around the now-playing
orb (`opacity`/`scale`, ease-in-out). No other looping animation. Respect
`prefers-reduced-motion` — disable the pulse for users who set it.

## Layout

- Main window: persistent top bar (logo mark + Home/Queue/Settings icons)
  and a persistent mini-player bar pinned to the bottom, both always
  visible while browsing.
- Clicking the mini-player bar expands to the full Player view (not a
  separate window).
- Hit targets: **44×44px minimum** for anything clickable in the real UI
  (buttons, icon buttons) — toggle switches and segmented controls are the
  one accepted exception (smaller visual track, padded click area).

## Accessibility

- Dark theme only for v1; contrast checked against `--bg-base`.
- Never convey state by color alone — pair the accent color with an icon
  or label change (e.g., filled vs. outline heart, not just a color swap).
- Visible focus ring (`outline`) on every interactive element, not just
  `:hover`.
