import { arrangementEngine } from '@/services/ArrangementEngine';
import { controllerMapping } from '@/services/ControllerMapping';
import { getCachedPadBuffer } from '@/audio/sampler/bufferCache';
import { getMasterVolumeDb, setMasterVolumeDb } from '@/audio/masterBus';
import { KnobPickup } from '@/services/KnobPickup';
import { MAX_VOLUME, MIN_VOLUME } from '@/utils/constants';
import type { KnobTarget, PadIndex, PadParam, PadSettings } from '@/types/sampler';

/**
 * Hardware knobs (controllerMapping.onKnob) → app parameters.
 *
 * Pad parameters act on the selected pad of the active drum track. A knob
 * twist is a burst of CC messages; each burst becomes one undo step (an undo
 * group closed after the knob has been idle for a moment, or when another
 * knob/pad starts moving). Swing, tempo and track volume twists are grouped
 * the same way (one step per gesture, never one per CC message); master
 * volume is not arrangement state and is not undoable.
 *
 * Soft takeover: a knob only takes control of a parameter once its position
 * crosses (or comes near) the parameter's current value (see KnobPickup).
 * Changing the selected pad or the active track resets every pickup.
 */

/** Value ranges for knob targets (value01 = 0..1 from the controller). */
export const KNOB_RANGES = {
  pitch: { min: -24, max: 24 },
  decay: { min: 0.005, max: 4 },
  filterCutoff: { min: 20, max: 20000 },
  gainDb: { min: -40, max: 6 },
  pan: { min: -1, max: 1 },
  swing: { min: 0, max: 0.75 },
  bpm: { min: 60, max: 180 },
  volume: { min: MIN_VOLUME, max: MAX_VOLUME },
} as const;

const clamp01 = (v: number) => (Number.isFinite(v) ? Math.min(1, Math.max(0, v)) : 0);
const lin = (v: number, min: number, max: number) => min + clamp01(v) * (max - min);
const log = (v: number, min: number, max: number) => min * Math.pow(max / min, clamp01(v));
const round = (v: number, step: number) => Math.round(v / step) * step;

/**
 * Pad parameter value for a knob position. `bufferDuration` (seconds) is needed
 * for `startSec` (0 .. the trim end); without it the knob does nothing (null).
 */
export function knobPadValue(param: PadParam, value01: number, pad: PadSettings, bufferDuration: number | null): number | null {
  switch (param) {
    case 'pitch':
      return Math.round(lin(value01, KNOB_RANGES.pitch.min, KNOB_RANGES.pitch.max));
    case 'decay':
      return round(log(value01, KNOB_RANGES.decay.min, KNOB_RANGES.decay.max), 0.001);
    case 'filterCutoff':
      return Math.round(log(value01, KNOB_RANGES.filterCutoff.min, KNOB_RANGES.filterCutoff.max));
    case 'gainDb':
      return round(lin(value01, KNOB_RANGES.gainDb.min, KNOB_RANGES.gainDb.max), 0.1);
    case 'pan':
      return round(lin(value01, KNOB_RANGES.pan.min, KNOB_RANGES.pan.max), 0.01);
    case 'startSec': {
      if (!bufferDuration || !(bufferDuration > 0)) return null;
      const end = Math.min(bufferDuration, pad.endSec ?? bufferDuration);
      // Keep at least 10 ms of slice.
      return Math.max(0, round(clamp01(value01) * Math.max(0, end - 0.01), 0.001));
    }
  }
}

export const knobSwing = (value01: number): number => round(lin(value01, KNOB_RANGES.swing.min, KNOB_RANGES.swing.max), 0.01);
export const knobBpm = (value01: number): number => Math.round(lin(value01, KNOB_RANGES.bpm.min, KNOB_RANGES.bpm.max));
export const knobVolumeDb = (value01: number): number => round(lin(value01, KNOB_RANGES.volume.min, KNOB_RANGES.volume.max), 0.5);

const unlin = (v: number, min: number, max: number) => clamp01((v - min) / (max - min));
const unlog = (v: number, min: number, max: number) => (v > 0 ? clamp01(Math.log(v / min) / Math.log(max / min)) : 0);

