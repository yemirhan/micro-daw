import { afterEach, describe, expect, it, vi } from 'vitest';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { createFactoryKit } from '@/audio/sampler/factoryKit';
import { fixMissingPads, planMissingPadFix } from './missingPackActions';

afterEach(() => vi.restoreAllMocks());

describe('missing pack fixes (review L-c)', () => {
  it('clear empties the pads', () => {
    expect(planMissingPadFix([0, 5], 'clear')).toEqual([
      { pad: 0, sample: null, patch: null },
      { pad: 5, sample: null, patch: null },
    ]);
  });

  it('builtin puts the factory sound of the same pad back and resets the trim', () => {
    const factory = createFactoryKit();
    const plan = planMissingPadFix([0, 15], 'builtin');
    expect(plan[0].sample).toEqual(factory.pads[0].sample);
    expect(plan[1].sample).toEqual(factory.pads[15].sample);
    expect(plan[0].patch).toEqual({ startSec: 0, endSec: null, reverse: false });
  });

  it('applies the fix as one undo step', () => {
    const calls: string[] = [];
    vi.spyOn(arrangementEngine, 'beginUndoGroup').mockImplementation((l) => void calls.push(`begin:${l}`));
    vi.spyOn(arrangementEngine, 'endUndoGroup').mockImplementation(() => void calls.push('end'));
    vi.spyOn(arrangementEngine, 'setPadSample').mockImplementation((_t, pad, s) => void calls.push(`set:${pad}:${s ? 'sample' : 'null'}`));
    vi.spyOn(arrangementEngine, 'updatePad').mockImplementation((_t, pad) => void calls.push(`update:${pad}`));
    fixMissingPads('t1', [1, 2], 'builtin');
    expect(calls).toEqual(['begin:Replace pads', 'set:1:sample', 'update:1', 'set:2:sample', 'update:2', 'end']);
    calls.length = 0;
    fixMissingPads('t1', [3], 'clear');
    expect(calls).toEqual(['begin:Clear pads', 'set:3:null', 'end']);
  });
});
