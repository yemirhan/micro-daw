/**
 * Pure helpers for the Beat view (pad layout, computer-keyboard pads, step
 * grid math, sample drops, chopping). No React, no audio — unit tested.
 */
import { PAD_COUNT, padToNote, type PadIndex, type PadSample, type PadSettings } from '@/types/sampler';
import type { Region } from '@/types/arrangement';
import { FACTORY_VOICE_LABELS, PAD_COLORS } from '@/audio/sampler/factoryKit';
import { classicSampleName } from '@/audio/sampler/classic/machines';
import { STEP_EPSILON, patternStepAt } from '@/utils/arrangementHelpers';
import { midiToNoteName } from '@/utils/noteHelpers';
import { SAMPLE_DRAG_MIME, displayName, isAudioFileName, type SampleDragPayload } from '@/components/browser/browserHelpers';

// ---------------------------------------------------------------------------
// Pad layout
// ---------------------------------------------------------------------------

export type PadLayout = '4x4' | '2x8';

/**
 * Pad indices row by row, top to bottom, as they appear on screen.
 * - 4x4 (MPC): pad 0 bottom-left, bank A = bottom two rows, bank B = top two.
 * - 2x8 (MPK Mini Play): bank A | bank B side by side, each 2x4 with pad 0/8
 *   bottom-left — the hardware's 8 pads with the bank switch unfolded.
 */
export function padRows(layout: PadLayout): PadIndex[][] {
  if (layout === '2x8') {
    return [
      [4, 5, 6, 7, 12, 13, 14, 15],
      [0, 1, 2, 3, 8, 9, 10, 11],
    ];
  }
  return [
    [12, 13, 14, 15],
    [8, 9, 10, 11],
    [4, 5, 6, 7],
    [0, 1, 2, 3],
  ];
}

export const padBank = (pad: PadIndex): 'A' | 'B' => (pad < PAD_COUNT / 2 ? 'A' : 'B');

/** "A1".."A8", "B1".."B8" — how the hardware labels its pads. */
export const padLabel = (pad: PadIndex): string => `${padBank(pad)}${(pad % 8) + 1}`;

export function padColor(settings: Pick<PadSettings, 'color'> | null | undefined, pad: PadIndex): string {
  return settings?.color ?? PAD_COLORS[pad % PAD_COLORS.length];
}

export function padName(settings: Pick<PadSettings, 'sample'> | null | undefined): string {
  const sample = settings?.sample;
  if (!sample) return '';
  if (sample.name) return sample.name;
  const src = sample.source;
  if (src.kind === 'factory') return FACTORY_VOICE_LABELS[src.voice];
  if (src.kind === 'classic') return classicSampleName(src.machine, src.voice);
  return displayName(src.kind === 'pack' ? src.file : src.path);
}

/** MIDI note name with the app-wide octave convention (noteHelpers: C4 = 60, so pad note 36 = C2). */
export function hardwareNoteLabel(note: number | null | undefined): string {
  if (note == null || !Number.isFinite(note)) return '—';
  return midiToNoteName(Math.round(note));
}

/**
 * Velocity 0..1 from where a pad was clicked: top edge = 1, bottom = 0.3.
 * Mirrors a real pad: hit harder by aiming higher.
 */
export function velocityFromPointer(clientY: number, rectTop: number, rectHeight: number): number {
  if (!(rectHeight > 0)) return 0.8;
  const t = Math.min(1, Math.max(0, (clientY - rectTop) / rectHeight));
  return Math.round((1 - t * 0.7) * 100) / 100;
}

// ---------------------------------------------------------------------------
// Computer keyboard pads
// ---------------------------------------------------------------------------

/**
 * 4x4 MPC layout on the left of a QWERTY keyboard (by physical key code).
 * Pad 12 is on T, not R: in the beat view R toggles loop recording.
 */
const KEY_CODE_TO_PAD: Record<string, PadIndex> = {
  KeyZ: 0, KeyX: 1, KeyC: 2, KeyV: 3,
  KeyA: 4, KeyS: 5, KeyD: 6, KeyF: 7,
  KeyQ: 8, KeyW: 9, KeyE: 10, KeyT: 11,
  Digit1: 12, Digit2: 13, Digit3: 14, Digit4: 15,
};
const KEY_TO_PAD: Record<string, PadIndex> = {
  z: 0, x: 1, c: 2, v: 3,
  a: 4, s: 5, d: 6, f: 7,
  q: 8, w: 9, e: 10, t: 11,
  '1': 12, '2': 13, '3': 14, '4': 15,
};

/** Key cap shown on each pad. */
export const PAD_KEY_HINTS: string[] = Array.from({ length: PAD_COUNT }, (_, pad) =>
  (Object.keys(KEY_TO_PAD).find((k) => KEY_TO_PAD[k] === pad) ?? '').toUpperCase(),
);

