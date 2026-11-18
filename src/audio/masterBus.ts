import * as Tone from 'tone';
import { DEFAULT_VOLUME, MAX_VOLUME, MIN_VOLUME } from '@/utils/constants';

/**
 * Master bus, inserted on a context's Destination so EVERYTHING (live synth,
 * drum pads, arrangement tracks, metronome, previews, sample audition) passes
 * through it:
 *
 *   Destination input -> master volume (real-time context only: the app's
 *                        master fader, TopBar slider + master knob)
 *                     -> drive (fixed +2 dB "glue" makeup x shaper input scale)
 *                     -> soft clipper (zero-latency true-peak ceiling, ~ -0.2 dBFS)
 *                     -> speakers
 *
 * No DynamicsCompressor on purpose: in Chromium every one adds a fixed 6 ms
 * look-ahead delay (measured with an offline click render: 6.0 ms through the
 * old bus, 0 ms now), which you feel when finger-drumming. The +2 dB drive
 * matches the loudness the old compressor's automatic makeup gain gave quiet
 * and normal material; loud peaks are rounded off by the waveshaper instead of
 * being compressed. The ceiling makes it impossible for the output to exceed
 * 0 dBFS.
 */

const CLIP_KNEE = 0.85;
const CLIP_CEILING = 0.98;
/** Fixed bus gain (dB) before the clipper: the old bus compressor's makeup gain below its threshold. */
export const MASTER_DRIVE_DB = 2;
/** Input range covered by the soft-clip curve: signal x in [-CLIP_RANGE, CLIP_RANGE] (beyond saturates at the ceiling). */
const CLIP_RANGE = 2;

/** Soft clipper transfer curve; domain is the shaper input u in [-1, 1] == signal x in [-2, 2]. */
export function softClip(x: number): number {
  const a = Math.abs(x);
  if (a <= CLIP_KNEE) return x;
  const room = CLIP_CEILING - CLIP_KNEE;
  return Math.sign(x) * (CLIP_KNEE + room * Math.tanh((a - CLIP_KNEE) / room));
}

export interface MasterBus {
  /** Master fader (0 dB in offline renders; follows getMasterVolumeDb() on the real-time bus). */
  master: Tone.Volume;
  /** Drive into the clipper (MASTER_DRIVE_DB, scaled to the shaper's [-1, 1] input). */
  drive: Tone.Gain;
  dispose(): void;
}

// ---------------------------------------------------------------------------
// Master volume (the user-facing "Master" level, dB on the app's -40..0 scale)
// ---------------------------------------------------------------------------

/** Master volume at which the bus is unity gain (the app's historical default level). */
export const MASTER_REFERENCE_DB = DEFAULT_VOLUME;
const STORAGE_KEY = 'micro-daw:masterVolume';
const RAMP_SEC = 0.03;

/** Pure: master fader setting (dB) -> master bus gain (dB). The default level is unity. */
export function masterGainDb(volumeDb: number): number {
  return clampVolume(volumeDb) - MASTER_REFERENCE_DB;
}

function clampVolume(db: number): number {
  return Number.isFinite(db) ? Math.min(MAX_VOLUME, Math.max(MIN_VOLUME, db)) : DEFAULT_VOLUME;
}

function loadStoredVolume(): number {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem(STORAGE_KEY) : null;
    const v = raw === null ? NaN : Number(raw);
    return Number.isFinite(v) ? clampVolume(v) : DEFAULT_VOLUME;
  } catch {
    return DEFAULT_VOLUME;
  }
}

let masterVolumeDb = loadStoredVolume();
const volumeListeners = new Set<(db: number) => void>();
/** The real-time bus (the one the master volume drives). */
let liveBus: MasterBus | null = null;

export function getMasterVolumeDb(): number {
  return masterVolumeDb;
}

/** Set the master volume (dB, MIN_VOLUME..MAX_VOLUME): every sound in the app follows. Persisted. */
export function setMasterVolumeDb(db: number): void {
  const v = clampVolume(db);
  if (v === masterVolumeDb) return;
  masterVolumeDb = v;
  try {
    liveBus?.master.volume.rampTo(masterGainDb(v), RAMP_SEC);
  } catch {
    /* disposed */
  }
  try {
    localStorage.setItem(STORAGE_KEY, String(v));
  } catch {
    /* storage unavailable */
  }
  for (const cb of [...volumeListeners]) cb(v);
}

export function onMasterVolumeChange(cb: (db: number) => void): () => void {
  volumeListeners.add(cb);
  return () => {
    volumeListeners.delete(cb);
  };
}

// ---------------------------------------------------------------------------

const installed = new WeakMap<object, MasterBus>();

/** Create the master processing nodes and chain them onto `destination`. */
export function installMasterBus(
  destination: Tone.ToneAudioNode = Tone.getDestination(),
  { masterDb = 0 }: { masterDb?: number } = {},
): MasterBus {
  const existing = installed.get(destination);
  if (existing) return existing;

  const master = new Tone.Volume(masterDb);
  // One gain node = makeup drive x the shaper's input scale (u = x / CLIP_RANGE).
  const drive = new Tone.Gain(Math.pow(10, MASTER_DRIVE_DB / 20) / CLIP_RANGE);
  // No oversampling on purpose: the up/down-sampling filters ring past the
  // curve's ceiling (measured +0.3 dBFS); without them the ceiling is exact.
  const clipper = new Tone.WaveShaper((u: number) => softClip(u * CLIP_RANGE), 4096);

  (destination as unknown as { chain: (...nodes: Tone.ToneAudioNode[]) => void }).chain(master, drive, clipper);

  const bus: MasterBus = {
    master,
    drive,
    dispose: () => {
      for (const n of [master, drive, clipper]) {
        try {
          n.dispose();
        } catch {
          /* already disposed */
        }
      }
      installed.delete(destination);
      if (liveBus === bus) liveBus = null;
    },
  };
  installed.set(destination, bus);
  return bus;
}

/** Idempotent: install on the current (real-time) context's destination, driven by the master volume. */
export function ensureMasterBus(): MasterBus {
  const destination = Tone.getDestination();
  const bus = installed.get(destination) ?? installMasterBus(destination, { masterDb: masterGainDb(masterVolumeDb) });
  if (liveBus !== bus) {
    liveBus = bus;
    bus.master.volume.value = masterGainDb(masterVolumeDb);
  }
  return bus;
}
