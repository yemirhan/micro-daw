/**
 * Pure logic for the piano roll editor: layout (pitch <-> row), coordinate
 * transforms, snapping, grid/ruler generation, hit-testing and all note edit
 * math (move / resize / draw / duplicate / paste / quantize / velocity).
 *
 * Everything here is side-effect free and unit tested
 * (src/utils/pianoRollHelpers.test.ts). Beats are relative to the region start
 * unless a parameter says otherwise; `offset` parameters are the region's
 * absolute start beat so the grid lines/snapping line up with the song's bars.
 */
import type { RegionNote } from '@/types/arrangement';
import type { GridLine, NoteEdge, NoteHit, NotesEdit, RowLayout, RulerLabel, SnapOption } from '@/types/pianoRollEditor';
import { DRUM_SOUNDS } from '@/utils/constants';
import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

export const BEATS_PER_BAR = 4;

/** Full MIDI range, top row = PR_NOTE_MAX. */
export const PR_NOTE_MIN = 0;
export const PR_NOTE_MAX = 127;
export const PR_NOTE_COUNT = PR_NOTE_MAX - PR_NOTE_MIN + 1;

/** Drum lane rows (GM pads, in pad order). */
export const DRUM_ROWS = DRUM_SOUNDS.map((d) => ({
  midiNote: d.midiNote,
  name: d.shortName,
  fullName: d.name,
  color: d.color,
}));
export const PR_DRUM_COUNT = DRUM_ROWS.length;

export const DEFAULT_NOTE_VELOCITY = 0.8;
export const DEFAULT_NOTE_DURATION = 1; // 1 beat
export const DEFAULT_DRUM_DURATION = 0.25;
/** Shortest note the editor will create/resize to when snap is off. */
export const MIN_NOTE_LENGTH = 1 / 64;
/** Internal resolution; all computed beat values are rounded to this. */
export const TICKS_PER_BEAT = 960;
export const MIN_VELOCITY = 1 / 127;

export const PR_MIN_PX_PER_BEAT = 6;
export const PR_MAX_PX_PER_BEAT = 640;
export const PR_DEFAULT_PX_PER_BEAT = 64;
export const PR_MIN_ROW_HEIGHT = 7;
export const PR_MAX_ROW_HEIGHT = 40;
export const PR_DEFAULT_ROW_HEIGHT = 16;
export const PR_DEFAULT_DRUM_ROW_HEIGHT = 30;

export const SNAP_OPTIONS: SnapOption[] = [
  { id: '1/1', label: '1/1', beats: 4 },
  { id: '1/2', label: '1/2', beats: 2 },
  { id: '1/4', label: '1/4', beats: 1 },
  { id: '1/8', label: '1/8', beats: 0.5 },
  { id: '1/16', label: '1/16', beats: 0.25 },
  { id: '1/32', label: '1/32', beats: 0.125 },
  { id: '1/4T', label: '1/4 T', beats: 2 / 3 },
  { id: '1/8T', label: '1/8 T', beats: 1 / 3 },
  { id: '1/16T', label: '1/16 T', beats: 1 / 6 },
  { id: '1/32T', label: '1/32 T', beats: 1 / 12 },
  { id: 'off', label: 'Off', beats: 0 },
];

/** Note lengths offered in the toolbar (in beats). */
export const NOTE_LENGTH_OPTIONS: SnapOption[] = [
  { id: '1/1', label: '1/1', beats: 4 },
  { id: '1/2', label: '1/2', beats: 2 },
  { id: '1/4.', label: '1/4 .', beats: 1.5 },
  { id: '1/4', label: '1/4', beats: 1 },
  { id: '1/8.', label: '1/8 .', beats: 0.75 },
  { id: '1/8', label: '1/8', beats: 0.5 },
  { id: '1/16', label: '1/16', beats: 0.25 },
  { id: '1/32', label: '1/32', beats: 0.125 },
  { id: '1/4T', label: '1/4 T', beats: 2 / 3 },
  { id: '1/8T', label: '1/8 T', beats: 1 / 3 },
  { id: '1/16T', label: '1/16 T', beats: 1 / 6 },
];

const EPS = 1e-6;

// ---------------------------------------------------------------------------
// Small math helpers
// ---------------------------------------------------------------------------

export function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