/** Pad for a key event, or null. Modifier chords (⌘S, Ctrl+Z…) never hit pads. */
export function padForKeyEvent(e: Pick<KeyboardEvent, 'key' | 'code' | 'metaKey' | 'ctrlKey' | 'altKey'>): PadIndex | null {
  if (e.metaKey || e.ctrlKey || e.altKey) return null;
  if (e.code && e.code in KEY_CODE_TO_PAD) return KEY_CODE_TO_PAD[e.code];
  const k = e.key?.toLowerCase();
  return k && k in KEY_TO_PAD ? KEY_TO_PAD[k] : null;
}

/** True while the user is typing (inputs, selects, contenteditable) — pad keys must not fire. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!target || typeof (target as Element).closest !== 'function') return false;
  const el = target as HTMLElement;
  if (el.isContentEditable) return true;
  return !!el.closest('input, textarea, select, [contenteditable=""], [contenteditable="true"], [role="combobox"], [role="textbox"]');
}

// ---------------------------------------------------------------------------
// Step grid
// ---------------------------------------------------------------------------

export type StepGrid = '1/8' | '1/16' | '1/32';
export const STEP_GRIDS: StepGrid[] = ['1/8', '1/16', '1/32'];

export function stepBeatsFor(grid: StepGrid): number {
  return grid === '1/8' ? 0.5 : grid === '1/32' ? 0.125 : 0.25;
}

export function stepCount(bars: number, stepBeats: number): number {
  return Math.max(1, Math.round((bars * 4) / stepBeats));
}

/** Steps per beat-group shading block (one beat). */
export const stepsPerBeat = (stepBeats: number): number => Math.max(1, Math.round(1 / stepBeats));

/**
 * Step positions are shared with the engine. Patterns are stored on the
 * straight grid (step × stepBeats); swing is applied at playback only.
 */
export { patternStepAt } from '@/utils/arrangementHelpers';

/** Note velocity (0..1 or 0..127) as 0..1. */
export const normVelocity = (v: number): number => Math.min(1, Math.max(0, v > 1 ? v / 127 : v));

/**
 * Velocity per cell: `matrix[pad * steps + step]` = 0..1 (0 = empty). Notes are
 * read from the pattern region relative to `patternStartBeat` (the loop start).
 * When several notes fall into one cell the loudest wins. Notes are stored on
 * the straight grid (swing is applied at playback only).
 */
export function buildStepMatrix(
  region: Pick<Region, 'startBeat' | 'notes'> | null,
  patternStartBeat: number,
  stepBeats: number,
  steps: number,
): Float32Array {
  const matrix = new Float32Array(PAD_COUNT * steps);
  if (!region) return matrix;
  for (const n of region.notes) {
    const pad = n.note - padToNote(0);
    if (pad < 0 || pad >= PAD_COUNT) continue;
    const beat = region.startBeat + n.startBeat - patternStartBeat;
    // Same window as the engine's pattern: [0, pattern length).
    if (beat < -STEP_EPSILON || beat >= steps * stepBeats - STEP_EPSILON) continue;
    // Nearest step, clamped: a note in the last half-step stays on the last cell.
    const step = patternStepAt(beat, stepBeats, steps);
    const i = pad * steps + step;
    // Keep a tiny floor so a velocity-0 note still shows as "on".
    matrix[i] = Math.max(matrix[i], Math.max(0.02, normVelocity(n.velocity)));
  }
  return matrix;
}

/** Fractional step (0..steps) of the playhead, or null when it is outside the pattern. */
export function playheadStep(positionBeat: number, patternStartBeat: number, patternBeats: number, stepBeats: number): number | null {
  if (!(patternBeats > 0)) return null;
  const rel = positionBeat - patternStartBeat;
  if (rel < 0 || rel >= patternBeats) return null;
  return rel / stepBeats;
}

/** Cell under a point in canvas CSS pixels, or null. */
export function cellAt(x: number, y: number, cellW: number, rowH: number, steps: number): { pad: PadIndex; step: number } | null {
  if (x < 0 || y < 0 || !(cellW > 0) || !(rowH > 0)) return null;
  const step = Math.floor(x / cellW);
  const row = Math.floor(y / rowH);
  if (step >= steps || row >= PAD_COUNT) return null;
  // Rows are drawn top to bottom from pad 15 down to pad 0 (kick at the bottom, like the pads).
  return { pad: PAD_COUNT - 1 - row, step };
}

export const rowForPad = (pad: PadIndex): number => PAD_COUNT - 1 - pad;

export type PaintMode = 'draw' | 'erase';

/** What a click/drag does: start on an empty cell = draw, on a filled one = erase. */
export const paintModeFor = (cellVelocity: number): PaintMode => (cellVelocity > 0 ? 'erase' : 'draw');

/** Whether painting over a cell needs a toggle to reach the paint mode's state. */
export const needsToggle = (mode: PaintMode, cellVelocity: number): boolean =>
  mode === 'draw' ? cellVelocity <= 0 : cellVelocity > 0;

/** New velocity after an alt-drag (dy px, up = louder) or wheel tick. Never below 0.05. */
export function adjustVelocity(start: number, deltaUp: number, pxPerFull = 120): number {
  const v = start + deltaUp / pxPerFull;
  return Math.round(Math.min(1, Math.max(0.05, v)) * 100) / 100;
}

// ---------------------------------------------------------------------------
// Sample drops
// ---------------------------------------------------------------------------

