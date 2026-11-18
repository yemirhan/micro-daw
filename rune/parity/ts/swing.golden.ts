import { it } from 'vitest';
import { quantizeBeat, swingOffsetBeats, loopGateBeats, wrapBeat, patternStepAt } from '@/utils/arrangementHelpers';
import type { QuantizeGrid, SwingGrid } from '@/types/sampler';
import { writeFixture } from './fixture';

const GRIDS: QuantizeGrid[] = ['off', '1/4', '1/8', '1/8T', '1/16', '1/16T', '1/32'];
const SWING_GRIDS: SwingGrid[] = ['1/8', '1/16'];
const SWINGS = [0, 0.1, 0.25, 0.5, 0.66, 0.75, 0.9, -0.2];

it('dumps quantize / swing / gate / wrap / step vectors', () => {
  const beats: number[] = [];
  for (let i = 0; i <= 200; i++) beats.push(i * 0.0371 - 0.3);
  beats.push(0, 0.25, 0.5, 0.75, 1, 1.125, 1.25, 1 / 3, 2 / 3, 0.2499, 0.2501, 7.99, 8);
  const quantize = [];
  for (const g of GRIDS) for (const sg of SWING_GRIDS) for (const s of SWINGS) for (const b of beats)
    quantize.push([b, g, s, sg, quantizeBeat(b, g, s, sg)]);
  const swing = [];
  for (const g of [...SWING_GRIDS, ...GRIDS]) for (const s of SWINGS) for (const b of beats)
    swing.push([b, g, s, swingOffsetBeats(b, g, s)]);
  const gate = [];
  for (const on of [0, 0.5, 3.75, 7.9]) for (const off of [0, 0.1, 0.5, 4, 7.95, 8]) for (const w of [0, 4, 8, -1, NaN]) for (const l of [0, 4, 8])
    gate.push([on, off, Number.isNaN(w) ? 'NaN' : w, l, loopGateBeats(on, off, w, l)]);
  const wrap = [];
  for (const b of [...beats, -8, -7.99999, 15.99999, 16, 33]) for (const l of [0, 4, 8, 1 / 3])
    wrap.push([b, l, wrapBeat(b, l)]);
  const steps = [];
  for (const b of beats) for (const sb of [0.125, 0.25, 0.5]) for (const n of [8, 16, 32])
    steps.push([b, sb, n, patternStepAt(b, sb, n)]);
  writeFixture('swing', { quantize, swing, gate, wrap, steps });
});