/** Round a beat value to the internal tick grid (kills float drift like 0.30000000000000004). */
export function roundBeat(beat: number): number {
  return Math.round(beat * TICKS_PER_BEAT) / TICKS_PER_BEAT || 0;
}

export function clampVelocity(v: number): number {
  return clamp(v, MIN_VELOCITY, 1);
}

/** 0..1 velocity -> MIDI 1..127 for display. */
export function velocityToMidi(v: number): number {
  return Math.round(clampVelocity(v) * 127);
}

export function isMultipleOf(value: number, step: number): boolean {
  if (step <= 0) return false;
  const r = value / step;
  return Math.abs(r - Math.round(r)) < 1e-4;
}

// ---------------------------------------------------------------------------
// Row layouts (pitch <-> row)
// ---------------------------------------------------------------------------

export function createPianoLayout(min = PR_NOTE_MIN, max = PR_NOTE_MAX): RowLayout {
  const rowCount = max - min + 1;
  return {
    kind: 'piano',
    rowCount,
    pitchToRow: (pitch) => (pitch < min || pitch > max ? -1 : max - pitch),
    rowToPitch: (row) => max - clamp(Math.round(row), 0, rowCount - 1),
    rowLabel: (row) => midiToNoteName(max - row),
  };
}

/**
 * Drum layout: one row per GM pad, plus extra rows for any other pitches that
 * appear in the region (so no note is ever hidden). Extra rows go below the pads.
 */
export function createDrumLayout(extraPitches: Iterable<number> = []): RowLayout & { pitches: number[] } {
  const pitches = DRUM_ROWS.map((d) => d.midiNote);
  const extras = [...new Set(extraPitches)].filter((p) => !pitches.includes(p)).sort((a, b) => a - b);
  pitches.push(...extras);
  const index = new Map(pitches.map((p, i) => [p, i]));
  return {
    kind: 'drum',
    pitches,
    rowCount: pitches.length,
    pitchToRow: (pitch) => index.get(pitch) ?? -1,
    rowToPitch: (row) => pitches[clamp(Math.round(row), 0, pitches.length - 1)],
    rowLabel: (row) => DRUM_ROWS[row]?.fullName ?? midiToNoteName(pitches[row] ?? 36),
    rowColor: (row) => DRUM_ROWS[row]?.color,
  };
}

/** Whether a row should get "black key" shading. */
export function isShadedRow(layout: RowLayout, row: number): boolean {
  if (layout.kind === 'drum') return row % 2 === 1;
  return isBlackKey(layout.rowToPitch(row));
}

// ---------------------------------------------------------------------------
// Coordinate transforms
// ---------------------------------------------------------------------------

export function beatToX(beat: number, pxPerBeat: number): number {
  return beat * pxPerBeat;
}

export function xToBeat(x: number, pxPerBeat: number): number {
  return x / pxPerBeat;
}

export function rowToY(row: number, rowHeight: number): number {
  return row * rowHeight;
}

export function yToRow(y: number, rowHeight: number): number {
  return Math.floor(y / rowHeight);
}

/** Legacy aliases (kept for callers of the old helpers). */
export const beatToGridX = beatToX;
export const gridXToBeat = xToBeat;

/**
 * Keep the content point under the cursor fixed while zooming.
 * `cursor` is the cursor offset inside the viewport (px).
 */
export function zoomScroll(scroll: number, cursor: number, oldScale: number, newScale: number): number {
  if (oldScale <= 0) return scroll;
  return Math.max(0, ((scroll + cursor) / oldScale) * newScale - cursor);
}

/** Multiplicative zoom step from a wheel delta, clamped to [min, max]. */
export function zoomFromWheel(current: number, deltaY: number, min: number, max: number, sensitivity = 0.0025): number {
  const factor = Math.exp(-deltaY * sensitivity);
  return clamp(current * factor, min, max);
}

/** Map a 0..1 slider position to a log scale between min and max (and back). */
export function sliderToZoom(t: number, min: number, max: number): number {
  return min * Math.pow(max / min, clamp(t, 0, 1));
}

export function zoomToSlider(v: number, min: number, max: number): number {
  return clamp(Math.log(v / min) / Math.log(max / min), 0, 1);
}

// ---------------------------------------------------------------------------
// Snapping
// ---------------------------------------------------------------------------

/** Round to the nearest grid line. `offset` = absolute beat of the region start. */
export function snapBeat(beat: number, snap: number, offset = 0): number {
  if (snap <= 0) return roundBeat(beat);
  return roundBeat(Math.round((beat + offset) / snap) * snap - offset);
}

