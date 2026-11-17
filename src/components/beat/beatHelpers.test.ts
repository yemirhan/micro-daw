import { describe, expect, it } from 'vitest';
import { PAD_COUNT, padToNote } from '@/types/sampler';
import { SAMPLE_DRAG_MIME } from '@/components/browser/browserHelpers';
import { midiToNoteName } from '@/utils/noteHelpers';
import {
  PAD_KEY_HINTS,
  adjustVelocity,
  buildStepMatrix,
  cellAt,
  dragHasSample,
  fromLog01,
  hardwareNoteLabel,
  isTypingTarget,
  needsToggle,
  padForKeyEvent,
  padLabel,
  padRows,
  paintModeFor,
  planChop,
  playheadStep,
  readDroppedSample,
  rowForPad,
  stepBeatsFor,
  stepCount,
  toLog01,
  velocityFromPointer,
} from './beatHelpers';

const key = (k: string, code: string, mods: Partial<Record<'metaKey' | 'ctrlKey' | 'altKey', boolean>> = {}) => ({
  key: k,
  code,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

describe('pad layout', () => {
  it('4x4 puts pad 0 bottom-left and pad 15 top-right (MPC)', () => {
    const rows = padRows('4x4');
    expect(rows).toHaveLength(4);
    expect(rows[3][0]).toBe(0);
    expect(rows[0][3]).toBe(15);
    expect(rows.flat().sort((a, b) => a - b)).toEqual(Array.from({ length: PAD_COUNT }, (_, i) => i));
  });

  it('2x8 shows bank A and bank B side by side, each with its first pad bottom-left', () => {
    const rows = padRows('2x8');
    expect(rows).toEqual([
      [4, 5, 6, 7, 12, 13, 14, 15],
      [0, 1, 2, 3, 8, 9, 10, 11],
    ]);
  });

  it('labels pads like the hardware banks', () => {
    expect(padLabel(0)).toBe('A1');
    expect(padLabel(7)).toBe('A8');
    expect(padLabel(8)).toBe('B1');
    expect(padLabel(15)).toBe('B8');
  });

  it('names hardware notes like the rest of the app (noteHelpers: C4 = 60)', () => {
    expect(hardwareNoteLabel(36)).toBe('C2');
    expect(hardwareNoteLabel(36)).toBe(midiToNoteName(36));
    expect(hardwareNoteLabel(60)).toBe('C4');
    expect(hardwareNoteLabel(null)).toBe('—');
  });

  it('derives velocity from the vertical click position', () => {
    expect(velocityFromPointer(0, 0, 100)).toBe(1);
    expect(velocityFromPointer(100, 0, 100)).toBe(0.3);
    expect(velocityFromPointer(50, 0, 100)).toBeCloseTo(0.65);
    expect(velocityFromPointer(10, 0, 0)).toBe(0.8);
  });
});

describe('computer keyboard pads', () => {
  it('maps ZXCV / ASDF / QWET / 1234 to pads 0..15 (R is loop record)', () => {
    expect(padForKeyEvent(key('z', 'KeyZ'))).toBe(0);
    expect(padForKeyEvent(key('v', 'KeyV'))).toBe(3);
    expect(padForKeyEvent(key('a', 'KeyA'))).toBe(4);
    expect(padForKeyEvent(key('q', 'KeyQ'))).toBe(8);
    expect(padForKeyEvent(key('t', 'KeyT'))).toBe(11);
    expect(padForKeyEvent(key('r', 'KeyR'))).toBeNull();
    expect(padForKeyEvent(key('1', 'Digit1'))).toBe(12);
    expect(padForKeyEvent(key('4', 'Digit4'))).toBe(15);
  });

  it('uses the physical key (non-QWERTY layouts) and falls back to the character', () => {
    expect(padForKeyEvent(key('y', 'KeyZ'))).toBe(0); // German layout: Z key prints y
    expect(padForKeyEvent(key('Z', ''))).toBe(0);
  });

  it('ignores other keys and modifier chords', () => {
    expect(padForKeyEvent(key('5', 'Digit5'))).toBeNull();
    expect(padForKeyEvent(key('g', 'KeyG'))).toBeNull();
    expect(padForKeyEvent(key('s', 'KeyS', { metaKey: true }))).toBeNull();
    expect(padForKeyEvent(key('z', 'KeyZ', { ctrlKey: true }))).toBeNull();
  });

  it('has a key hint for every pad', () => {
    expect(PAD_KEY_HINTS).toHaveLength(16);
    expect(PAD_KEY_HINTS[0]).toBe('Z');
    expect(PAD_KEY_HINTS[15]).toBe('4');
  });

  it('detects typing targets', () => {
    const input = document.createElement('input');
    const div = document.createElement('div');
    const editable = document.createElement('div');
    editable.setAttribute('contenteditable', 'true');
    const inner = document.createElement('span');
    editable.appendChild(inner);
    expect(isTypingTarget(input)).toBe(true);
    expect(isTypingTarget(inner)).toBe(true);
    expect(isTypingTarget(div)).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget(window)).toBe(false);
  });
});

describe('step grid', () => {
  it('step size and count', () => {
    expect(stepBeatsFor('1/8')).toBe(0.5);
    expect(stepBeatsFor('1/16')).toBe(0.25);
    expect(stepBeatsFor('1/32')).toBe(0.125);
    expect(stepCount(1, 0.25)).toBe(16);
    expect(stepCount(2, 0.25)).toBe(32);
    expect(stepCount(8, 0.125)).toBe(256);
  });

  it('builds the cell matrix from pattern notes (relative to the pattern start)', () => {
    const region = {
      startBeat: 8,
      notes: [
        { note: padToNote(0), velocity: 1, startBeat: 0, durationBeats: 0.25, isDrum: true },
        { note: padToNote(1), velocity: 0.5, startBeat: 1, durationBeats: 0.25, isDrum: true },
        { note: padToNote(15), velocity: 64, startBeat: 3.75, durationBeats: 0.25, isDrum: true }, // 0..127 velocity
        { note: 20, velocity: 1, startBeat: 0, durationBeats: 0.25, isDrum: true }, // not a pad note
        { note: padToNote(2), velocity: 1, startBeat: 4, durationBeats: 0.25, isDrum: true }, // past a 1-bar pattern
      ],
    };
    const m = buildStepMatrix(region, 8, 0.25, 16, 0);
    expect(m[0 * 16 + 0]).toBe(1);
    expect(m[1 * 16 + 4]).toBeCloseTo(0.5);
    expect(m[15 * 16 + 15]).toBeCloseTo(64 / 127);
    expect(m.filter((v) => v > 0)).toHaveLength(3);
  });

  it('maps straight-stored notes to their step regardless of swing', () => {
    const notes = [1, 2, 3].map((step) => ({
      note: padToNote(4),
      velocity: 0.8,
      startBeat: step * 0.25,
      durationBeats: 0.25,
      isDrum: true,
    }));
    for (const swing of [0, 0.5]) {
      const m = buildStepMatrix({ startBeat: 0, notes }, 0, 0.25, 16, swing);
      expect([...m.slice(4 * 16, 4 * 16 + 5)].map((v) => v > 0)).toEqual([false, true, true, true, false]);
    }
  });

  it('shows a note in the last half-step on the last step (never invisible)', () => {
    const notes = [3.8, 3.95].map((startBeat, i) => ({
      note: padToNote(i),
      velocity: 0.8,
      startBeat,
      durationBeats: 0.25,
      isDrum: true,
    }));
    const m = buildStepMatrix({ startBeat: 0, notes }, 0, 0.25, 16);
    expect(m[0 * 16 + 15]).toBeGreaterThan(0);
    expect(m[1 * 16 + 15]).toBeGreaterThan(0);
    expect(m.filter((v) => v > 0)).toHaveLength(2);
  });

  it('returns an empty matrix without a region', () => {
    expect(buildStepMatrix(null, 0, 0.25, 16, 0).every((v) => v === 0)).toBe(true);
  });

  it('finds the cell under a point with pad 0 on the bottom row', () => {
    expect(cellAt(0, 0, 20, 10, 16)).toEqual({ pad: 15, step: 0 });
    expect(cellAt(39, 155, 20, 10, 16)).toEqual({ pad: 0, step: 1 });
    expect(cellAt(320, 5, 20, 10, 16)).toBeNull();
    expect(cellAt(5, 160, 20, 10, 16)).toBeNull();
    expect(cellAt(-1, 5, 20, 10, 16)).toBeNull();
    expect(rowForPad(0)).toBe(15);
  });

  it('paint mode is decided by the first cell, and only cells that differ toggle', () => {
    expect(paintModeFor(0)).toBe('draw');
    expect(paintModeFor(0.7)).toBe('erase');
    expect(needsToggle('draw', 0)).toBe(true);
    expect(needsToggle('draw', 0.5)).toBe(false);
    expect(needsToggle('erase', 0.5)).toBe(true);
    expect(needsToggle('erase', 0)).toBe(false);
  });

  it('adjusts velocity by drag distance, clamped to 0.05..1', () => {
    expect(adjustVelocity(0.5, 60)).toBe(1);
    expect(adjustVelocity(0.5, 12)).toBe(0.6);
    expect(adjustVelocity(0.5, -1000)).toBe(0.05);
  });

  it('places the playhead inside the pattern only', () => {
    expect(playheadStep(9, 8, 4, 0.25)).toBe(4);
    expect(playheadStep(7.9, 8, 4, 0.25)).toBeNull();
    expect(playheadStep(12, 8, 4, 0.25)).toBeNull();
  });
});

describe('sample drops', () => {
  const dt = (data: Record<string, string>, files: File[] = []) => ({
    types: [...Object.keys(data), ...(files.length ? ['Files'] : [])],
    getData: (t: string) => data[t] ?? '',
    files,
  });

  it('reads the browser payload', () => {
    const d = dt({ [SAMPLE_DRAG_MIME]: JSON.stringify({ path: '/s/Kick 01.wav', name: 'Kick 01' }) });
    expect(dragHasSample(d)).toBe(true);
    expect(readDroppedSample(d)).toEqual({ path: '/s/Kick 01.wav', name: 'Kick 01' });
  });

  it('reads the first audio file from Finder through getPathForFile', () => {
    const files = [new File([''], 'notes.txt'), new File([''], 'Snare.WAV')];
    const d = dt({}, files);
    expect(dragHasSample(d)).toBe(true);
    expect(readDroppedSample(d, (f) => `/drop/${f.name}`)).toEqual({ path: '/drop/Snare.WAV', name: 'Snare' });
  });

  it('rejects unusable drops', () => {
    expect(readDroppedSample(dt({ [SAMPLE_DRAG_MIME]: '{broken' }))).toBeNull();
    expect(readDroppedSample(dt({}, [new File([''], 'a.wav')]), () => '')).toBeNull();
    expect(readDroppedSample(dt({}, [new File([''], 'a.wav')]))).toBeNull();
    expect(dragHasSample(dt({ 'text/plain': 'x' }))).toBe(false);
    expect(readDroppedSample(null)).toBeNull();
  });
});

describe('chop + knob helpers', () => {
  it('assigns slices to consecutive pads and drops the ones past pad 16', () => {
    const ranges = Array.from({ length: 8 }, (_, i) => ({ startSec: i, endSec: i + 1 }));
    expect(planChop(2, ranges.slice(0, 3))).toEqual([
      { pad: 2, startSec: 0, endSec: 1 },
      { pad: 3, startSec: 1, endSec: 2 },
      { pad: 4, startSec: 2, endSec: 3 },
    ]);
    const tail = planChop(12, ranges);
    expect(tail.map((a) => a.pad)).toEqual([12, 13, 14, 15]);
  });

  it('log mapping round-trips', () => {
    expect(fromLog01(0, 20, 20000)).toBeCloseTo(20);
    expect(fromLog01(1, 20, 20000)).toBeCloseTo(20000);
    expect(toLog01(fromLog01(0.37, 0.005, 4), 0.005, 4)).toBeCloseTo(0.37);
  });
});
