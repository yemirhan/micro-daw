import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { defaultPadSettings } from '@/audio/sampler/factoryKit';
import type { KitState, LoopRecordSettings, PadSettings } from '@/types/sampler';
import type { Track } from '@/types/arrangement';
import {
  bpmTo01,
  createKnobRouter,
  padParamTo01,
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
  const state = { bpm: 120, swing: 0, selectedPad: 3, trackVolume: -6 };
  const engine = {
    getActiveDrumTrackId: vi.fn(() => trackId),
    ensureDrumTrack: vi.fn(() => 'new'),
    getSelectedPad: vi.fn(() => state.selectedPad),
    getKit: vi.fn(() => kit),
    updatePad: vi.fn((_t: string, pad: number, patch: Partial<PadSettings>) => {
      kit.pads[pad] = { ...kit.pads[pad], ...patch };
    }),
    beginUndoGroup: vi.fn(),
    endUndoGroup: vi.fn(),
    setLoopSettings: vi.fn((patch: Partial<LoopRecordSettings>) => {
      if (patch.swing !== undefined) state.swing = patch.swing;
    }),
    getLoopSettings: vi.fn(() => ({ swing: state.swing }) as LoopRecordSettings),
    setBpm: vi.fn((bpm: number) => {
      state.bpm = bpm;
    }),
    getBpm: vi.fn(() => state.bpm),
    setTrackVolume: vi.fn((_id: string, db: number) => {
      state.trackVolume = db;
    }),
    getTracks: vi.fn(() => (trackId ? [{ id: trackId, volume: state.trackVolume } as Track] : [])),
  } satisfies KnobEngine;
  return { engine, kit, state };
}

/** Router with a master volume held in a variable. */
function makeRouter(engine: KnobEngine, opts: { idleMs?: number; padBufferDuration?: () => number | null } = {}) {
  let master = -12;
  const setMasterVolume = vi.fn((db: number) => {
    master = db;
  });
  const router = createKnobRouter({
    engine,
    setMasterVolume,
    getMasterVolume: () => master,
    padBufferDuration: opts.padBufferDuration ?? (() => 1),
    idleMs: opts.idleMs,
  });
  return { router, setMasterVolume, master: () => master };
}