export function snapFloor(beat: number, snap: number, offset = 0): number {
  if (snap <= 0) return roundBeat(beat);
  return roundBeat(Math.floor((beat + offset) / snap + EPS) * snap - offset);
}

export function snapCeil(beat: number, snap: number, offset = 0): number {
  if (snap <= 0) return roundBeat(beat);
  return roundBeat(Math.ceil((beat + offset) / snap - EPS) * snap - offset);
}

/** Legacy names. */
export const snapBeatPR = (beat: number, snapValue: number) => snapBeat(beat, snapValue);
export const snapBeatFloor = (beat: number, snapValue: number) => snapFloor(beat, snapValue);

/** Step used for keyboard nudges: the grid, or a 1/16 when snap is off. */
export function nudgeStep(snap: number): number {
  return snap > 0 ? snap : 0.25;
}

export function findSnapOption(beats: number, options: SnapOption[] = SNAP_OPTIONS): SnapOption | undefined {
  return options.find((o) => Math.abs(o.beats - beats) < 1e-6);
}

/** Human readable length: "1/16", "3/16", "1/8 T", "2 bars", "0.37 beats". */
export function formatBeats(beats: number): string {
  if (beats <= 0) return '0';
  const opt = findSnapOption(beats, NOTE_LENGTH_OPTIONS) ?? findSnapOption(beats);
  if (opt) return opt.label;
  if (isMultipleOf(beats, BEATS_PER_BAR)) {
    const bars = Math.round(beats / BEATS_PER_BAR);
    return `${bars} bar${bars === 1 ? '' : 's'}`;
  }
  const whole = beats / 4;
  for (const den of [2, 4, 8, 16, 32, 64]) {
    const num = whole * den;
    if (Math.abs(num - Math.round(num)) < 1e-6) return `${Math.round(num)}/${den}`;
  }
  for (const den of [3, 6, 12, 24, 48]) {
    const num = whole * den;
    if (Math.abs(num - Math.round(num)) < 1e-6) return `${Math.round(num)}/${den}`;
  }
  return `${beats.toFixed(2)} beats`;
}

/** Absolute beat -> "bar.beat.sixteenth" (1-based), e.g. 0 -> "1.1.1". */
export function formatPosition(absBeat: number): string {
  const b = Math.max(0, absBeat);
  const bar = Math.floor(b / BEATS_PER_BAR + EPS) + 1;
  const beatInBar = Math.floor((b % BEATS_PER_BAR) + EPS) + 1;
  const sixteenth = Math.floor(((b % 1) + EPS) * 4) + 1;
  return `${bar}.${Math.min(beatInBar, BEATS_PER_BAR)}.${Math.min(sixteenth, 4)}`;
}

// ---------------------------------------------------------------------------
// Grid + ruler generation
// ---------------------------------------------------------------------------

/**
 * Subdivision actually drawn for a given snap + zoom: the snap value, coarsened
 * (x2) until lines are at least `minSpacing` px apart. With snap off an
 * adaptive power-of-two subdivision is used.
 */
export function gridSubdivision(snap: number, pxPerBeat: number, minSpacing = 6): number {
  let sub = snap > 0 ? snap : 0.25;
  if (snap <= 0) {
    // Adaptive: finest power-of-two subdivision that is >= 12 px apart.
    sub = 1;
    while (sub / 2 >= 1 / 16 && (sub / 2) * pxPerBeat >= 12) sub /= 2;
  }
  while (sub * pxPerBeat < minSpacing && sub < BEATS_PER_BAR * 64) sub *= 2;
  return sub;
}

