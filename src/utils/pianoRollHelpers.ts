import { DRUM_SOUNDS } from '@/utils/constants';

// Piano roll uses MIDI range 24 (C1) to 96 (C7) — 72 notes
export const PR_NOTE_MIN = 24;
export const PR_NOTE_MAX = 96;
export const PR_NOTE_COUNT = PR_NOTE_MAX - PR_NOTE_MIN;

// Drum lane: maps row index to drum pad info
export const DRUM_ROWS = DRUM_SOUNDS.map((d) => ({
  midiNote: d.midiNote,
  name: d.shortName,
  color: d.color,
}));
export const PR_DRUM_COUNT = DRUM_ROWS.length;

export const DEFAULT_NOTE_VELOCITY = 0.8;
export const DEFAULT_NOTE_DURATION = 1; // 1 beat
export const DEFAULT_DRUM_DURATION = 0.25;

export function beatToGridX(beat: number, pxPerBeat: number): number {
  return beat * pxPerBeat;
}

export function gridXToBeat(x: number, pxPerBeat: number): number {
  return x / pxPerBeat;
}

/** Convert MIDI note (higher = top) to Y pixel position */
export function noteToGridY(midiNote: number, rowHeight: number): number {
  // Higher notes at top → invert
  return (PR_NOTE_MAX - midiNote - 1) * rowHeight;
}

export function gridYToNote(y: number, rowHeight: number): number {
  return PR_NOTE_MAX - Math.floor(y / rowHeight) - 1;
}

/** For drum grid: row index (0=top) → Y */
export function drumRowToGridY(rowIndex: number, rowHeight: number): number {
  return rowIndex * rowHeight;
}

/** Y → drum row index */
export function gridYToDrumRow(y: number, rowHeight: number): number {
  return Math.floor(y / rowHeight);
}

/** Get drum MIDI note from row index */
export function drumRowToMidi(rowIndex: number): number {
  return DRUM_ROWS[rowIndex]?.midiNote ?? 36;
}

/** Get drum row index from MIDI note */
export function drumMidiToRow(midiNote: number): number {
  return DRUM_ROWS.findIndex((d) => d.midiNote === midiNote);
}

export function snapBeatPR(beat: number, snapValue: number): number {
  if (snapValue <= 0) return beat;
  return Math.round(beat / snapValue) * snapValue;
}

export function snapBeatFloor(beat: number, snapValue: number): number {
  if (snapValue <= 0) return beat;
  return Math.floor(beat / snapValue) * snapValue;
}

/** Quantize notes by snapping startBeat to the nearest grid position */
export function quantizeNotes(notes: import('@/types/arrangement').RegionNote[], snapValue: number): import('@/types/arrangement').RegionNote[] {
  if (snapValue <= 0) return notes;
  return notes.map((note) => ({
    ...note,
    startBeat: snapBeatPR(note.startBeat, snapValue),
  }));
}