export interface DroppedSample {
  path: string;
  name: string;
}

type DataTransferLike = Pick<DataTransfer, 'getData' | 'types'> & { files?: ArrayLike<File> | null };

/** Whether a drag carries something a pad accepts (browser sample or files). */
export function dragHasSample(dt: Pick<DataTransfer, 'types'> | null | undefined): boolean {
  if (!dt) return false;
  const types = Array.from(dt.types ?? []);
  return types.includes(SAMPLE_DRAG_MIME) || types.includes('Files');
}

/**
 * Read a dropped sample: the browser's JSON payload first, then the first
 * audio file from Finder (path resolved through Electron).
 */
export function readDroppedSample(
  dt: DataTransferLike | null | undefined,
  getPathForFile?: (file: File) => string,
): DroppedSample | null {
  if (!dt) return null;
  const raw = dt.getData(SAMPLE_DRAG_MIME);
  if (raw) {
    try {
      const p = JSON.parse(raw) as Partial<SampleDragPayload>;
      if (typeof p.path === 'string' && p.path) {
        return { path: p.path, name: typeof p.name === 'string' && p.name ? p.name : displayName(p.path.split(/[\\/]/).pop() ?? p.path) };
      }
    } catch {
      // fall through to files
    }
  }
  const files = dt.files ? Array.from(dt.files) : [];
  for (const file of files) {
    if (!isAudioFileName(file.name)) continue;
    const path = getPathForFile?.(file) ?? '';
    if (path) return { path, name: displayName(file.name) };
  }
  return null;
}

export const fileSample = (s: DroppedSample): PadSample => ({ source: { kind: 'file', path: s.path }, name: s.name });

// ---------------------------------------------------------------------------
// Chop to pads
// ---------------------------------------------------------------------------

export interface ChopAssignment {
  pad: PadIndex;
  startSec: number;
  endSec: number;
}

/** Slices → consecutive pads starting at `fromPad`; slices past pad 15 are dropped. */
export function planChop(fromPad: PadIndex, ranges: { startSec: number; endSec: number }[]): ChopAssignment[] {
  const out: ChopAssignment[] = [];
  for (let i = 0; i < ranges.length; i++) {
    const pad = fromPad + i;
    if (pad >= PAD_COUNT) break;
    out.push({ pad, startSec: ranges[i].startSec, endSec: ranges[i].endSec });
  }
  return out;
}

// ---------------------------------------------------------------------------
// Log-scaled knob params
// ---------------------------------------------------------------------------

/** Map 0..1 onto [min, max] logarithmically (decay, cutoff). */
export const fromLog01 = (t: number, min: number, max: number): number =>
  min * Math.pow(max / min, Math.min(1, Math.max(0, t)));
export const toLog01 = (v: number, min: number, max: number): number =>
  Math.min(1, Math.max(0, Math.log(Math.max(min, v) / min) / Math.log(max / min)));

export function formatSeconds(sec: number): string {
  return sec < 1 ? `${Math.round(sec * 1000)}ms` : `${sec.toFixed(2)}s`;
}

export function formatHz(hz: number): string {
  return hz >= 1000 ? `${(hz / 1000).toFixed(hz >= 10000 ? 0 : 1)}k` : `${Math.round(hz)}`;
}

// ---------------------------------------------------------------------------
// Loop range vs pattern range
// ---------------------------------------------------------------------------

const beatsLabel = (beats: number): string => {
  const bars = beats / 4;
  if (Math.abs(bars - Math.round(bars)) < STEP_EPSILON) {
    const n = Math.round(bars);
    return `${n} bar${n === 1 ? '' : 's'}`;
  }
  return `${+beats.toFixed(2)} beats`;
};

/**
 * Why the transport would not loop exactly the pattern the step grid shows
 * (null = it does). The grid shows `bars` bars from `patternStartBeat`;
 * playback loops `loop` (the arrangement's loop markers) when `loopEnabled`.
 * While Beat Play or a loop take runs, the engine loops the pattern range
 * transiently (useBeat reports that effective loop), so there is no mismatch
 * then; when stopped, `loop` is the stored Song markers (none = no mismatch).
 */
export function describeLoopMismatch(opts: {
  loop: { startBeat: number; endBeat: number } | null | undefined;
  loopEnabled: boolean;
  playing: boolean;
  patternStartBeat: number;
  bars: number;
}): string | null {
  const { loop, loopEnabled, playing, patternStartBeat, bars } = opts;
  if (playing && !loopEnabled) return 'Loop is off: playback runs past the pattern';
  if (!loop) return null;
  const patternBeats = bars * 4;
  const loopBeats = loop.endBeat - loop.startBeat;
  const sameStart = Math.abs(loop.startBeat - patternStartBeat) < STEP_EPSILON;
  const sameLength = Math.abs(loopBeats - patternBeats) < STEP_EPSILON;
  if (sameStart && sameLength) return null;
  if (!sameLength) return `Loop is ${beatsLabel(loopBeats)}, pattern is ${beatsLabel(patternBeats)}`;
  return 'Loop range starts away from the pattern';
}
