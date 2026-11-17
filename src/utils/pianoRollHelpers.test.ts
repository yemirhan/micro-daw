import { describe, expect, it } from 'vitest';
import type { RegionNote } from '@/types/arrangement';
import {
  appendCopies,
  applyClickSelection,
  centerScrollTop,
  changeLengths,
  clampMoveDelta,
  computeGridLines,
  computeRulerLabels,
  copyNotes,
  createDrumLayout,
  createPianoLayout,
  deleteNotes,
  drawSpan,
  duplicateNotes,
  fitView,
  followScroll,
  formatBeats,
  formatPosition,
  gridSubdivision,
  hitTestNote,
  isShadedRow,
  MIN_NOTE_LENGTH,
  moveNotes,
  notesAtX,
  notesInRect,
  offsetVelocities,
  pasteNotes,
  quantizeNotes,
  rampVelocities,
  requiredRegionLength,
  resizeNotes,
  roundBeat,
  setVelocities,
  sliderToZoom,
  snapBeat,
  snapCeil,
  snapFloor,
  snappedMoveDelta,
  transposeNotes,
  velocityToMidi,
  zoomFromWheel,
  zoomScroll,
  zoomToSlider,
} from './pianoRollHelpers';

const n = (note: number, startBeat: number, durationBeats = 1, velocity = 0.8): RegionNote => ({
  note,
  startBeat,
  durationBeats,
  velocity,
  isDrum: false,
});

const piano = createPianoLayout();

describe('layouts', () => {
  it('maps pitches to rows with the highest pitch on top', () => {
    expect(piano.rowCount).toBe(128);
    expect(piano.pitchToRow(127)).toBe(0);
    expect(piano.pitchToRow(0)).toBe(127);
    expect(piano.rowToPitch(piano.pitchToRow(60))).toBe(60);
    expect(piano.pitchToRow(128)).toBe(-1);
    expect(piano.rowLabel(piano.pitchToRow(60))).toBe('C4');
    expect(piano.rowToPitch(-5)).toBe(127); // clamped
  });

  it('shades black-key rows', () => {
    expect(isShadedRow(piano, piano.pitchToRow(61))).toBe(true);
    expect(isShadedRow(piano, piano.pitchToRow(60))).toBe(false);
  });

  it('drum layout lists GM pads first and adds rows for unknown pitches', () => {
    const drums = createDrumLayout([36, 37, 38]);
    expect(drums.rowToPitch(0)).toBe(36);
    expect(drums.rowLabel(0)).toBe('Kick');
    expect(drums.rowCount).toBe(9); // 8 pads + note 37
    expect(drums.pitchToRow(37)).toBe(8);
    expect(drums.rowLabel(8)).toBe('C#2');
    expect(drums.pitchToRow(99)).toBe(-1);
  });
});

describe('snapping', () => {
  it('snaps to nearest / floor / ceil grid lines', () => {
    expect(snapBeat(0.3, 0.25)).toBe(0.25);
    expect(snapBeat(0.4, 0.25)).toBe(0.5);
    expect(snapFloor(0.49, 0.25)).toBe(0.25);
    expect(snapCeil(0.26, 0.25)).toBe(0.5);
    expect(snapCeil(0.5, 0.25)).toBe(0.5);
  });

  it('snap off only rounds to ticks', () => {
    expect(snapBeat(0.1 + 0.2, 0)).toBe(0.3);
  });

  it('respects the region offset so the grid follows song bars', () => {
    // Region starts at beat 2: bar lines are at relative 2, 6, ...
    expect(snapBeat(1.4, 4, 2)).toBe(2);
    expect(snapBeat(0.9, 4, 2)).toBe(2);
    expect(snapBeat(-0.5, 4, 2)).toBe(-2);
  });

  it('handles triplets without float drift', () => {
    const t = 1 / 3;
    expect(snapBeat(0.34, t)).toBeCloseTo(1 / 3, 3);
    expect(snapBeat(0.9, t)).toBe(1);
    expect(roundBeat(0.1 + 0.2)).toBe(0.3);
  });
});

describe('formatting', () => {
  it('formats lengths', () => {
    expect(formatBeats(0.25)).toBe('1/16');
    expect(formatBeats(0.75)).toBe('1/8 .');
    expect(formatBeats(1 / 3)).toBe('1/8 T');
    expect(formatBeats(8)).toBe('2 bars');
    expect(formatBeats(0.625)).toBe('5/32');
  });

  it('formats positions as bar.beat.sixteenth', () => {
    expect(formatPosition(0)).toBe('1.1.1');
    expect(formatPosition(5.25)).toBe('2.2.2');
  });

  it('converts velocity for display', () => {
    expect(velocityToMidi(1)).toBe(127);
    expect(velocityToMidi(0)).toBe(1);
  });
});

