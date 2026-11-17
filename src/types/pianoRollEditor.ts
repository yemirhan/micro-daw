import type { RegionNote } from '@/types/arrangement';

export type PianoRollTool = 'pointer' | 'draw' | 'eraser';

/** Which part of a note the pointer is over. */
export type NoteEdge = 'body' | 'start' | 'end';

export interface NoteHit {
  index: number;
  edge: NoteEdge;
}

/** A grid/snap choice. `beats === 0` means snapping is off. */
export interface SnapOption {
  id: string;
  label: string;
  beats: number;
}

/** Maps MIDI pitches to vertical rows (row 0 = top). */
export interface RowLayout {
  kind: 'piano' | 'drum';
  rowCount: number;
  /** Row index for a pitch, or -1 if the pitch has no row. */
  pitchToRow: (pitch: number) => number;
  /** Pitch for a row index (clamped into range). */
  rowToPitch: (row: number) => number;
  /** Label for a row (note name / drum pad name). */
  rowLabel: (row: number) => string;
  /** Optional per-row accent color (drum pads). */
  rowColor?: (row: number) => string | undefined;
}

/** A vertical line in the note grid / ruler. level 0 = bar, 1 = beat, 2 = subdivision. */
export interface GridLine {
  beat: number;
  level: 0 | 1 | 2;
}

export interface RulerLabel {
  beat: number;
  label: string;
  /** true for bar starts */
  major: boolean;
}

/** Result of an edit that also changes the selection. */
export interface NotesEdit {
  notes: RegionNote[];
  selection: number[];
}

/** Last pointer position over the grid, for status display + key highlight. */
export interface GridHover {
  beat: number;
  row: number;
  pitch: number;
}

/** Zoom + scroll state shared by every piano roll pane. */
export interface PianoRollViewport {
  pxPerBeat: number;
  rowHeight: number;
  scrollLeft: number;
  scrollTop: number;
  /** Visible size of the note grid (px). */
  width: number;
  height: number;
}