/** Vertical grid lines between two region-relative beats (sorted, one per position, strongest level wins). */
export function computeGridLines(
  startBeat: number,
  endBeat: number,
  pxPerBeat: number,
  snap: number,
  offset = 0,
  minSpacing = 6,
): GridLine[] {
  if (endBeat <= startBeat || pxPerBeat <= 0) return [];
  const byTick = new Map<number, GridLine>();
  const add = (step: number, level: 0 | 1 | 2) => {
    const first = Math.ceil((startBeat + offset) / step - EPS);
    const last = Math.floor((endBeat + offset) / step + EPS);
    for (let k = first; k <= last; k++) {
      const beat = roundBeat(k * step - offset);
      const key = Math.round(beat * TICKS_PER_BEAT);
      const prev = byTick.get(key);
      if (!prev || prev.level > level) byTick.set(key, { beat, level });
    }
  };
  // Bars can get too dense when zoomed far out: only keep every Nth bar.
  let barEvery = 1;
  while (barEvery * BEATS_PER_BAR * pxPerBeat < minSpacing * 2) barEvery *= 2;
  add(BEATS_PER_BAR * barEvery, 0);
  if (pxPerBeat >= minSpacing) add(1, 1);
  const sub = gridSubdivision(snap, pxPerBeat, minSpacing);
  if (sub < 1 && sub * pxPerBeat >= minSpacing) add(sub, 2);
  return [...byTick.values()].sort((a, b) => a.beat - b.beat);
}

/** Ruler labels: bar numbers (and bar.beat when zoomed in) at least `minLabelPx` apart. */
export function computeRulerLabels(
  startBeat: number,
  endBeat: number,
  pxPerBeat: number,
  offset = 0,
  minLabelPx = 44,
): RulerLabel[] {
  const labels: RulerLabel[] = [];
  if (endBeat <= startBeat || pxPerBeat <= 0) return labels;
  const barPx = BEATS_PER_BAR * pxPerBeat;
  const showBeats = pxPerBeat >= minLabelPx;
  let barEvery = 1;
  while (barEvery * barPx < minLabelPx) barEvery *= 2;
  const step = showBeats ? 1 : BEATS_PER_BAR * barEvery;
  const first = Math.ceil((startBeat + offset) / step - EPS);
  const last = Math.floor((endBeat + offset) / step + EPS);
  for (let k = first; k <= last; k++) {
    const abs = k * step;
    const bar = Math.floor(abs / BEATS_PER_BAR + EPS) + 1;
    const major = isMultipleOf(abs, BEATS_PER_BAR);
    const beatInBar = Math.round(abs - (bar - 1) * BEATS_PER_BAR) + 1;
    labels.push({
      beat: roundBeat(abs - offset),
      label: major ? `${bar}` : `${bar}.${beatInBar}`,
      major,
    });
  }
  return labels;
}

// ---------------------------------------------------------------------------
// Hit-testing & selection
// ---------------------------------------------------------------------------

export interface HitTestOptions {
  /** Width of the resize handle zone at each end, px. */
  edgePx?: number;
  /** Minimum drawn width of a note, px (short notes are still grabbable). */
  minWidthPx?: number;
  /** Drum notes / one-shots can't be resized. */
  resizable?: boolean;
  /** Selected notes are drawn on top, so they win hit-tests. */
  selected?: ReadonlySet<number>;
}

export function noteVisualWidth(note: RegionNote, pxPerBeat: number, minWidthPx = 4): number {
  return Math.max(note.durationBeats * pxPerBeat, minWidthPx);
}

/**
 * Find the note under grid-content coordinates (x, y in px, relative to the
 * content origin), and which part of it (start edge / body / end edge).
 */
export function hitTestNote(
  notes: readonly RegionNote[],
  x: number,
  y: number,
  layout: RowLayout,
  pxPerBeat: number,
  rowHeight: number,
  opts: HitTestOptions = {},
): NoteHit | null {
  const { edgePx = 6, minWidthPx = 4, resizable = true, selected } = opts;
  const row = yToRow(y, rowHeight);
  if (row < 0 || row >= layout.rowCount) return null;
  let best: NoteHit | null = null;
  let bestSelected = false;
  for (let i = notes.length - 1; i >= 0; i--) {
    const n = notes[i];
    if (layout.pitchToRow(n.note) !== row) continue;
    const x0 = n.startBeat * pxPerBeat;
    const w = noteVisualWidth(n, pxPerBeat, minWidthPx);
    if (x < x0 || x > x0 + w) continue;
    const isSel = selected?.has(i) ?? false;
    if (best && (bestSelected || !isSel)) continue;
    let edge: NoteEdge = 'body';
    if (resizable) {
      const zone = Math.min(edgePx, w / 3);
      if (x >= x0 + w - zone) edge = 'end';
      else if (x <= x0 + zone) edge = 'start';
    }
    best = { index: i, edge };
    bestSelected = isSel;
    if (isSel) break;
  }
  return best;
}