describe('grid generation', () => {
  it('produces bar, beat and subdivision levels', () => {
    const lines = computeGridLines(0, 4, 64, 0.25);
    expect(lines).toHaveLength(17);
    expect(lines[0]).toEqual({ beat: 0, level: 0 });
    expect(lines[1]).toEqual({ beat: 0.25, level: 2 });
    expect(lines[4]).toEqual({ beat: 1, level: 1 });
    expect(lines[16]).toEqual({ beat: 4, level: 0 });
  });

  it('coarsens subdivisions that would be too dense', () => {
    expect(gridSubdivision(0.125, 8)).toBe(1);
    const lines = computeGridLines(0, 8, 8, 0.125);
    expect(lines.every((l) => l.level < 2)).toBe(true);
  });

  it('includes triplet subdivisions and keeps beat lines', () => {
    const lines = computeGridLines(0, 1, 120, 1 / 3);
    expect(lines.map((l) => l.level)).toEqual([0, 2, 2, 1]);
  });

  it('thins out bars when zoomed far out', () => {
    const lines = computeGridLines(0, 64, 1, 0.25);
    expect(lines.every((l) => l.level === 0)).toBe(true);
    expect(lines.length).toBeLessThan(17);
  });

  it('labels bars using absolute song positions', () => {
    const labels = computeRulerLabels(0, 8, 12, 4);
    expect(labels.map((l) => l.label)).toEqual(['2', '3', '4']);
    const fine = computeRulerLabels(0, 1, 60);
    expect(fine.map((l) => l.label)).toEqual(['1', '1.2']);
  });
});

describe('hit-testing and selection', () => {
  const notes = [n(60, 0, 1), n(60, 2, 1), n(64, 0, 4)];
  const px = 40;
  const rh = 10;
  const yOf = (pitch: number) => piano.pitchToRow(pitch) * rh + 5;

  it('finds the note body and edges', () => {
    expect(hitTestNote(notes, 20, yOf(60), piano, px, rh)).toEqual({ index: 0, edge: 'body' });
    expect(hitTestNote(notes, 38, yOf(60), piano, px, rh)).toEqual({ index: 0, edge: 'end' });
    expect(hitTestNote(notes, 81, yOf(60), piano, px, rh)).toEqual({ index: 1, edge: 'start' });
    expect(hitTestNote(notes, 60, yOf(60), piano, px, rh)).toBeNull();
    expect(hitTestNote(notes, 20, yOf(62), piano, px, rh)).toBeNull();
  });

  it('never reports edges when not resizable', () => {
    expect(hitTestNote(notes, 38, yOf(60), piano, px, rh, { resizable: false })?.edge).toBe('body');
  });

  it('prefers selected notes when overlapping', () => {
    const stacked = [n(60, 0, 2), n(60, 0, 2)];
    expect(hitTestNote(stacked, 20, yOf(60), piano, px, rh)?.index).toBe(1);
    expect(hitTestNote(stacked, 20, yOf(60), piano, px, rh, { selected: new Set([0]) })?.index).toBe(0);
  });

  it('keeps very short notes grabbable', () => {
    const tiny = [n(60, 1, 0.01)];
    expect(hitTestNote(tiny, 42, yOf(60), piano, px, rh)?.index).toBe(0);
  });

  it('selects notes inside a rubber band', () => {
    const r60 = piano.pitchToRow(60);
    const r64 = piano.pitchToRow(64);
    expect(notesInRect(notes, 0.5, 1.5, r60, r60, piano)).toEqual([0]);
    expect(notesInRect(notes, 0.5, 2.5, r64, r60, piano)).toEqual([0, 1, 2]);
  });

  it('click selection semantics', () => {
    expect([...applyClickSelection(new Set([1, 2]), 3, false)]).toEqual([3]);
    expect([...applyClickSelection(new Set([1, 2]), 2, false)]).toEqual([1, 2]);
    expect([...applyClickSelection(new Set([1, 2]), 2, true)]).toEqual([1]);
    expect([...applyClickSelection(new Set([1]), 2, true)]).toEqual([1, 2]);
  });
});

