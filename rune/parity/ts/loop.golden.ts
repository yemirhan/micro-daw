/** normalizeLoopSettings / normalizeBeatState over malformed and edge inputs. */
import { it } from 'vitest';
import { DEFAULT_LOOP_SETTINGS, normalizeBeatState, normalizeLoopSettings } from '@/utils/arrangementHelpers';
import type { Track } from '@/types/arrangement';
import type { LoopRecordSettings } from '@/types/sampler';
import { writeFixture } from './fixture';

const LOOPS: unknown[] = [
  null,
  7,
  'loop',
  [],
  {},
  { bars: 1, quantize: 'off', swing: 0.2, swingGrid: '1/8', countInBars: 0, metronome: false, overdub: false },
  { bars: 8, quantize: '1/32', countInBars: 2 },
  { bars: 3, quantize: '1/5', swing: 9, countInBars: 2, overdub: false },
  { bars: '4', countInBars: '1', metronome: 'yes', overdub: 0 },
  { bars: 4.0, countInBars: 1.5 },
  { quantize: '1/8' },
  { quantize: '1/8', swing: 0.3 },
  { quantize: '1/4', swing: 0.3 },
  { quantize: '1/8', swingGrid: '1/16' },
  { quantize: '1/8T' },
  { quantize: 7 },
  { quantize: 'bogus' },
  { quantize: 'bogus', swingGrid: null },
  { swingGrid: '1/32' },
  { swingGrid: '1/8' },
  { swingGrid: null, quantize: '1/8' },
  { swing: -0.3 },
  { swing: 0.75 },
  { swing: 0.7500001 },
  { swing: '0.5' },
  { swing: 1e300 },
  { quantize: '1/16T', swing: 0.66, swingGrid: '1/16', bars: 2, countInBars: 0, metronome: true, overdub: true },
];

const BASES: LoopRecordSettings[] = [
  DEFAULT_LOOP_SETTINGS,
  { bars: 8, quantize: '1/8T', swing: 0.4, swingGrid: '1/8', countInBars: 0, metronome: false, overdub: false },
];

const track = (id: string, type: 'drums' | 'synth' | 'audio'): Track =>
  ({ id, name: id, instrument: { type, presetIndex: 0 }, regions: [], volume: 0, pan: 0, muted: false, solo: false, color: '#fff' }) as Track;

const TRACK_SETS: Track[][] = [
  [],
  [track('s1', 'synth')],
  [track('s1', 'synth'), track('d1', 'drums'), track('d2', 'drums'), track('a1', 'audio')],
];

const BEATS: unknown[] = [
  undefined,
  null,
  5,
  [],
  {},
  { activeDrumTrackId: 'd2', selectedPad: 3 },
  { activeDrumTrackId: 'gone', selectedPad: 40 },
  { activeDrumTrackId: 's1', selectedPad: -1 },
  { activeDrumTrackId: 7, selectedPad: 15.4 },
  { activeDrumTrackId: null, selectedPad: 15.5 },
  { selectedPad: 14.5 },
  { selectedPad: -0.4 },
  { selectedPad: '3' },
  { loop: { bars: 4, quantize: '1/8' } },
  { activeDrumTrackId: 'd1', loop: 'nope', selectedPad: 0.5 },
];

it('dumps loop / beat state vectors', () => {
  const loops: unknown[] = [];
  for (const [b, base] of BASES.entries()) for (const raw of LOOPS) loops.push([raw, b, normalizeLoopSettings(raw, base)]);
  const beats: unknown[] = [];
  for (const [t, tracks] of TRACK_SETS.entries()) {
    for (const raw of BEATS) beats.push([raw === undefined ? '__undefined__' : raw, t, normalizeBeatState(raw, tracks)]);
  }
  writeFixture('loop', { bases: BASES, trackSets: TRACK_SETS, loops, beats });
});
