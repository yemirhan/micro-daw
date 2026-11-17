import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultPadSettings } from '@/audio/sampler/factoryKit';
import type { KitState, PadSettings } from '@/types/sampler';
import {
  createKnobRouter,
  knobBpm,
  knobPadValue,
  knobSwing,
  knobVolumeDb,
  type KnobEngine,
} from './KnobRouter';

describe('knob value ranges', () => {
  const pad = defaultPadSettings();

  it('maps pad params to the plan ranges', () => {
    expect(knobPadValue('pitch', 0, pad, null)).toBe(-24);
    expect(knobPadValue('pitch', 0.5, pad, null)).toBe(0);
    expect(knobPadValue('pitch', 1, pad, null)).toBe(24);
    expect(knobPadValue('gainDb', 0, pad, null)).toBe(-40);
    expect(knobPadValue('gainDb', 1, pad, null)).toBe(6);
    expect(knobPadValue('pan', 0.5, pad, null)).toBe(0);
  });

  it('maps decay and cutoff logarithmically', () => {
    expect(knobPadValue('decay', 0, pad, null)).toBeCloseTo(0.005, 3);
    expect(knobPadValue('decay', 1, pad, null)).toBeCloseTo(4, 3);
    // Geometric midpoint of 0.005..4 ≈ 0.141
    expect(knobPadValue('decay', 0.5, pad, null)).toBeCloseTo(Math.sqrt(0.005 * 4), 2);
    expect(knobPadValue('filterCutoff', 0, pad, null)).toBe(20);
    expect(knobPadValue('filterCutoff', 1, pad, null)).toBe(20000);
    expect(knobPadValue('filterCutoff', 0.5, pad, null)).toBe(632);
  });

  it('maps the trim start within the slice and needs the buffer length', () => {
    expect(knobPadValue('startSec', 0.5, pad, null)).toBeNull();
    expect(knobPadValue('startSec', 0, pad, 2)).toBe(0);
    expect(knobPadValue('startSec', 1, pad, 2)).toBeCloseTo(1.99, 3);
    expect(knobPadValue('startSec', 1, { ...pad, endSec: 1 }, 2)).toBeCloseTo(0.99, 3);
  });

  it('maps swing, tempo and volume', () => {
    expect(knobSwing(0)).toBe(0);
    expect(knobSwing(1)).toBe(0.75);
    expect(knobBpm(0)).toBe(60);
    expect(knobBpm(0.5)).toBe(120);
    expect(knobBpm(1)).toBe(180);
    expect(knobVolumeDb(0)).toBe(-40);
    expect(knobVolumeDb(1)).toBe(0);
  });

  it('clamps out-of-range input', () => {
    expect(knobBpm(2)).toBe(180);
    expect(knobBpm(-1)).toBe(60);
    expect(knobBpm(Number.NaN)).toBe(60);
  });
});

function fakeEngine(trackId: string | null = 't1') {
  const kit: KitState = { id: 'k', name: 'K', pads: Array.from({ length: 16 }, () => defaultPadSettings()) };
  const engine = {
    getActiveDrumTrackId: vi.fn(() => trackId),
    ensureDrumTrack: vi.fn(() => 'new'),
    getSelectedPad: vi.fn(() => 3),
    getKit: vi.fn(() => kit),
    updatePad: vi.fn((_t: string, pad: number, patch: Partial<PadSettings>) => {
      kit.pads[pad] = { ...kit.pads[pad], ...patch };
    }),
    beginUndoGroup: vi.fn(),
    endUndoGroup: vi.fn(),
    setLoopSettings: vi.fn(),
    setBpm: vi.fn(),
    setTrackVolume: vi.fn(),
  } satisfies KnobEngine;
  return { engine, kit };
}

describe('createKnobRouter', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('edits the selected pad of the active drum track; one twist = one undo group', () => {
    const { engine } = fakeEngine();
    const router = createKnobRouter({ engine, setMasterVolume: vi.fn(), padBufferDuration: () => 1, idleMs: 300 });
    router.handle({ kind: 'pad', param: 'pitch' }, 0.6);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.7);
    vi.advanceTimersByTime(200);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.8);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(1);
    expect(engine.endUndoGroup).not.toHaveBeenCalled();
    expect(engine.updatePad).toHaveBeenLastCalledWith('t1', 3, { pitch: 14 });

    vi.advanceTimersByTime(300);
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(1);

    router.handle({ kind: 'pad', param: 'pitch' }, 0.2);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(2);
  });

  it('closes the open step when another knob moves', () => {
    const { engine } = fakeEngine();
    const router = createKnobRouter({ engine, setMasterVolume: vi.fn(), padBufferDuration: () => 1 });
    router.handle({ kind: 'pad', param: 'pitch' }, 0.6);
    router.handle({ kind: 'pad', param: 'decay' }, 0.6);
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(1);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(2);
    router.dispose();
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(2);
  });

  it('skips updates that do not change the value (no empty undo steps)', () => {
    const { engine } = fakeEngine();
    const router = createKnobRouter({ engine, setMasterVolume: vi.fn(), padBufferDuration: () => 1 });
    router.handle({ kind: 'pad', param: 'pitch' }, 0.5); // pitch 0 = default
    expect(engine.updatePad).not.toHaveBeenCalled();
    expect(engine.beginUndoGroup).not.toHaveBeenCalled();
  });

  it('creates a drum track for pad knobs when there is none', () => {
    const { engine } = fakeEngine(null);
    const router = createKnobRouter({ engine, setMasterVolume: vi.fn(), padBufferDuration: () => 1 });
    router.handle({ kind: 'pad', param: 'gainDb' }, 1);
    expect(engine.ensureDrumTrack).toHaveBeenCalled();
    expect(engine.updatePad).toHaveBeenCalledWith('new', 3, { gainDb: 6 });
  });

  it('routes swing, bpm and volumes', () => {
    const { engine } = fakeEngine();
    const setMasterVolume = vi.fn();
    const router = createKnobRouter({ engine, setMasterVolume, padBufferDuration: () => null });
    router.handle({ kind: 'swing' }, 1);
    router.handle({ kind: 'bpm' }, 0.5);
    router.handle({ kind: 'trackVolume' }, 0);
    router.handle({ kind: 'masterVolume' }, 1);
    expect(engine.setLoopSettings).toHaveBeenCalledWith({ swing: 0.75 });
    expect(engine.setBpm).toHaveBeenCalledWith(120);
    expect(engine.setTrackVolume).toHaveBeenCalledWith('t1', -40);
    expect(setMasterVolume).toHaveBeenCalledWith(0);
    expect(engine.beginUndoGroup).not.toHaveBeenCalled();
  });

  it('ignores the track volume knob without a drum track', () => {
    const { engine } = fakeEngine(null);
    const router = createKnobRouter({ engine, setMasterVolume: vi.fn(), padBufferDuration: () => null });
    router.handle({ kind: 'trackVolume' }, 0.5);
    expect(engine.setTrackVolume).not.toHaveBeenCalled();
    expect(engine.ensureDrumTrack).not.toHaveBeenCalled();
  });
});
