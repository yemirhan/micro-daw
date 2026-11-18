import { it, vi } from 'vitest';
import { defaultPadSettings } from '@/audio/sampler/factoryKit';
import type { KitState, KnobTarget, LoopRecordSettings, PadParam, PadSettings } from '@/types/sampler';
import type { Track } from '@/types/arrangement';
import {
  bpmTo01,
  createKnobRouter,
  knobBpm,
  knobPadValue,
  knobSwing,
  knobVolumeDb,
  padParamTo01,
  swingTo01,
  volumeTo01,
  type KnobEngine,
} from '@/services/KnobRouter';
import { KnobPickup, PICKUP_THRESHOLD, initialPickupState, pickupStep, type PickupState } from '@/services/KnobPickup';
import { writeFixture } from './fixture';

/** JSON-safe number: NaN / ±Infinity as strings. */
const enc = (n: number | null): number | string | null =>
  n === null ? null : Number.isNaN(n) ? 'NaN' : n === Infinity ? 'Infinity' : n === -Infinity ? '-Infinity' : n;

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PARAMS: PadParam[] = ['pitch', 'decay', 'filterCutoff', 'gainDb', 'pan', 'startSec'];

function values01(): number[] {
  const v: number[] = [];
  for (let i = 0; i <= 1000; i++) v.push(i / 1000);
  for (let i = 0; i <= 127; i++) v.push(i / 127);
  v.push(-0.5, -1e-9, 1 + 1e-9, 1.5, 2, -0, NaN, Infinity, -Infinity, 0.49999999999999994, 0.5000000000000001);
  return v;
}

const encPad = (p: PadSettings) => [p.gainDb, p.pan, p.pitch, p.decay, p.filterCutoff, p.startSec].map(enc);