/** Indices of notes intersecting a beat/row rectangle (inclusive rows). */
export function notesInRect(
  notes: readonly RegionNote[],
  beatA: number,
  beatB: number,
  rowA: number,
  rowB: number,
  layout: RowLayout,
): number[] {
  const b0 = Math.min(beatA, beatB);
  const b1 = Math.max(beatA, beatB);
  const r0 = Math.min(rowA, rowB);
  const r1 = Math.max(rowA, rowB);
  const out: number[] = [];
  notes.forEach((n, i) => {
    const row = layout.pitchToRow(n.note);
    if (row < r0 || row > r1) return;
    const end = n.startBeat + Math.max(n.durationBeats, EPS);
    if (n.startBeat <= b1 && end >= b0) out.push(i);
  });
  return out;
}

/** Shift-click semantics: toggle membership. Plain click: select just this one (unless already selected). */
export function applyClickSelection(current: ReadonlySet<number>, index: number, toggle: boolean): Set<number> {
  if (toggle) {
    const next = new Set(current);
    if (next.has(index)) next.delete(index);
    else next.add(index);
    return next;
  }
  if (current.has(index)) return new Set(current);
  return new Set([index]);
}

// ---------------------------------------------------------------------------
// Edits
// ---------------------------------------------------------------------------

function uniqueSorted(indices: Iterable<number>, length: number): number[] {
  return [...new Set(indices)].filter((i) => i >= 0 && i < length).sort((a, b) => a - b);
}

/**
 * Clamp a move so every selected note stays at startBeat >= 0 and inside the row range.
 * dRow is in rows (positive = down = lower pitch for the piano layout).
 */
export function clampMoveDelta(
  notes: readonly RegionNote[],
  indices: Iterable<number>,
  dBeat: number,
  dRow: number,
  layout: RowLayout,
): { dBeat: number; dRow: number } {
  const idx = uniqueSorted(indices, notes.length);
  if (idx.length === 0) return { dBeat: 0, dRow: 0 };
  let minStart = Infinity;
  let minRow = Infinity;
  let maxRow = -Infinity;
  for (const i of idx) {
    minStart = Math.min(minStart, notes[i].startBeat);
    const r = layout.pitchToRow(notes[i].note);
    if (r >= 0) {
      minRow = Math.min(minRow, r);
      maxRow = Math.max(maxRow, r);
    }
  }
  const beat = Math.max(dBeat, -minStart);
  let row = dRow;
  if (minRow !== Infinity) {
    row = clamp(dRow, -minRow, layout.rowCount - 1 - maxRow);
  }
  return { dBeat: roundBeat(beat), dRow: Math.round(row) };
}

/** Beat delta that lands the anchor note's start on the grid. */
export function snappedMoveDelta(anchorStart: number, rawDelta: number, snap: number, offset = 0): number {
  if (snap <= 0) return roundBeat(rawDelta);
  return roundBeat(snapBeat(anchorStart + rawDelta, snap, offset) - anchorStart);
}

/** Move the given notes by dBeat / dRow (clamped). Returns a new array. */
export function moveNotes(
  notes: readonly RegionNote[],
  indices: Iterable<number>,
  dBeat: number,
  dRow: number,
  layout: RowLayout,
): RegionNote[] {
  const idx = new Set(uniqueSorted(indices, notes.length));
  const d = clampMoveDelta(notes, idx, dBeat, dRow, layout);
  if (d.dBeat === 0 && d.dRow === 0) return notes.slice();
  return notes.map((n, i) => {
    if (!idx.has(i)) return n;
    const row = layout.pitchToRow(n.note);
    const note = row >= 0 && d.dRow !== 0 ? layout.rowToPitch(row + d.dRow) : n.note;
    return { ...n, startBeat: roundBeat(n.startBeat + d.dBeat), note };
  });
}

/** Transpose by semitones (piano) — positive = up. Clamped to the MIDI range. */
export function transposeNotes(notes: readonly RegionNote[], indices: Iterable<number>, semitones: number, layout: RowLayout): RegionNote[] {
  return moveNotes(notes, indices, 0, -semitones, layout);
}

/**
 * Resize from the start or end edge. The anchor note's edge snaps to the grid
 * and the same delta is applied to all selected notes. A note never gets shorter
 * than min(grid, its original length) or the absolute minimum length.
 */