describe('move / transpose', () => {
  it('moves selected notes by beats and rows', () => {
    const notes = [n(60, 1), n(62, 2), n(64, 3)];
    const out = moveNotes(notes, [0, 1], 1, -2, piano);
    expect(out[0]).toMatchObject({ note: 62, startBeat: 2 });
    expect(out[1]).toMatchObject({ note: 64, startBeat: 3 });
    expect(out[2]).toBe(notes[2]);
  });

  it('clamps so notes never go before 0 or out of range', () => {
    const notes = [n(126, 0.5), n(60, 2)];
    expect(clampMoveDelta(notes, [0, 1], -3, -5, piano)).toEqual({ dBeat: -0.5, dRow: -1 });
    const out = moveNotes(notes, [0, 1], -3, -5, piano);
    expect(out.map((x) => [x.note, x.startBeat])).toEqual([[127, 0], [61, 1.5]]);
  });

  it('snaps the grabbed note onto the grid', () => {
    expect(snappedMoveDelta(0.1, 0.5, 0.25)).toBe(0.4);
    expect(snappedMoveDelta(0.1, 0.5, 0)).toBe(0.5);
  });

  it('transposes by semitones/octaves', () => {
    const notes = [n(60, 0)];
    expect(transposeNotes(notes, [0], 12, piano)[0].note).toBe(72);
    expect(transposeNotes(notes, [0], -1, piano)[0].note).toBe(59);
  });

  it('moves drum notes between pad rows', () => {
    const drums = createDrumLayout();
    const notes = [{ ...n(36, 0, 0.25), isDrum: true }];
    expect(moveNotes(notes, [0], 0, 1, drums)[0].note).toBe(38);
    expect(moveNotes(notes, [0], 0, -1, drums)[0].note).toBe(36); // clamped at top
  });
});

describe('resize / draw / length', () => {
  it('resizes from the end, snapping the anchor edge', () => {
    const notes = [n(60, 0, 1), n(62, 1, 0.5)];
    const out = resizeNotes(notes, [0, 1], 'end', 0, 0.6, 0.25);
    expect(out[0].durationBeats).toBe(1.5);
    expect(out[1].durationBeats).toBe(1);
  });

  it('resizes from the start keeping the end fixed', () => {
    const notes = [n(60, 1, 1)];
    const out = resizeNotes(notes, [0], 'start', 0, -0.4, 0.25);
    expect(out[0]).toMatchObject({ startBeat: 0.5, durationBeats: 1.5 });
    const shrunk = resizeNotes(notes, [0], 'start', 0, 5, 0.25);
    expect(shrunk[0]).toMatchObject({ startBeat: 1.75, durationBeats: 0.25 });
  });

  it('does not stretch short notes up to the grid just by grabbing them', () => {
    const notes = [n(60, 0, 0.1)];
    expect(resizeNotes(notes, [0], 'end', 0, 0, 0.25)[0].durationBeats).toBeCloseTo(0.1, 2);
    expect(resizeNotes(notes, [0], 'end', 0, -1, 0)[0].durationBeats).toBe(MIN_NOTE_LENGTH);
  });

  it('draw span extends to the nearest grid line, min one step', () => {
    expect(drawSpan(1, 1.05, 0.25)).toEqual({ startBeat: 1, durationBeats: 0.25 });
    expect(drawSpan(1, 2.4, 0.25)).toEqual({ startBeat: 1, durationBeats: 1.5 });
    expect(drawSpan(1, 0.3, 0.25)).toEqual({ startBeat: 0.25, durationBeats: 1 });
  });

  it('changes lengths by a step', () => {
    expect(changeLengths([n(60, 0, 1)], [0], -2)[0].durationBeats).toBe(MIN_NOTE_LENGTH);
    expect(changeLengths([n(60, 0, 1)], [0], 0.25)[0].durationBeats).toBe(1.25);
  });
});

describe('delete / duplicate / clipboard', () => {
  const notes = [n(60, 0, 1), n(64, 1, 1), n(67, 2, 0.5)];

  it('deletes', () => {
    expect(deleteNotes(notes, [0, 2]).map((x) => x.note)).toEqual([64]);
  });

  it('duplicates the selection after itself, grid aligned', () => {
    const { notes: out, selection } = duplicateNotes(notes, [0, 1], 0.25);
    expect(out).toHaveLength(5);
    expect(selection).toEqual([3, 4]);
    expect(out[3]).toMatchObject({ note: 60, startBeat: 2 });
    expect(out[4]).toMatchObject({ note: 64, startBeat: 3 });
    // span 2.5 beats with a bar grid -> shift by a whole bar
    expect(duplicateNotes(notes, [0, 1, 2], 4).notes[3].startBeat).toBe(4);
  });

  it('appends shifted copies (alt-drag duplicate)', () => {
    const { notes: out, selection } = appendCopies(notes, [2], 1, -12, piano);
    expect(selection).toEqual([3]);
    expect(out[3]).toMatchObject({ note: 79, startBeat: 3 });
    expect(out[2]).toBe(notes[2]);
  });

  it('copy normalises and paste places at a beat', () => {
    const clip = copyNotes(notes, [1, 2]);
    expect(clip.map((x) => x.startBeat)).toEqual([0, 1]);
    const { notes: out, selection } = pasteNotes(notes, clip, 8);
    expect(selection).toEqual([3, 4]);
    expect(out[3].startBeat).toBe(8);
    expect(out[4].startBeat).toBe(9);
    expect(pasteNotes([], clip, 0, true).notes.every((x) => x.isDrum)).toBe(true);
  });
});