/** Inverse of knobPadValue: where the knob would sit for the pad's current value (0..1), null if unknown. */
export function padParamTo01(param: PadParam, pad: PadSettings, bufferDuration: number | null): number | null {
  switch (param) {
    case 'pitch':
      return unlin(pad.pitch, KNOB_RANGES.pitch.min, KNOB_RANGES.pitch.max);
    case 'decay':
      return unlog(pad.decay, KNOB_RANGES.decay.min, KNOB_RANGES.decay.max);
    case 'filterCutoff':
      return unlog(pad.filterCutoff, KNOB_RANGES.filterCutoff.min, KNOB_RANGES.filterCutoff.max);
    case 'gainDb':
      return unlin(pad.gainDb, KNOB_RANGES.gainDb.min, KNOB_RANGES.gainDb.max);
    case 'pan':
      return unlin(pad.pan, KNOB_RANGES.pan.min, KNOB_RANGES.pan.max);
    case 'startSec': {
      if (!bufferDuration || !(bufferDuration > 0)) return null;
      const end = Math.min(bufferDuration, pad.endSec ?? bufferDuration);
      const span = Math.max(0, end - 0.01);
      return span > 0 ? clamp01((pad.startSec || 0) / span) : 0;
    }
  }
}

export const swingTo01 = (swing: number): number => unlin(swing, KNOB_RANGES.swing.min, KNOB_RANGES.swing.max);
export const bpmTo01 = (bpm: number): number => unlin(bpm, KNOB_RANGES.bpm.min, KNOB_RANGES.bpm.max);
export const volumeTo01 = (db: number): number => unlin(db, KNOB_RANGES.volume.min, KNOB_RANGES.volume.max);

const PARAM_LABELS: Record<PadParam, string> = {
  gainDb: 'Pad Gain',
  pan: 'Pad Pan',
  pitch: 'Pad Pitch',
  decay: 'Pad Decay',
  filterCutoff: 'Pad Filter',
  startSec: 'Pad Start',
};

/** The parts of arrangementEngine the router uses. */
export type KnobEngine = Pick<
  typeof arrangementEngine,
  | 'getActiveDrumTrackId'
  | 'ensureDrumTrack'
  | 'getSelectedPad'
  | 'getKit'
  | 'updatePad'
  | 'beginUndoGroup'
  | 'endUndoGroup'
  | 'setLoopSettings'
  | 'getLoopSettings'
  | 'setBpm'
  | 'getBpm'
  | 'setTrackVolume'
  | 'getTracks'
>;

export interface KnobRouterDeps {
  engine: KnobEngine;
  setMasterVolume: (db: number) => void;
  getMasterVolume: () => number;
  /** Loaded buffer length of a pad's sample, if known. */
  padBufferDuration: (pad: PadSettings) => number | null;
  /** Idle time that ends a knob gesture's undo step. */
  idleMs?: number;
}

export interface KnobRouter {
  handle(target: KnobTarget, value01: number): void;
  /** Close the open undo step now. */
  flush(): void;
  /** Every knob has to be picked up again (soft takeover). */
  resetPickup(): void;
  dispose(): void;
}

export const KNOB_GESTURE_IDLE_MS = 400;

