/**
 * Chart colors for the practice statistics, mirroring the "studio instrument"
 * tokens in index.css (SVG attributes get literal values, not var()).
 *
 * Every stats chart is single-series, so each uses ONE hue:
 *   practice time  -> accent (signal orange)
 *   accuracy       -> correct (green)
 *   categories     -> select (cool cyan-blue), as an HTML bar list
 * Axes and grid stay recessive; text uses the neutral ink tokens.
 */
export const CHART_COLORS = {
  /** --primary / --note-target */
  primary: 'oklch(0.74 0.17 55)',
  /** --note-correct / --signal-play */
  secondary: 'oklch(0.76 0.17 150)',
  /** --signal-select */
  accent: 'oklch(0.74 0.12 230)',
  /** --surface-4 */
  muted: 'oklch(0.330 0.007 65)',
  /** hairline grid on --surface-1 */
  grid: 'oklch(0.285 0.006 65)',
  /** --muted-foreground */
  text: 'oklch(0.68 0.008 70)',
  /** --foreground */
  ink: 'oklch(0.935 0.004 80)',
  /** --popover */
  tooltip: 'oklch(0.235 0.005 65)',
  /** --border */
  tooltipBorder: 'oklch(0.330 0.007 65)',
  /** --surface-1 (ring around markers so they separate from the line) */
  surface: 'oklch(0.212 0.005 65)',
  /** hover cursor band */
  cursor: 'oklch(1 0 0 / 0.04)',
};

/** Single-hue color per practice category (categories are labelled; color is not identity). */
export const CATEGORY_COLORS: Record<string, string> = {
  'piano-basics': CHART_COLORS.accent,
  scales: CHART_COLORS.accent,
  chords: CHART_COLORS.accent,
  rhythm: CHART_COLORS.accent,
  'drum-patterns': CHART_COLORS.accent,
  'free-play': CHART_COLORS.accent,
};

/** Shared recharts tooltip styling. */
export const TOOLTIP_STYLE = {
  contentStyle: {
    backgroundColor: CHART_COLORS.tooltip,
    border: `1px solid ${CHART_COLORS.tooltipBorder}`,
    borderRadius: 8,
    fontSize: 12,
    padding: '6px 10px',
    boxShadow: '0 8px 24px -12px oklch(0 0 0 / 0.6)',
  },
  labelStyle: { color: CHART_COLORS.text, marginBottom: 2 },
  itemStyle: { color: CHART_COLORS.ink, fontFamily: "'JetBrains Mono', ui-monospace, monospace", padding: 0 },
} as const;

export const AXIS_TICK = { fontSize: 10, fill: CHART_COLORS.text, fontFamily: "'JetBrains Mono', ui-monospace, monospace" };