export function resizeNotes(
  notes: readonly RegionNote[],
  indices: Iterable<number>,
  edge: 'start' | 'end',
  anchorIndex: number,
  rawDelta: number,
  snap: number,
  offset = 0,
): RegionNote[] {
  const idx = new Set(uniqueSorted(indices, notes.length));
  const anchor = notes[anchorIndex];
  if (!anchor) return notes.slice();
  let delta: number;
  if (edge === 'end') {
    const end = anchor.startBeat + anchor.durationBeats;
    delta = snap > 0 ? snapBeat(end + rawDelta, snap, offset) - end : rawDelta;
  } else {
    delta = snap > 0 ? snapBeat(anchor.startBeat + rawDelta, snap, offset) - anchor.startBeat : rawDelta;
  }
  const minLen = (dur: number) => Math.max(MIN_NOTE_LENGTH, Math.min(snap > 0 ? snap : MIN_NOTE_LENGTH, dur));
  return notes.map((n, i) => {
    if (!idx.has(i)) return n;
    if (edge === 'end') {
      const dur = Math.max(minLen(n.durationBeats), n.durationBeats + delta);
      return { ...n, durationBeats: roundBeat(dur) };
    }
    const end = n.startBeat + n.durationBeats;
    const start = clamp(n.startBeat + delta, 0, end - minLen(n.durationBeats));
    return { ...n, startBeat: roundBeat(start), durationBeats: roundBeat(end - start) };
  });
}

/** Change lengths by a fixed amount (keyboard Alt+Left/Right). */
export function changeLengths(notes: readonly RegionNote[], indices: Iterable<number>, delta: number, minLength = MIN_NOTE_LENGTH): RegionNote[] {
  const idx = new Set(uniqueSorted(indices, notes.length));
  return notes.map((n, i) => (idx.has(i) ? { ...n, durationBeats: roundBeat(Math.max(minLength, n.durationBeats + delta)) } : n));
}

/**
 * Start + duration of a note being drawn: anchored at `anchorStart`, extending
 * to the grid line nearest the cursor (at least one grid step).
 */
export function drawSpan(anchorStart: number, cursorBeat: number, snap: number, offset = 0): { startBeat: number; durationBeats: number } {
  const step = snap > 0 ? snap : MIN_NOTE_LENGTH;
  if (cursorBeat >= anchorStart) {
    const end = snap > 0 ? snapBeat(cursorBeat, snap, offset) : cursorBeat;
    return { startBeat: anchorStart, durationBeats: roundBeat(Math.max(step, end - anchorStart)) };
  }
  // Dragging left of the anchor: the note grows leftwards, ending one step after the anchor.
  const start = Math.max(0, snap > 0 ? snapFloor(cursorBeat, snap, offset) : cursorBeat);
  return { startBeat: roundBeat(start), durationBeats: roundBeat(anchorStart + step - start) };
}

export function deleteNotes(notes: readonly RegionNote[], indices: Iterable<number>): RegionNote[] {
  const idx = new Set(indices);
  return notes.filter((_, i) => !idx.has(i));
}

/** [start, end) of the selected notes, or null. */
export function selectionSpan(notes: readonly RegionNote[], indices: Iterable<number>): { start: number; end: number } | null {
  let start = Infinity;
  let end = -Infinity;
  for (const i of indices) {
    const n = notes[i];
    if (!n) continue;
    start = Math.min(start, n.startBeat);
    end = Math.max(end, n.startBeat + n.durationBeats);
  }
  return start === Infinity ? null : { start, end };
}

/** Append copies of the given notes, shifted by dBeat / dRow; the copies become the selection. */
export function appendCopies(
  notes: readonly RegionNote[],
  indices: Iterable<number>,
  dBeat = 0,
  dRow = 0,
  layout?: RowLayout,
): NotesEdit {
  const idx = uniqueSorted(indices, notes.length);
  const copies = idx.map((i) => {
    const n = notes[i];
    let note = n.note;
    if (layout && dRow !== 0) {
      const row = layout.pitchToRow(n.note);
      if (row >= 0) note = layout.rowToPitch(row + dRow);
    }
    return { ...n, note, startBeat: roundBeat(Math.max(0, n.startBeat + dBeat)) };
  });
  const out = [...notes, ...copies];
  return { notes: out, selection: copies.map((_, k) => notes.length + k) };
}

/**
 * Cmd+D: duplicate the selection right after itself. The shift is the
 * selection's span rounded up to the grid (so patterns stay on the grid).
 */
