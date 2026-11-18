/** Automation helper vectors: interpolation, value mapping, point insert/update/delete, labels. */
import { it } from 'vitest';
import {
  deleteAutomationPoint,
  getAutomationParameterLabel,
  getAutomationValue,
  mapAutomationValue,
  setAutomationPoint,
} from '@/utils/automationHelpers';
import type { AutomationParameter, AutomationPoint } from '@/types/arrangement';
import { writeFixture } from './fixture';

const PARAMS: AutomationParameter[] = [
  'volume', 'pan', 'reverbWet', 'delayWet', 'chorusDepth', 'distortionWet', 'filterCutoff', 'eqLow', 'eqMid', 'eqHigh',
];

const P = (beat: number, value: number): AutomationPoint => ({ beat, value });

const LANES: AutomationPoint[][] = [
  [],
  [P(2, 0.3)],
  [P(0, 0), P(4, 1)],
  [P(0, 0.2), P(1, 0.9), P(1, 0.1), P(3.5, 0.6), P(8, 0)],
  [P(-2, 1), P(0.333, 0.25), P(16, 0.75)],
  [P(4, 0.5), P(2, 1)], // unsorted (as a corrupt file could have)
];

it('dumps automation vectors', () => {
  const values: unknown[] = [];
  for (const [i, lane] of LANES.entries()) {
    for (let b = -3; b <= 18; b += 0.125) values.push([i, b, getAutomationValue(lane, b)]);
  }
  const mapped: unknown[] = [];
  for (const p of PARAMS) for (const v of [-0.5, 0, 0.1, 0.25, 0.5, 0.77, 1, 1.5]) mapped.push([p, v, mapAutomationValue(p, v)]);
  const labels = PARAMS.map((p) => [p, getAutomationParameterLabel(p)]);

  const edits: unknown[] = [];
  const ops: [number, number, number][] = [
    [1.9, 0.5, 1], [2.0004, 2, 0], [0, -1, 0.25], [3.3, 0.7, 0.25], [3.25, 0.1, 0.25], [7.99, 0.4, 0.5],
    [1.25, 0.6, 0.5], [-0.75, 0.3, 0.5], [10.1, 0.9, 0], [10.1005, 0.2, 0], [5, 1.2, -1], [2.5, 0.5, 1 / 3],
  ];
  for (const [i, lane] of LANES.entries()) {
    let pts = lane;
    for (const [beat, value, snap] of ops) {
      pts = setAutomationPoint(pts, beat, value, snap);
      edits.push([i, beat, value, snap, pts]);
    }
    for (const idx of [0, 2, 99]) edits.push([i, 'delete', idx, null, (pts = deleteAutomationPoint(pts, idx))]);
  }
  writeFixture('automation', { lanes: LANES, values, mapped, labels, edits });
});
