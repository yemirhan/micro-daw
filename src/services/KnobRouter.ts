import { arrangementEngine } from '@/services/ArrangementEngine';
import { controllerMapping } from '@/services/ControllerMapping';
import { getCachedPadBuffer } from '@/audio/sampler/bufferCache';
import { MAX_VOLUME, MIN_VOLUME } from '@/utils/constants';
import type { KnobTarget, PadIndex, PadParam, PadSettings } from '@/types/sampler';

/**
 * Hardware knobs (controllerMapping.onKnob) → app parameters.
 *
 * Pad parameters act on the selected pad of the active drum track. A knob
 * twist is a burst of CC messages; each burst becomes one undo step (an undo
 * group closed after the knob has been idle for a moment, or when another
 * knob/pad starts moving). Swing, tempo and volumes are not undoable, like
 * their on-screen controls.
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
  | 'setBpm'
  | 'setTrackVolume'
>;

export interface KnobRouterDeps {
  engine: KnobEngine;
  setMasterVolume: (db: number) => void;
  /** Loaded buffer length of a pad's sample, if known. */
  padBufferDuration: (pad: PadSettings) => number | null;
  /** Idle time that ends a knob gesture's undo step. */
  idleMs?: number;
}

export interface KnobRouter {
  handle(target: KnobTarget, value01: number): void;
  /** Close the open undo step now. */
  flush(): void;
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

  const handlePad = (param: PadParam, value01: number) => {
    const trackId = engine.getActiveDrumTrackId() ?? engine.ensureDrumTrack();
    const pad: PadIndex = engine.getSelectedPad();
    const settings = engine.getKit(trackId).pads[pad];
    if (!settings) return;
    const value = knobPadValue(param, value01, settings, deps.padBufferDuration(settings));
    if (value === null || value === settings[param]) return;
    touch(`${trackId}:${pad}:${param}`, PARAM_LABELS[param]);
    engine.updatePad(trackId, pad, { [param]: value });
  };

  return {
    handle(target, value01) {
      switch (target.kind) {
        case 'pad':
          handlePad(target.param, value01);
          break;
        case 'swing':
          engine.setLoopSettings({ swing: knobSwing(value01) });
          break;
        case 'bpm':
          engine.setBpm(knobBpm(value01));
          break;
        case 'trackVolume': {
          const trackId = engine.getActiveDrumTrackId();
          if (trackId) engine.setTrackVolume(trackId, knobVolumeDb(value01));
          break;
        }
        case 'masterVolume':
          deps.setMasterVolume(knobVolumeDb(value01));
          break;
      }
    },
    flush,
    dispose: flush,
  };
}

/** Route the controller's knobs to the app. Returns an uninstall function. */
export function installKnobRouting(opts: { setMasterVolume: (db: number) => void }): () => void {
  const router = createKnobRouter({
    engine: arrangementEngine,
    setMasterVolume: opts.setMasterVolume,
    padBufferDuration: (pad) => (pad.sample ? getCachedPadBuffer(pad.sample.source)?.duration ?? null : null),
  });
  const off = controllerMapping.onKnob((target, value01) => router.handle(target, value01));
  return () => {
    off();
    router.dispose();
  };
}
