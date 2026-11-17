import {
  PAD_COUNT,
  type FactoryVoiceId,
  type KitState,
  type PadIndex,
  type PadPlayMode,
  type PadSample,
  type PadSampleSource,
  type PadSettings,
} from '@/types/sampler';
import { classicSampleName, isClassicMachineId, isClassicVoice } from './classic/machines';
import { packRefFromPath } from '@/shared/packSource';
import { isSafePackPath, isValidPackId } from '@/shared/packValidate';
import { basename, displayName } from '@/shared/paths';

// ---------------------------------------------------------------------------
// Packs folder (for pack sample sources)
// ---------------------------------------------------------------------------

let packsDirOverride: string | null | undefined;

/**
 * `<userData>/packs`, known synchronously in Electron (the preload exposes it
 * as `electronAPI.packsDirPath`); null in a plain browser / tests.
 */
export function currentPacksDir(): string | null {
  if (packsDirOverride !== undefined) return packsDirOverride;
  const dir = typeof window !== 'undefined' ? window.electronAPI?.packsDirPath : undefined;
  return typeof dir === 'string' && dir.length > 0 ? dir : null;
}

/** Tests only: pretend the packs folder is `dir` (undefined = back to the Electron value). */
export function setPacksDirForTests(dir: string | null | undefined): void {
  packsDirOverride = dir;
}

/** Id of the built-in kit (user kits get their own ids). */
export const FACTORY_KIT_ID = 'factory';

export const FACTORY_VOICE_LABELS: Record<FactoryVoiceId, string> = {
  kick: 'Kick',
  'kick-sub': 'Sub Kick',
  snare: 'Snare',
  rim: 'Rim',
  clap: 'Clap',
  chh: 'Closed HH',
  phh: 'Pedal HH',
  ohh: 'Open HH',
  'tom-lo': 'Low Tom',
  'tom-mid': 'Mid Tom',
  'tom-hi': 'High Tom',
  crash: 'Crash',
  ride: 'Ride',
  shaker: 'Shaker',
  perc: 'Cowbell',
  snap: 'Snap',
};

export const FACTORY_VOICE_IDS = Object.keys(FACTORY_VOICE_LABELS) as FactoryVoiceId[];

/**
 * Factory pad layout. Bank A (pads 0..7) keeps the order of the old 8-pad kit
 * (kick, snare, closed hat, open hat, clap, tom, crash, ride).
 */
export const FACTORY_PAD_VOICES: readonly FactoryVoiceId[] = [
  'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'crash', 'ride',
  'kick-sub', 'rim', 'phh', 'shaker', 'snap', 'tom-lo', 'tom-hi', 'perc',
];

/** Hi-hats choke each other (closed/pedal cut the open hat). */
const FACTORY_CHOKE: Partial<Record<FactoryVoiceId, number>> = { chh: 1, phh: 1, ohh: 1 };

/** One color per pad; bank A matches the old pad colors. */
export const PAD_COLORS: string[] = [
  'oklch(0.62 0.22 25)',
  'oklch(0.70 0.18 65)',
  'oklch(0.70 0.20 150)',
  'oklch(0.65 0.18 180)',
  'oklch(0.62 0.22 310)',
  'oklch(0.65 0.20 265)',
  'oklch(0.75 0.17 90)',
  'oklch(0.65 0.18 210)',
  'oklch(0.58 0.20 10)',
  'oklch(0.72 0.16 50)',
  'oklch(0.74 0.17 135)',
  'oklch(0.70 0.15 195)',
  'oklch(0.66 0.20 330)',
  'oklch(0.60 0.18 285)',
  'oklch(0.78 0.15 105)',
  'oklch(0.68 0.16 235)',
];

/** General MIDI drum note that best represents each factory voice (MIDI export). */
export const FACTORY_VOICE_GM_NOTE: Record<FactoryVoiceId, number> = {
  kick: 36,
  'kick-sub': 35,
  snare: 38,
  rim: 37,
  clap: 39,
  chh: 42,
  phh: 44,
  ohh: 46,
  'tom-lo': 41,
  'tom-mid': 45,
  'tom-hi': 48,
  crash: 49,
  ride: 51,
  shaker: 82,
  perc: 56,
  snap: 75,
};

/** GM drum note -> closest factory voice (used to migrate old GM drum notes). */
const GM_TO_FACTORY_VOICE: Record<number, FactoryVoiceId> = {
  35: 'kick', 36: 'kick',
  37: 'rim', 38: 'snare', 40: 'snare',
  39: 'clap',
  42: 'chh', 44: 'phh', 46: 'ohh',
  41: 'tom-lo', 43: 'tom-lo', 45: 'tom-mid', 47: 'tom-mid', 48: 'tom-hi', 50: 'tom-hi',
  49: 'crash', 52: 'crash', 55: 'crash', 57: 'crash',
  51: 'ride', 53: 'ride', 59: 'ride',
  54: 'shaker', 69: 'shaker', 70: 'shaker', 82: 'shaker',
  56: 'perc', 76: 'perc', 77: 'perc', 75: 'snap',
};

export function gmNoteToFactoryVoice(note: number): FactoryVoiceId | null {
  return GM_TO_FACTORY_VOICE[Math.round(note)] ?? null;
}

/** Factory-kit pad that plays the voice closest to a GM drum note, or null. */
export function factoryPadForGmNote(note: number): PadIndex | null {
  const voice = gmNoteToFactoryVoice(note);
  if (!voice) return null;
  const pad = FACTORY_PAD_VOICES.indexOf(voice);
  return pad >= 0 ? pad : null;
}

