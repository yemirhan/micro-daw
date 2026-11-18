import { PAD_COUNT, type PadIndex } from '@/types/sampler';

/**
 * Split routing mode: a keyboard split. Notes below the split point trigger pads
 * chromatically (the 16 notes just below the split are pads 0-15, i.e.
 * `note = split - 16 + pad`; lower notes repeat that layout every 16 semitones),
 * ignoring the controller profile's pad map. Notes at/above it play the synth.
 *
 * The split point is stored in localStorage under MIDI_SPLIT_POINT_KEY as a JSON
 * number (the Rust port uses the same key and shape).
 */
export const MIDI_SPLIT_POINT_KEY = 'micro-daw:midi-split-point';
/** C4. */
export const DEFAULT_SPLIT_POINT = 60;

/** An integer MIDI note 0..127. */
export function clampSplitPoint(note: number): number {
  if (!Number.isFinite(note)) return DEFAULT_SPLIT_POINT;
  return Math.min(127, Math.max(0, Math.trunc(note)));
}

/** Pad a note below the split triggers (null at/above the split). */
export function splitPad(note: number, split: number): PadIndex | null {
  if (!(note < split)) return null;
  const k = (note - split + PAD_COUNT) % PAD_COUNT;
  return ((k + PAD_COUNT) % PAD_COUNT) as PadIndex;
}

export function readSplitPoint(): number {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(MIDI_SPLIT_POINT_KEY) : null;
    if (raw === null) return DEFAULT_SPLIT_POINT;
    const v: unknown = JSON.parse(raw);
    return typeof v === 'number' && Number.isFinite(v) ? clampSplitPoint(v) : DEFAULT_SPLIT_POINT;
  } catch {
    return DEFAULT_SPLIT_POINT;
  }
}

/** Stores the clamped split point and returns it. */
export function writeSplitPoint(note: number): number {
  const v = clampSplitPoint(note);
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(MIDI_SPLIT_POINT_KEY, JSON.stringify(v));
  } catch {
    // storage full / unavailable: the value still applies for this session
  }
  return v;
}
