export type PianoRollTool = 'pointer' | 'draw' | 'eraser';

export interface NoteSelection {
  noteIndices: Set<number>;
}

export interface DragState {
  type: 'move' | 'resize-right' | 'resize-left' | 'rubber-band' | 'draw-extend';
  startX: number;
  startY: number;
  originBeat: number;
  originNote: number;
  /** For move: initial beats/notes of selected notes */
  initialPositions?: Array<{ startBeat: number; note: number }>;
  /** For resize: initial durations */
  initialDurations?: number[];
  /** For rubber-band selection */
  currentX?: number;
  currentY?: number;
  /** For draw-extend: index of the note being drawn */
  drawNoteIndex?: number;
}