export function duplicateNotes(notes: readonly RegionNote[], indices: Iterable<number>, snap: number): NotesEdit {
  const idx = uniqueSorted(indices, notes.length);
  const span = selectionSpan(notes, idx);
  if (!span) return { notes: notes.slice(), selection: [] };
  const len = span.end - span.start;
  const unit = snap > 0 ? snap : MIN_NOTE_LENGTH;
  const shift = Math.max(unit, Math.ceil(len / unit - EPS) * unit);
  return appendCopies(notes, idx, roundBeat(shift));
}

/** Copy notes to a clipboard payload, normalised so the earliest note starts at 0. */
export function copyNotes(notes: readonly RegionNote[], indices: Iterable<number>): RegionNote[] {
  const idx = uniqueSorted(indices, notes.length);
  const span = selectionSpan(notes, idx);
  if (!span) return [];
  return idx.map((i) => ({ ...notes[i], startBeat: roundBeat(notes[i].startBeat - span.start) }));
}

/** Paste clipboard notes at `atBeat`; the pasted notes become the selection. */
export function pasteNotes(notes: readonly RegionNote[], clipboard: readonly RegionNote[], atBeat: number, isDrum?: boolean): NotesEdit {
  const at = Math.max(0, atBeat);
  const pasted = clipboard.map((n) => ({ ...n, isDrum: isDrum ?? n.isDrum, startBeat: roundBeat(n.startBeat + at) }));
  return { notes: [...notes, ...pasted], selection: pasted.map((_, k) => notes.length + k) };
}

export interface QuantizeOptions {
  /** Only these notes (default: all). */
  indices?: Iterable<number>;
  /** Also snap note ends (length). */
  ends?: boolean;
  /** Region start (absolute beat) so the grid matches the song's bars. */
  offset?: number;
  /** 0..1, how far to move towards the grid. */
  strength?: number;
}

/** Quantize note starts (and optionally ends) to the grid. */
export function quantizeNotes(notes: RegionNote[], snapValue: number, opts: QuantizeOptions = {}): RegionNote[] {
  if (snapValue <= 0) return notes;
  const { ends = false, offset = 0, strength = 1 } = opts;
  const only = opts.indices ? new Set(opts.indices) : null;
  return notes.map((note, i) => {
    if (only && !only.has(i)) return note;
    const start = Math.max(0, note.startBeat + (snapBeat(note.startBeat, snapValue, offset) - note.startBeat) * strength);
    let dur = note.durationBeats;
    if (ends) {
      const end = note.startBeat + note.durationBeats;
      const qEnd = end + (snapBeat(end, snapValue, offset) - end) * strength;
      dur = Math.max(snapValue * strength || MIN_NOTE_LENGTH, qEnd - start);
    }
    return { ...note, startBeat: roundBeat(start), durationBeats: roundBeat(Math.max(MIN_NOTE_LENGTH, dur)) };
  });
}

// ---------------------------------------------------------------------------
// Velocity
// ---------------------------------------------------------------------------

export function setVelocities(notes: readonly RegionNote[], indices: Iterable<number>, velocity: number): RegionNote[] {
  const idx = new Set(indices);
  const v = clampVelocity(velocity);
  return notes.map((n, i) => (idx.has(i) ? { ...n, velocity: v } : n));
}

export function offsetVelocities(notes: readonly RegionNote[], indices: Iterable<number>, delta: number): RegionNote[] {
  const idx = new Set(indices);
  return notes.map((n, i) => (idx.has(i) ? { ...n, velocity: clampVelocity(n.velocity + delta) } : n));
}

/**
 * Linear ramp: every candidate note whose start lies between beatA and beatB
 * gets the velocity on the line (beatA, velA) -> (beatB, velB).
 */
export function rampVelocities(
  notes: readonly RegionNote[],
  candidates: Iterable<number> | null,
  beatA: number,
  velA: number,
  beatB: number,
  velB: number,
): RegionNote[] {
  const only = candidates ? new Set(candidates) : null;
  const b0 = Math.min(beatA, beatB);
  const b1 = Math.max(beatA, beatB);
  return notes.map((n, i) => {
    if (only && !only.has(i)) return n;
    if (n.startBeat < b0 - EPS || n.startBeat > b1 + EPS) return n;
    const t = beatB === beatA ? 1 : (n.startBeat - beatA) / (beatB - beatA);
    return { ...n, velocity: clampVelocity(velA + (velB - velA) * t) };
  });
}

