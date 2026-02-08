// Consistent oklch chart colors matching the app theme
export const CHART_COLORS = {
  primary: 'oklch(0.65 0.20 265)',       // purple
  secondary: 'oklch(0.65 0.20 155)',     // green
  accent: 'oklch(0.70 0.18 65)',         // orange
  muted: 'oklch(0.30 0.02 270)',         // dark gray
  grid: 'oklch(0.22 0.01 270)',          // grid lines
  text: 'oklch(0.55 0.02 270)',          // axis text
  tooltip: 'oklch(0.16 0.02 270)',       // tooltip bg
  tooltipBorder: 'oklch(0.28 0.02 270)', // tooltip border
};

export const CATEGORY_COLORS: Record<string, string> = {
  'piano-basics': 'oklch(0.65 0.20 265)',
  'scales': 'oklch(0.65 0.20 155)',
  'chords': 'oklch(0.70 0.18 65)',
  'rhythm': 'oklch(0.62 0.22 25)',
  'drum-patterns': 'oklch(0.62 0.22 310)',
  'free-play': 'oklch(0.70 0.20 270)',
};