describe('quantize', () => {
  const notes = [n(60, 0.1, 0.9), n(62, 1.13, 0.3)];

  it('quantizes starts of all notes (legacy signature)', () => {
    expect(quantizeNotes(notes, 0.25).map((x) => x.startBeat)).toEqual([0, 1.25]);
    expect(quantizeNotes(notes, 0)).toBe(notes);
  });

  it('quantizes only selected notes, optionally ends', () => {
    const out = quantizeNotes(notes, 0.25, { indices: [1], ends: true });
    expect(out[0]).toBe(notes[0]);
    expect(out[1]).toMatchObject({ startBeat: 1.25, durationBeats: 0.25 });
  });

  it('supports partial strength', () => {
    expect(quantizeNotes([n(60, 0.2)], 1, { strength: 0.5 })[0].startBeat).toBe(0.1);
  });
});

describe('velocity', () => {
  const notes = [n(60, 0, 1, 0.5), n(62, 1, 1, 0.5), n(64, 2, 1, 0.5), n(65, 3, 1, 0.5)];

  it('sets and offsets with clamping', () => {
    expect(setVelocities(notes, [0], 2)[0].velocity).toBe(1);
    expect(offsetVelocities(notes, [0, 1], -1).slice(0, 2).every((x) => x.velocity > 0)).toBe(true);
    expect(offsetVelocities(notes, [2], 0.25)[2].velocity).toBe(0.75);
  });

  it('ramps linearly across the dragged range', () => {
    const out = rampVelocities(notes, null, 0, 0.2, 3, 0.8);
    expect(out.map((x) => Number(x.velocity.toFixed(2)))).toEqual([0.2, 0.4, 0.6, 0.8]);
    const onlySome = rampVelocities(notes, [1], 0, 0.2, 3, 0.8);
    expect(onlySome[0].velocity).toBe(0.5);
    expect(onlySome[1].velocity).toBeCloseTo(0.4);
  });

  it('finds stems at x', () => {
    expect(notesAtX(notes, 41, 40)).toEqual([1]);
    expect(notesAtX(notes, 41, 40, 5, new Set([0]))).toEqual([]);
  });
});

describe('view helpers', () => {
  it('extends regions to whole bars, never shrinks', () => {
    expect(requiredRegionLength([n(60, 7, 2)], 8)).toBe(12);
    expect(requiredRegionLength([n(60, 0, 1)], 8)).toBe(8);
  });

  it('zooms around the cursor', () => {
    // Content x under cursor: (100 + 50) / 10 = beat 15; at 20 px/beat that's 300 -> scroll 250
    expect(zoomScroll(100, 50, 10, 20)).toBe(250);
    expect(zoomFromWheel(100, -100, 10, 120)).toBeGreaterThan(100);
    expect(zoomFromWheel(100, 10000, 10, 120)).toBe(10);
    expect(sliderToZoom(zoomToSlider(64, 6, 640), 6, 640)).toBeCloseTo(64);
  });

  it('centres the notes vertically on open', () => {
    const notes = [n(60, 0), n(72, 1)];
    const top = centerScrollTop(notes, piano, 10, 200);
    const centerRow = (top + 100) / 10;
    expect(centerRow).toBeGreaterThan(piano.pitchToRow(72));
    expect(centerRow).toBeLessThan(piano.pitchToRow(60) + 1);
    expect(centerScrollTop([], createDrumLayout(), 10, 200)).toBe(0);
  });

  it('fits the region into the viewport', () => {
    const fit = fitView([n(60, 0, 1), n(72, 15, 1)], piano, 16, 816, 340);
    expect(fit.pxPerBeat).toBe(50);
    expect(fit.rowHeight).toBe(20);
  });

  it('follows the playhead page by page', () => {
    expect(followScroll(500, 0, 1000)).toBeNull();
    expect(followScroll(990, 0, 1000)).toBe(890);
    expect(followScroll(10, 400, 1000)).toBe(0);
  });
});