/** Notes whose velocity stem is within `tolPx` of x (px, content coords). */
export function notesAtX(notes: readonly RegionNote[], x: number, pxPerBeat: number, tolPx = 5, candidates?: ReadonlySet<number> | null): number[] {
  const out: number[] = [];
  notes.forEach((n, i) => {
    if (candidates && !candidates.has(i)) return;
    if (Math.abs(n.startBeat * pxPerBeat - x) <= tolPx) out.push(i);
  });
  return out;
}

/** 0..1 velocity -> bucket index for pre-computed colors. */
export function velocityBucket(v: number, buckets: number): number {
  return clamp(Math.round(clampVelocity(v) * (buckets - 1)), 0, buckets - 1);
}

// ---------------------------------------------------------------------------
// View helpers
// ---------------------------------------------------------------------------

/** Region length needed to contain every note (rounded up to a bar), never shrinks. */
export function requiredRegionLength(notes: readonly RegionNote[], currentLength: number): number {
  let end = 0;
  for (const n of notes) end = Math.max(end, n.startBeat + n.durationBeats);
  if (end <= currentLength + EPS) return currentLength;
  return Math.ceil(end / BEATS_PER_BAR - EPS) * BEATS_PER_BAR;
}

/** Pitch range [lo, hi] of the notes, or null. */
export function pitchRange(notes: readonly RegionNote[]): { lo: number; hi: number } | null {
  if (notes.length === 0) return null;
  let lo = Infinity;
  let hi = -Infinity;
  for (const n of notes) {
    lo = Math.min(lo, n.note);
    hi = Math.max(hi, n.note);
  }
  return { lo, hi };
}

/**
 * Vertical scroll (px) that centres the notes (or C4 when empty) in a viewport
 * of `viewHeight`.
 */
export function centerScrollTop(notes: readonly RegionNote[], layout: RowLayout, rowHeight: number, viewHeight: number): number {
  if (layout.kind === 'drum') return 0;
  const range = pitchRange(notes) ?? { lo: 60, hi: 72 };
  const rowTop = layout.pitchToRow(Math.min(PR_NOTE_MAX, range.hi));
  const rowBottom = layout.pitchToRow(Math.max(PR_NOTE_MIN, range.lo));
  const center = ((rowTop + rowBottom + 1) / 2) * rowHeight;
  const max = Math.max(0, layout.rowCount * rowHeight - viewHeight);
  return clamp(Math.round(center - viewHeight / 2), 0, max);
}

/**
 * "Fit": zoom so the region (or notes, whichever is longer) fills the width and
 * the note range fills the height (within the row-height limits).
 */
export function fitView(
  notes: readonly RegionNote[],
  layout: RowLayout,
  regionLength: number,
  viewWidth: number,
  viewHeight: number,
): { pxPerBeat: number; rowHeight: number; scrollLeft: number; scrollTop: number } {
  let end = regionLength;
  for (const n of notes) end = Math.max(end, n.startBeat + n.durationBeats);
  const pxPerBeat = clamp((viewWidth - 16) / Math.max(1, end), PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT);
  let rowHeight: number;
  if (layout.kind === 'drum') {
    rowHeight = clamp(Math.floor(viewHeight / layout.rowCount), PR_MIN_ROW_HEIGHT, PR_MAX_ROW_HEIGHT);
  } else {
    const range = pitchRange(notes) ?? { lo: 60, hi: 72 };
    const rows = range.hi - range.lo + 1 + 4; // two rows of padding each side
    rowHeight = clamp(Math.floor(viewHeight / rows), PR_MIN_ROW_HEIGHT, 24);
  }
  return {
    pxPerBeat,
    rowHeight,
    scrollLeft: 0,
    scrollTop: centerScrollTop(notes, layout, rowHeight, viewHeight),
  };
}

/** Where the "page" should scroll to so the playhead stays visible while following. */
export function followScroll(playheadX: number, scrollLeft: number, viewWidth: number, margin = 0.1): number | null {
  if (viewWidth <= 0) return null;
  if (playheadX < scrollLeft || playheadX > scrollLeft + viewWidth * (1 - margin / 2)) {
    return Math.max(0, playheadX - viewWidth * margin);
  }
  return null;
}