it('dumps knob curves, inverse curves, pickup and router sessions', () => {
  const vals = values01();
  const base = defaultPadSettings();

  // --- forward pad curves (pad state only matters for startSec)
  const forward: unknown[] = [];
  for (const param of PARAMS.filter((p) => p !== 'startSec'))
    for (const v of vals) forward.push([param, enc(v), enc(knobPadValue(param, v, base, 1))]);

  const startPads: PadSettings[] = [
    base,
    { ...base, endSec: 1 },
    { ...base, endSec: 0.005 },
    { ...base, endSec: 0.01 },
    { ...base, endSec: 3 },
    { ...base, endSec: 0 },
    { ...base, endSec: 1.2345 },
  ];
  const durations: (number | null)[] = [null, 0, -1, 0.004, 0.01, 0.0123, 0.5, 1, 2, 2.0123, 10, 60.5, NaN];
  const startSec: unknown[] = [];
  for (const pad of startPads)
    for (const d of durations)
      for (let i = 0; i < vals.length; i += 9) startSec.push([pad.endSec ?? null, enc(d), enc(vals[i]), enc(knobPadValue('startSec', vals[i], pad, d))]);

  const scalar: unknown[] = [];
  for (const v of vals) scalar.push([enc(v), enc(knobSwing(v)), enc(knobBpm(v)), enc(knobVolumeDb(v))]);

  // --- inverse curves
  const inverse: unknown[] = [];
  const invVals: Record<Exclude<PadParam, 'startSec'>, number[]> = {
    pitch: [],
    decay: [],
    filterCutoff: [],
    gainDb: [],
    pan: [],
  };
  for (let x = -30; x <= 30; x += 0.25) invVals.pitch.push(x);
  for (let i = -5; i <= 400; i++) invVals.decay.push(i * 0.0125);
  invVals.decay.push(0.005, 4, 0.001, 0.141, -0, NaN, Infinity);
  for (let i = -3; i <= 500; i++) invVals.filterCutoff.push(i * 45.1);
  invVals.filterCutoff.push(20, 20000, 632, 1, 25000, NaN);
  for (let x = -60; x <= 12; x += 0.1) invVals.gainDb.push(x);
  for (let x = -1.5; x <= 1.5; x += 0.01) invVals.pan.push(x);
  for (const [param, xs] of Object.entries(invVals) as [PadParam, number[]][])
    for (const x of xs) inverse.push([param, enc(x), enc(padParamTo01(param, { ...base, [param]: x }, 1))]);

  const inverseStart: unknown[] = [];
  for (const pad of startPads)
    for (const d of durations)
      for (const s of [0, 0.001, 0.1, 0.5, 0.99, 1, 1.5, 3, 100, -0.2, NaN])
        inverseStart.push([pad.endSec ?? null, enc(d), enc(s), enc(padParamTo01('startSec', { ...pad, startSec: s }, d))]);

  const inverseScalar: unknown[] = [];
  for (let x = -0.2; x <= 1; x += 0.005) inverseScalar.push(['swing', enc(x), enc(swingTo01(x))]);
  for (let x = 40; x <= 200; x += 0.5) inverseScalar.push(['bpm', enc(x), enc(bpmTo01(x))]);
  for (let x = -60; x <= 6; x += 0.25) inverseScalar.push(['volume', enc(x), enc(volumeTo01(x))]);
  for (const x of [NaN, Infinity, -Infinity])
    inverseScalar.push(['swing', enc(x), enc(swingTo01(x))], ['bpm', enc(x), enc(bpmTo01(x))], ['volume', enc(x), enc(volumeTo01(x))]);

  // --- pickupStep sequences
  const rnd = mulberry32(1234);
  const pickupSteps: unknown[] = [];
  for (let seq = 0; seq < 36; seq++) {
    let state: PickupState = initialPickupState();
    const threshold = seq % 3 === 0 ? PICKUP_THRESHOLD : seq % 3 === 1 ? 0.1 : 0;
    let current = rnd();
    const steps: unknown[] = [];
    for (let i = 0; i < 40; i++) {
      if (rnd() < 0.15) current = rnd();
      if (rnd() < 0.2 && state.pickedUp) state = { ...state, lastSet: current + (rnd() - 0.5) * 0.06 };
      const knob = Math.round(rnd() * 127) / 127;
      const r = pickupStep(state, knob, current, threshold);
      steps.push([state, knob, current, r.pass, r.state]);
      state = r.state;
    }
    pickupSteps.push({ threshold, steps });
  }

  // --- KnobPickup op sequences
  const pickupOps: unknown[] = [];
  for (let seq = 0; seq < 30; seq++) {
    const p = new KnobPickup();
    const keys = ['a', 'b', 'c'];
    const cur: Record<string, number> = { a: 0.5, b: 0.2, c: 0.9 };
    const ops: unknown[] = [];
    for (let i = 0; i < 60; i++) {
      const k = keys[Math.floor(rnd() * 3)];
      const r = rnd();
      if (r < 0.6) {
        const knob = Math.round(rnd() * 127) / 127;
        const current = cur[k];
        const pass = p.check(k, knob, current);
        if (pass) cur[k] = knob;
        ops.push(['check', k, knob, current, pass, p.isPickedUp(k)]);
      } else if (r < 0.75) {
        const v = cur[k];
        p.applied(k, v);
        ops.push(['applied', k, v, p.isPickedUp(k)]);
      } else if (r < 0.88) {
        cur[k] = rnd();
        ops.push(['external', k, cur[k]]);
      } else if (r < 0.94) {
        p.reset(k);
        ops.push(['reset', k, p.isPickedUp(k)]);
      } else {
        p.reset();
        ops.push(['resetAll', null, p.isPickedUp(k)]);
      }
    }
    pickupOps.push(ops);
  }

  // --- router sessions (fake engine + fake timers)
  vi.useFakeTimers();
  const sessions: unknown[] = [];
  const targets: KnobTarget[] = [
    ...PARAMS.map((param) => ({ kind: 'pad', param }) as KnobTarget),
    { kind: 'swing' },
    { kind: 'bpm' },
    { kind: 'trackVolume' },
    { kind: 'masterVolume' },
  ];
  for (let s = 0; s < 40; s++) {
    const trackStart: string | null = s % 5 === 4 ? null : 't1';
    const durationMode = s % 4; // 0: 1s, 1: null, 2: 2.5s, 3: per pad
    const idleMs = s % 3 === 0 ? undefined : s % 3 === 1 ? 300 : 120;
    const log: unknown[] = [];
    const kit: KitState = { id: 'k', name: 'K', pads: Array.from({ length: 16 }, () => defaultPadSettings()) };
    const state = { bpm: 120, swing: 0, selectedPad: 3, trackVolume: -6, trackId: trackStart as string | null, master: -12 };
    const engine = {
      getActiveDrumTrackId: () => state.trackId,
      ensureDrumTrack: () => {
        log.push(['ensure']);
        return 'new';
      },
      getSelectedPad: () => state.selectedPad,
      getKit: () => kit,
      updatePad: (t: string, pad: number, patch: Partial<PadSettings>) => {
        const [param, value] = Object.entries(patch)[0];
        log.push(['updatePad', t, pad, param, enc(value as number)]);
        kit.pads[pad] = { ...kit.pads[pad], ...patch };
      },
      beginUndoGroup: (label: string) => log.push(['begin', label]),
      endUndoGroup: () => log.push(['end']),
      setLoopSettings: (patch: Partial<LoopRecordSettings>) => {
        log.push(['setSwing', patch.swing]);
        if (patch.swing !== undefined) state.swing = patch.swing;
      },
      getLoopSettings: () => ({ swing: state.swing }) as LoopRecordSettings,
      setBpm: (bpm: number) => {
        log.push(['setBpm', bpm]);
        state.bpm = bpm;
      },
      getBpm: () => state.bpm,
      setTrackVolume: (id: string, db: number) => {
        log.push(['setTrackVolume', id, db]);
        state.trackVolume = db;
      },
      getTracks: () => (state.trackId ? [{ id: state.trackId, volume: state.trackVolume } as Track] : []),
    } satisfies KnobEngine;
    const router = createKnobRouter({
      engine,
      setMasterVolume: (db) => {
        log.push(['setMaster', db]);
        state.master = db;
      },
      getMasterVolume: () => state.master,
      padBufferDuration: (pad) =>
        durationMode === 0 ? 1 : durationMode === 1 ? null : durationMode === 2 ? 2.5 : 0.5 + (pad.pitch + 24) / 10,
      idleMs,
    });
    const ops: unknown[] = [];
    const knobPos = new Map<string, number>();
    for (let i = 0; i < 150; i++) {
      const r = rnd();
      let op: unknown[];
      if (r < 0.7) {
        const t = targets[Math.floor(rnd() * targets.length)];
        const key = JSON.stringify(t);
        // Knob-like motion: mostly small steps from the last position, sometimes a jump.
        let pos = knobPos.get(key) ?? Math.floor(rnd() * 128);
        pos = rnd() < 0.15 ? Math.floor(rnd() * 128) : Math.max(0, Math.min(127, pos + Math.floor(rnd() * 9) - 4));
        knobPos.set(key, pos);
        // Hold a burst on one target for a few messages.
        const burst = 1 + Math.floor(rnd() * 6);
        const positions: number[] = [];
        for (let b = 0; b < burst; b++) {
          const p = Math.max(0, Math.min(127, pos + b * (rnd() < 0.5 ? 1 : -1)));
          positions.push(p);
          router.handle(t, p / 127);
        }
        op = ['handle', t, positions];
      } else if (r < 0.82) {
        const ms = Math.floor(rnd() * 500);
        op = ['advance', ms];
        vi.advanceTimersByTime(ms);
      } else if (r < 0.87) {
        state.selectedPad = Math.floor(rnd() * 16);
        op = ['selectPad', state.selectedPad];
      } else if (r < 0.91) {
        const pad = Math.floor(rnd() * 16);
        const param = (['pitch', 'gainDb', 'decay', 'pan'] as const)[Math.floor(rnd() * 4)];
        const v = param === 'pitch' ? Math.round(rnd() * 48 - 24) : param === 'gainDb' ? Math.round(rnd() * 46 - 40) : param === 'decay' ? 0.005 + rnd() * 2 : rnd() * 2 - 1;
        kit.pads[pad] = { ...kit.pads[pad], [param]: v };
        op = ['externalPad', pad, param, v];
      } else if (r < 0.93) {
        const which = Math.floor(rnd() * 3);
        if (which === 0) state.bpm = 60 + Math.floor(rnd() * 120);
        else if (which === 1) state.swing = Math.round(rnd() * 75) / 100;
        else state.master = -Math.round(rnd() * 80) / 2;
        op = ['externalScalar', which, which === 0 ? state.bpm : which === 1 ? state.swing : state.master];
      } else if (r < 0.95) {
        state.trackId = rnd() < 0.3 ? null : rnd() < 0.5 ? 't1' : 't2';
        op = ['setTrack', state.trackId];
      } else if (r < 0.97) {
        op = ['flush'];
        router.flush();
      } else if (r < 0.99) {
        op = ['resetPickup'];
        router.resetPickup();
      } else {
        op = ['dispose'];
        router.dispose();
      }
      ops.push([op, log.splice(0)]);
    }
    vi.advanceTimersByTime(10_000);
    ops.push([['advance', 10_000], log.splice(0)]);
    sessions.push({ trackStart, durationMode, idleMs: idleMs ?? null, ops, finalPads: kit.pads.map(encPad) });
  }
  vi.useRealTimers();

  // --- raw Math.log / Math.pow (the curves' transcendental building blocks)
  const mathLog: unknown[] = [];
  const lrnd = mulberry32(42);
  for (let i = 0; i < 3000; i++) {
    const e = Math.floor(lrnd() * 80) - 40;
    const x = lrnd() * Math.pow(2, e);
    mathLog.push([x, Math.log(x)]);
  }
  for (let i = 1; i <= 2000; i++) mathLog.push([i * 0.0125 / 0.005, Math.log((i * 0.0125) / 0.005)], [(i * 45.1) / 20, Math.log((i * 45.1) / 20)]);
  for (const x of [1, 1 + 2 ** -52, 1 - 2 ** -53, 1 + 2 ** -21, 1 - 2 ** -21, 2, 0.5, 800, 1000, 185, 5e-324, 2.2e-308, 1e-310, 1.7976931348623157e308])
    mathLog.push([x, Math.log(x)]);
  const mathPow: unknown[] = [];
  for (let i = 0; i <= 2000; i++) mathPow.push([800, i / 2000, Math.pow(800, i / 2000)], [1000, i / 2000, Math.pow(1000, i / 2000)]);
  for (let i = 0; i < 2000; i++) {
    const x = lrnd() * 2000;
    const y = lrnd() * 4 - 2;
    mathPow.push([x, y, Math.pow(x, y)]);
  }

  writeFixture('knobs', { mathLog, mathPow, forward, startSec, scalar, inverse, inverseStart, inverseScalar, pickupSteps, pickupOps, sessions });
});