describe('createKnobRouter', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('edits the selected pad of the active drum track; one twist = one undo group', () => {
    const { engine } = fakeEngine();
    const { router } = makeRouter(engine, { idleMs: 300 });
    router.handle({ kind: 'pad', param: 'pitch' }, 0.5); // picks up at the current value (pitch 0)
    router.handle({ kind: 'pad', param: 'pitch' }, 0.6);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.7);
    vi.advanceTimersByTime(200);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.8);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(1);
    expect(engine.endUndoGroup).not.toHaveBeenCalled();
    expect(engine.updatePad).toHaveBeenLastCalledWith('t1', 3, { pitch: 14 });

    vi.advanceTimersByTime(300);
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(1);

    router.handle({ kind: 'pad', param: 'pitch' }, 0.75);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(2);
  });

  it('soft takeover: a knob far from the value does nothing until it crosses it', () => {
    const { engine, kit } = fakeEngine();
    const { router } = makeRouter(engine);
    // pitch is 0 (knob position 0.5); the physical knob sits at 0.9
    router.handle({ kind: 'pad', param: 'pitch' }, 0.9);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.8);
    expect(engine.updatePad).not.toHaveBeenCalled();
    router.handle({ kind: 'pad', param: 'pitch' }, 0.45); // crossed 0.5
    expect(engine.updatePad).toHaveBeenLastCalledWith('t1', 3, { pitch: -2 });
    router.handle({ kind: 'pad', param: 'pitch' }, 0.3);
    expect(kit.pads[3].pitch).toBe(Math.round(-24 + 0.3 * 48));
  });

  it('pickup resets when the selected pad or the active track changes', () => {
    const { engine, kit, state } = fakeEngine();
    const { router } = makeRouter(engine);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.5);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.75); // pad 3 → +12
    expect(kit.pads[3].pitch).toBe(12);
    state.selectedPad = 4; // pad 4 is at 0: the knob (0.75) must come back to 0.5 first
    router.handle({ kind: 'pad', param: 'pitch' }, 0.76);
    expect(kit.pads[4].pitch).toBe(0);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.4); // crossed 0.5
    expect(kit.pads[4].pitch).toBe(-5);
  });

  it('a value changed elsewhere (UI, undo) has to be picked up again', () => {
    const { engine, kit } = fakeEngine();
    const { router } = makeRouter(engine);
    router.handle({ kind: 'pad', param: 'gainDb' }, padParamTo01('gainDb', kit.pads[3], 1)!);
    router.handle({ kind: 'pad', param: 'gainDb' }, 0.9);
    const set = kit.pads[3].gainDb;
    kit.pads[3] = { ...kit.pads[3], gainDb: -30 }; // on-screen edit
    router.handle({ kind: 'pad', param: 'gainDb' }, 0.95);
    expect(kit.pads[3].gainDb).toBe(-30);
    expect(set).not.toBe(-30);
  });

  it('closes the open step when another knob moves', () => {
    const { engine } = fakeEngine();
    const { router } = makeRouter(engine);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.5);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.6);
    router.handle({ kind: 'pad', param: 'decay' }, padParamTo01('decay', defaultPadSettings(), 1)!);
    router.handle({ kind: 'pad', param: 'decay' }, 0.2);
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(1);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(2);
    router.dispose();
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(2);
  });

  it('skips updates that do not change the value (no empty undo steps)', () => {
    const { engine } = fakeEngine();
    const { router } = makeRouter(engine);
    router.handle({ kind: 'pad', param: 'pitch' }, 0.5); // pitch 0 = default
    expect(engine.updatePad).not.toHaveBeenCalled();
    expect(engine.beginUndoGroup).not.toHaveBeenCalled();
  });

  it('creates a drum track for pad knobs when there is none', () => {
    const { engine } = fakeEngine(null);
    const { router } = makeRouter(engine);
    router.handle({ kind: 'pad', param: 'gainDb' }, padParamTo01('gainDb', defaultPadSettings(), 1)!);
    router.handle({ kind: 'pad', param: 'gainDb' }, 1);
    expect(engine.ensureDrumTrack).toHaveBeenCalled();
    expect(engine.updatePad).toHaveBeenCalledWith('new', 3, { gainDb: 6 });
  });

  it('routes swing, bpm and volumes once picked up', () => {
    const { engine } = fakeEngine();
    const { router, setMasterVolume } = makeRouter(engine);
    router.handle({ kind: 'swing' }, 0);
    router.handle({ kind: 'swing' }, 1);
    router.handle({ kind: 'bpm' }, 0.5);
    router.handle({ kind: 'bpm' }, 0.6);
    router.handle({ kind: 'trackVolume' }, 0.85); // -6 dB
    router.handle({ kind: 'trackVolume' }, 0);
    router.handle({ kind: 'masterVolume' }, 0.7); // -12 dB
    router.handle({ kind: 'masterVolume' }, 1);
    expect(engine.setLoopSettings).toHaveBeenCalledWith({ swing: 0.75 });
    expect(engine.setBpm).toHaveBeenCalledWith(132);
    expect(engine.setTrackVolume).toHaveBeenCalledWith('t1', -40);
    expect(setMasterVolume).toHaveBeenCalledWith(0);
  });

  it('bpm is whole numbers; a tempo / volume twist is one undo step, not one per CC', () => {
    const { engine } = fakeEngine();
    const { router } = makeRouter(engine, { idleMs: 300 });
    router.handle({ kind: 'bpm' }, bpmTo01(120));
    for (let v = 0.5; v <= 0.6; v += 0.003) router.handle({ kind: 'bpm' }, v);
    for (const [bpm] of engine.setBpm.mock.calls) expect(Number.isInteger(bpm)).toBe(true);
    expect(engine.setBpm.mock.calls.length).toBeGreaterThan(5);
    // Same value again: no call.
    const n = engine.setBpm.mock.calls.length;
    router.handle({ kind: 'bpm' }, 0.6);
    expect(engine.setBpm.mock.calls.length).toBe(n);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(300);
    expect(engine.endUndoGroup).toHaveBeenCalledTimes(1);

    router.handle({ kind: 'trackVolume' }, 0.85);
    router.handle({ kind: 'trackVolume' }, 0.8);
    router.handle({ kind: 'trackVolume' }, 0.7);
    expect(engine.beginUndoGroup).toHaveBeenCalledTimes(2);
  });

  it('master volume is not an undo step', () => {
    const { engine } = fakeEngine();
    const { router, master } = makeRouter(engine);
    router.handle({ kind: 'masterVolume' }, 0.7);
    router.handle({ kind: 'masterVolume' }, 0.5);
    expect(master()).toBe(-20);
    expect(engine.beginUndoGroup).not.toHaveBeenCalled();
  });

  it('ignores the track volume knob without a drum track', () => {
    const { engine } = fakeEngine(null);
    const { router } = makeRouter(engine, { padBufferDuration: () => null });
    router.handle({ kind: 'trackVolume' }, 0.5);
    expect(engine.setTrackVolume).not.toHaveBeenCalled();
    expect(engine.ensureDrumTrack).not.toHaveBeenCalled();
  });
});