export function createKnobRouter(deps: KnobRouterDeps): KnobRouter {
  const { engine, idleMs = KNOB_GESTURE_IDLE_MS } = deps;
  let gesture: { key: string; timer: ReturnType<typeof setTimeout> } | null = null;

  const flush = () => {
    if (!gesture) return;
    clearTimeout(gesture.timer);
    gesture = null;
    engine.endUndoGroup();
  };

  /** One undo group per (track, pad, param) gesture. */
  const touch = (key: string, label: string) => {
    if (gesture && gesture.key !== key) flush();
    if (!gesture) {
      engine.beginUndoGroup(label);
      gesture = { key, timer: setTimeout(flush, idleMs) };
      return;
    }
    clearTimeout(gesture.timer);
    gesture.timer = setTimeout(flush, idleMs);
  };

  const pickup = new KnobPickup();
  /** Selected pad / active track the pickups were made for. */
  let pickupContext: string | null = null;
  const syncPickupContext = () => {
    const ctx = `${engine.getActiveDrumTrackId() ?? ''}:${engine.getSelectedPad()}`;
    if (ctx !== pickupContext) {
      pickupContext = ctx;
      pickup.reset();
    }
  };

  /** Soft takeover gate: true when the knob may set the value (now at `current01`). */
  const takeover = (key: string, value01: number, current01: number | null): boolean =>
    current01 === null ? true : pickup.check(key, clamp01(value01), current01);

  const handlePad = (param: PadParam, value01: number) => {
    const trackId = engine.getActiveDrumTrackId() ?? engine.ensureDrumTrack();
    const pad: PadIndex = engine.getSelectedPad();
    const settings = engine.getKit(trackId).pads[pad];
    if (!settings) return;
    const duration = deps.padBufferDuration(settings);
    const key = `pad:${param}`;
    if (!takeover(key, value01, padParamTo01(param, settings, duration))) return;
    const value = knobPadValue(param, value01, settings, duration);
    if (value === null || value === settings[param]) return;
    touch(`${trackId}:${pad}:${param}`, PARAM_LABELS[param]);
    engine.updatePad(trackId, pad, { [param]: value });
    const after = engine.getKit(trackId).pads[pad];
    const set01 = after ? padParamTo01(param, after, duration) : null;
    if (set01 !== null) pickup.applied(key, set01);
  };

  const handleSwing = (value01: number) => {
    const current = engine.getLoopSettings().swing;
    if (!takeover('swing', value01, swingTo01(current))) return;
    const swing = knobSwing(value01);
    if (swing === current) return;
    touch('swing', 'Swing');
    engine.setLoopSettings({ swing });
    pickup.applied('swing', swingTo01(engine.getLoopSettings().swing));
  };

  const handleBpm = (value01: number) => {
    const current = engine.getBpm();
    if (!takeover('bpm', value01, bpmTo01(current))) return;
    const bpm = knobBpm(value01); // whole BPM
    if (bpm === current) return;
    touch('bpm', 'Tempo');
    engine.setBpm(bpm);
    pickup.applied('bpm', bpmTo01(bpm));
  };

  const handleTrackVolume = (value01: number) => {
    const trackId = engine.getActiveDrumTrackId();
    const track = trackId ? engine.getTracks().find((t) => t.id === trackId) : undefined;
    if (!trackId || !track) return;
    const key = `trackVolume:${trackId}`;
    if (!takeover(key, value01, volumeTo01(track.volume))) return;
    const db = knobVolumeDb(value01);
    if (db === track.volume) return;
    touch(key, 'Track Volume');
    engine.setTrackVolume(trackId, db);
    pickup.applied(key, volumeTo01(db));
  };

  const handleMasterVolume = (value01: number) => {
    if (!takeover('masterVolume', value01, volumeTo01(deps.getMasterVolume()))) return;
    const db = knobVolumeDb(value01);
    if (db === deps.getMasterVolume()) return;
    deps.setMasterVolume(db);
    pickup.applied('masterVolume', volumeTo01(db));
  };

  return {
    handle(target, value01) {
      syncPickupContext();
      switch (target.kind) {
        case 'pad':
          handlePad(target.param, value01);
          break;
        case 'swing':
          handleSwing(value01);
          break;
        case 'bpm':
          handleBpm(value01);
          break;
        case 'trackVolume':
          handleTrackVolume(value01);
          break;
        case 'masterVolume':
          handleMasterVolume(value01);
          break;
      }
    },
    flush,
    resetPickup: () => pickup.reset(),
    dispose: flush,
  };
}

/**
 * Route the controller's knobs to the app. Master volume goes to the master
 * bus (masterBus.setMasterVolumeDb) unless overridden. Returns an uninstall function.
 */
export function installKnobRouting(
  opts: { setMasterVolume?: (db: number) => void; getMasterVolume?: () => number } = {},
): () => void {
  const router = createKnobRouter({
    engine: arrangementEngine,
    setMasterVolume: opts.setMasterVolume ?? setMasterVolumeDb,
    getMasterVolume: opts.getMasterVolume ?? getMasterVolumeDb,
    padBufferDuration: (pad) => (pad.sample ? getCachedPadBuffer(pad.sample.source)?.duration ?? null : null),
  });
  const off = controllerMapping.onKnob((target, value01) => router.handle(target, value01));
  return () => {
    off();
    router.dispose();
  };
}