export function isFactoryVoiceId(v: unknown): v is FactoryVoiceId {
  return typeof v === 'string' && v in FACTORY_VOICE_LABELS;
}

export function factorySample(voice: FactoryVoiceId): PadSample {
  return { source: { kind: 'factory', voice }, name: FACTORY_VOICE_LABELS[voice] };
}

export function defaultPadSettings(): PadSettings {
  return {
    sample: null,
    gainDb: 0,
    pan: 0,
    pitch: 0,
    startSec: 0,
    endSec: null,
    decay: 0.05,
    filterCutoff: 20000,
    reverse: false,
    mode: 'oneshot',
    chokeGroup: 0,
  };
}

export function createFactoryKit(): KitState {
  return {
    id: FACTORY_KIT_ID,
    name: 'Factory',
    pads: FACTORY_PAD_VOICES.map((voice) => ({
      ...defaultPadSettings(),
      sample: factorySample(voice),
      chokeGroup: FACTORY_CHOKE[voice] ?? 0,
    })),
  };
}

// ---------------------------------------------------------------------------
// Validation / cloning (for persisted kits and project files)
// ---------------------------------------------------------------------------

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const num = (v: unknown, fallback: number) => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);

function normalizeSource(src: unknown): PadSampleSource | null {
  if (!src || typeof src !== 'object') return null;
  const s = src as Record<string, unknown>;
  if (s.kind === 'factory' && isFactoryVoiceId(s.voice)) return { kind: 'factory', voice: s.voice };
  if (s.kind === 'classic' && isClassicMachineId(s.machine) && isClassicVoice(s.machine, s.voice)) {
    return { kind: 'classic', machine: s.machine, voice: s.voice as string };
  }
  if (s.kind === 'pack' && isValidPackId(s.packId) && isSafePackPath(s.file)) {
    return { kind: 'pack', packId: s.packId, file: s.file };
  }
  if (s.kind === 'file' && typeof s.path === 'string' && s.path) {
    // Migration: absolute paths into the packs folder (older pack kits) become pack sources.
    const ref = packRefFromPath(currentPacksDir(), s.path);
    if (ref) return { kind: 'pack', ...ref };
    return typeof s.sampleId === 'string'
      ? { kind: 'file', path: s.path, sampleId: s.sampleId }
      : { kind: 'file', path: s.path };
  }
  return null;
}

/** Display name for a sample source without a stored name. */
export function sourceFallbackName(source: PadSampleSource): string {
  switch (source.kind) {
    case 'factory':
      return FACTORY_VOICE_LABELS[source.voice];
    case 'classic':
      return classicSampleName(source.machine, source.voice);
    case 'pack':
      return displayName(basename(source.file)) || 'Sample';
    case 'file':
      return displayName(basename(source.path)) || 'Sample';
  }
}

function normalizeSample(sample: unknown): PadSample | null {
  if (!sample || typeof sample !== 'object') return null;
  const s = sample as Record<string, unknown>;
  const source = normalizeSource(s.source);
  if (!source) return null;
  return { source, name: typeof s.name === 'string' && s.name ? s.name : sourceFallbackName(source) };
}

/** Coerce untrusted pad data (old/foreign JSON) into valid, clamped PadSettings. */
export function normalizePad(raw: unknown): PadSettings {
  const d = defaultPadSettings();
  if (!raw || typeof raw !== 'object') return d;
  const p = raw as Record<string, unknown>;
  const startSec = Math.max(0, num(p.startSec, d.startSec));
  const endRaw = p.endSec;
  const endSec = typeof endRaw === 'number' && Number.isFinite(endRaw) && endRaw > startSec ? endRaw : null;
  const mode: PadPlayMode = p.mode === 'gate' ? 'gate' : 'oneshot';
  const pad: PadSettings = {
    sample: normalizeSample(p.sample),
    gainDb: clamp(num(p.gainDb, d.gainDb), -60, 12),
    pan: clamp(num(p.pan, d.pan), -1, 1),
    pitch: clamp(num(p.pitch, d.pitch), -24, 24),
    startSec,
    endSec,
    decay: clamp(num(p.decay, d.decay), 0.005, 4),
    filterCutoff: clamp(num(p.filterCutoff, d.filterCutoff), 20, 20000),
    reverse: p.reverse === true,
    mode,
    chokeGroup: Math.max(0, Math.round(num(p.chokeGroup, 0))),
  };
  if (typeof p.color === 'string' && p.color) pad.color = p.color;
  return pad;
}

/** Valid KitState with exactly PAD_COUNT pads (missing pads are empty). */
export function normalizeKit(raw: unknown): KitState {
  const k = (raw && typeof raw === 'object' ? raw : {}) as Record<string, unknown>;
  const pads = Array.isArray(k.pads) ? k.pads : [];
  return {
    id: typeof k.id === 'string' && k.id ? k.id : crypto.randomUUID(),
    name: typeof k.name === 'string' && k.name ? k.name : 'Kit',
    pads: Array.from({ length: PAD_COUNT }, (_, i) => normalizePad(pads[i])),
  };
}

/** Deep copy of a kit (pads, samples and sources are all fresh objects). */
export function cloneKit(kit: KitState): KitState {
  return {
    id: kit.id,
    name: kit.name,
    pads: kit.pads.map((p) => ({
      ...p,
      sample: p.sample ? { ...p.sample, source: { ...p.sample.source } } : null,
    })),
  };
}
