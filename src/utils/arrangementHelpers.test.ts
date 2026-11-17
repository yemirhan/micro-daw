import { describe, expect, it } from 'vitest';
import type { RegionNote, Track } from '@/types/arrangement';
import { padToNote } from '@/types/sampler';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { ALL_TEMPLATES } from '@/data/templates';
import {
  DEFAULT_LOOP_SETTINGS,
  drumNoteToGm,
  gridStepBeats,
  mergeDrumHit,
  migrateDrumTrack,
  migrateGmDrumNotes,
  normalizeBeatState,
  normalizeLoopSettings,
  quantizeBeat,
  stepAtBeat,
  stepToBeat,
  wrapBeat,
} from './arrangementHelpers';

const hit = (note: number, startBeat: number, velocity = 0.8): RegionNote => ({
  note,
  startBeat,
  velocity,
  durationBeats: 0.25,
  isDrum: true,
});

const drumTrack = (notes: RegionNote[], kit = false): Track => ({
  id: 't1',
  name: 'Drums',
  instrument: { type: 'drums', presetIndex: 0, ...(kit ? { kit: createFactoryKit() } : {}) },
  regions: [{ id: 'r1', startBeat: 0, lengthBeats: 4, notes }],
  volume: 0,
  pan: 0,
  muted: false,
  solo: false,
  color: '#fff',
});

describe('quantizeBeat', () => {
  it('snaps to straight grids', () => {
    expect(quantizeBeat(0.13, '1/16', 0)).toBe(0.25);
    expect(quantizeBeat(0.12, '1/16', 0)).toBe(0);
    expect(quantizeBeat(1.7, '1/8', 0)).toBe(1.5);
    expect(quantizeBeat(1.8, '1/4', 0)).toBe(2);
    expect(quantizeBeat(0.07, '1/32', 0)).toBe(0.125);
  });

  it("returns the beat unchanged for 'off'", () => {
    expect(quantizeBeat(1.2345, 'off', 0.5)).toBe(1.2345);
  });

  it('delays odd steps by swing × step', () => {
    // 1/16 with 50% swing: odd 16ths sit at +0.125 beat
    expect(quantizeBeat(0.3, '1/16', 0.5)).toBe(0.375);
    expect(quantizeBeat(0.26, '1/16', 0.5)).toBe(0.375);
    // even steps stay put
    expect(quantizeBeat(0.52, '1/16', 0.5)).toBe(0.5);
    // close to the next even step: goes there, not to the swung odd step
    expect(quantizeBeat(0.47, '1/16', 0.5)).toBe(0.5);
    // swing is clamped to 0.75
    expect(quantizeBeat(0.45, '1/16', 2)).toBe(0.4375);
  });

  it('handles triplet grids (never swung, no float noise)', () => {
    expect(quantizeBeat(0.3, '1/8T', 0)).toBe(0.333333333);
    expect(quantizeBeat(0.7, '1/8T', 0.5)).toBe(0.666666667);
    expect(quantizeBeat(1.02, '1/8T', 0)).toBe(1);
    expect(quantizeBeat(0.18, '1/16T', 0.6)).toBe(0.166666667);
    expect(quantizeBeat(2.9, '1/16T', 0)).toBe(2.833333333);
  });

  it('step <-> beat mapping round-trips with swing', () => {
    for (const swing of [0, 0.25, 0.5, 0.75]) {
      for (let step = 0; step < 32; step++) {
        expect(stepAtBeat(stepToBeat(step, 0.25, swing), 0.25, swing)).toBe(step);
      }
    }
    for (let step = 0; step < 24; step++) {
      expect(stepAtBeat(stepToBeat(step, 1 / 6, 0.5), 1 / 6, 0.5)).toBe(step);
    }
  });

  it('gridStepBeats', () => {
    expect(gridStepBeats('1/16')).toBe(0.25);
    expect(gridStepBeats('1/8T')).toBeCloseTo(1 / 3);
    expect(gridStepBeats('off')).toBeNull();
  });
});

describe('wrapBeat', () => {
  it('wraps into [0, length)', () => {
    expect(wrapBeat(4.25, 4)).toBe(0.25);
    expect(wrapBeat(-0.25, 4)).toBe(3.75);
    expect(wrapBeat(4, 4)).toBe(0);
    expect(wrapBeat(7.99999, 8)).toBe(0);
  });
});

describe('mergeDrumHit', () => {
  it('appends new hits and keeps the louder of duplicates', () => {
    let notes = mergeDrumHit([], hit(36, 0, 0.5));
    notes = mergeDrumHit(notes, hit(38, 0, 0.5));
    expect(notes).toHaveLength(2);
    notes = mergeDrumHit(notes, hit(36, 0.00001, 0.9));
    expect(notes).toHaveLength(2);
    expect(notes[0].velocity).toBe(0.9);
    notes = mergeDrumHit(notes, hit(36, 0, 0.2));
    expect(notes[0].velocity).toBe(0.9);
  });
});

describe('GM drum migration', () => {
  it('maps GM notes to factory pads and drops unknown notes', () => {
    const { notes, dropped } = migrateGmDrumNotes([hit(36, 0), hit(38, 1), hit(42, 0.5), hit(46, 1.5), hit(49, 0), hit(51, 2), hit(39, 3), hit(100, 0)]);
    expect(dropped).toBe(1);
    expect(notes.map((n) => n.note)).toEqual([0, 1, 2, 3, 6, 7, 4].map(padToNote));
  });

  it('merges legacy notes that land on the same pad', () => {
    // 35 and 36 are both the factory kick
    const { notes } = migrateGmDrumNotes([hit(35, 0, 0.4), hit(36, 0, 0.7)]);
    expect(notes).toEqual([hit(padToNote(0), 0, 0.7)]);
  });

  it('migrates kitless drum tracks only', () => {
    const legacy = drumTrack([hit(38, 0)]);
    expect(migrateDrumTrack(legacy)).toBe(0);
    expect(legacy.instrument.kit?.pads).toHaveLength(16);
    expect(legacy.regions[0].notes[0].note).toBe(padToNote(1));

    // Already on the kit model: pad note 38 (= pad 2) must stay as is
    const modern = drumTrack([hit(38, 0)], true);
    migrateDrumTrack(modern);
    expect(modern.regions[0].notes[0].note).toBe(38);
  });

  it('every template drum note maps to the matching factory voice', () => {
    for (const template of ALL_TEMPLATES) {
      for (const raw of template.arrangement.tracks) {
        if (raw.instrument.type !== 'drums') continue;
        const track = structuredClone(raw);
        const gm = track.regions.flatMap((r) => r.notes.map((n) => n.note));
        expect(migrateDrumTrack(track), template.id).toBe(0);
        const migrated = track.regions.flatMap((r) => r.notes.map((n) => n.note));
        expect(migrated.every((n) => n >= 36 && n <= 51), template.id).toBe(true);
        // Templates use canonical GM notes: mapping the pads back for MIDI export gives the originals.
        expect(migrated.map((n) => drumNoteToGm(n, track.instrument.kit)), template.id).toEqual(gm);
      }
    }
  });
});

describe('drumNoteToGm', () => {
  it('uses the factory voice GM note, else the pad note', () => {
    const kit = createFactoryKit();
    expect(drumNoteToGm(padToNote(0), kit)).toBe(36); // kick
    expect(drumNoteToGm(padToNote(2), kit)).toBe(42); // closed hat
    expect(drumNoteToGm(padToNote(15), kit)).toBe(56); // cowbell
    kit.pads[1] = { ...kit.pads[1], sample: { name: 'x', source: { kind: 'file', path: '/x.wav' } } };
    expect(drumNoteToGm(padToNote(1), kit)).toBe(padToNote(1));
    expect(drumNoteToGm(70, kit)).toBe(70);
  });
});

describe('loop / beat state normalization', () => {
  it('fills defaults and rejects invalid values', () => {
    expect(normalizeLoopSettings(undefined)).toEqual(DEFAULT_LOOP_SETTINGS);
    expect(normalizeLoopSettings({ bars: 3, quantize: '1/5', swing: 9, countInBars: 2, overdub: false })).toEqual({
      ...DEFAULT_LOOP_SETTINGS,
      swing: 0.75,
      countInBars: 2,
      overdub: false,
    });
  });

  it('falls back to the first drum track for a stale active id', () => {
    const t = drumTrack([], true);
    expect(normalizeBeatState({ activeDrumTrackId: 'gone', selectedPad: 40 }, [t])).toMatchObject({
      activeDrumTrackId: 't1',
      selectedPad: 0,
    });
    expect(normalizeBeatState(undefined, []).activeDrumTrackId).toBeNull();
  });
});
