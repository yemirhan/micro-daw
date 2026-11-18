/**
 * Metadata of the built-in classic-machine kits (no DSP here, so it can be
 * imported cheaply by validation / UI code). These are ORIGINAL syntheses of
 * the *style* of classic drum machines: always name them "808-style" etc.,
 * never as the trademarked product.
 */
import type { ClassicMachineId, PadPlayMode } from '@/types/sampler';

export interface ClassicVoiceInfo {
  /** Voice id, unique within its machine (stored in `{ kind: 'classic', voice }`). */
  id: string;
  label: string;
  /** General MIDI drum note that best represents the voice (MIDI export). */
  gm: number;
  /** Default pad gain in dB (voices are all normalized to -1 dBFS; this balances the kit). */
  gainDb?: number;
  /** Choke group on the default kit (1 = hi-hats). */
  choke?: number;
  mode?: PadPlayMode;
}

export interface ClassicMachine {
  id: ClassicMachineId;
  /** Display name, e.g. "808-style". */
  name: string;
  tagline: string;
  description: string;
  /** Accent color (CSS). */
  color: string;
  voices: ClassicVoiceInfo[];
  /** Default 16-pad layout (voice ids). Bank A: kick, snare, closed hat, open hat, clap, tom, cymbal, perc. */
  pads: string[];
}

const HAT = { choke: 1, gainDb: -3 } as const;

export const CLASSIC_MACHINES: readonly ClassicMachine[] = [
  {
    id: 'tr808',
    name: '808-style',
    tagline: 'Booming analog kick, snappy snare, metallic hats',
    description:
      'Analog-modelled voices in the spirit of the 1980 classic: resonator kick with a long tuned tail, two-tone snare with noise, six-oscillator metallic hats and cymbal, multi-burst clap, cowbell, congas and clave.',
    color: 'oklch(0.70 0.18 45)',
    voices: [
      { id: 'kick', label: 'Kick', gm: 36 },
      { id: 'kick-long', label: 'Long Boom', gm: 35, mode: 'oneshot' },
      { id: 'snare', label: 'Snare', gm: 38 },
      { id: 'rim', label: 'Rimshot', gm: 37, gainDb: -2 },
      { id: 'clap', label: 'Clap', gm: 39 },
      { id: 'chh', label: 'Closed Hat', gm: 42, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'cymbal', label: 'Cymbal', gm: 49, gainDb: -4 },
      { id: 'cowbell', label: 'Cowbell', gm: 56, gainDb: -3 },
      { id: 'clave', label: 'Clave', gm: 75, gainDb: -4 },
      { id: 'maracas', label: 'Maracas', gm: 70, gainDb: -5 },
      { id: 'tom-lo', label: 'Low Tom', gm: 41 },
      { id: 'tom-mid', label: 'Mid Tom', gm: 45 },
      { id: 'tom-hi', label: 'High Tom', gm: 48 },
      { id: 'conga-lo', label: 'Low Conga', gm: 64, gainDb: -2 },
      { id: 'conga-mid', label: 'Mid Conga', gm: 63, gainDb: -2 },
      { id: 'conga-hi', label: 'High Conga', gm: 62, gainDb: -2 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'cymbal', 'cowbell',
      'kick-long', 'rim', 'maracas', 'clave', 'tom-lo', 'tom-hi', 'conga-mid', 'conga-hi',
    ],
  },
  {
    id: 'tr909',
    name: '909-style',
    tagline: 'Punchy kick with click, noisy snare, gritty hats',
    description:
      'Hybrid analog/digital style: kick with a fast pitch drop and beater click, noise-heavy snares, tuned toms, and crunchy low-bit-depth hats, crash and ride.',
    color: 'oklch(0.76 0.15 85)',
    voices: [
      { id: 'kick', label: 'Kick', gm: 36 },
      { id: 'kick-long', label: 'Long Kick', gm: 35 },
      { id: 'kick-hard', label: 'Hard Kick', gm: 36 },
      { id: 'snare', label: 'Snare', gm: 38 },
      { id: 'snare-tight', label: 'Tight Snare', gm: 40 },
      { id: 'snare-low', label: 'Low Snare', gm: 38 },
      { id: 'rim', label: 'Rimshot', gm: 37, gainDb: -2 },
      { id: 'clap', label: 'Clap', gm: 39 },
      { id: 'chh', label: 'Closed Hat', gm: 42, ...HAT },
      { id: 'phh', label: 'Pedal Hat', gm: 44, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'crash', label: 'Crash', gm: 49, gainDb: -4 },
      { id: 'ride', label: 'Ride', gm: 51, gainDb: -5 },
      { id: 'tom-lo', label: 'Low Tom', gm: 41 },
      { id: 'tom-mid', label: 'Mid Tom', gm: 45 },
      { id: 'tom-hi', label: 'High Tom', gm: 48 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'crash', 'ride',
      'kick-long', 'rim', 'phh', 'snare-tight', 'kick-hard', 'tom-lo', 'tom-hi', 'snare-low',
    ],
  },
  {
    id: 'tr707',
    name: '707-style',
    tagline: 'Crisp digital drums with an 8-bit edge',
    description:
      'Punchy, sample-like voices with short, natural envelopes and a gentle early-digital crunch: two kicks, two snares, toms, rim, cowbell, clap, tambourine, hats, crash and ride.',
    color: 'oklch(0.64 0.19 22)',
    voices: [
      { id: 'kick', label: 'Kick 1', gm: 36 },
      { id: 'kick-2', label: 'Kick 2', gm: 35 },
      { id: 'snare', label: 'Snare 1', gm: 38 },
      { id: 'snare-2', label: 'Snare 2', gm: 40 },
      { id: 'rim', label: 'Rimshot', gm: 37, gainDb: -2 },
      { id: 'clap', label: 'Clap', gm: 39 },
      { id: 'cowbell', label: 'Cowbell', gm: 56, gainDb: -3 },
      { id: 'tambourine', label: 'Tambourine', gm: 54, gainDb: -4 },
      { id: 'chh', label: 'Closed Hat', gm: 42, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'crash', label: 'Crash', gm: 49, gainDb: -4 },
      { id: 'ride', label: 'Ride', gm: 51, gainDb: -5 },
      { id: 'ride-bell', label: 'Ride Bell', gm: 53, gainDb: -5 },
      { id: 'tom-lo', label: 'Low Tom', gm: 41 },
      { id: 'tom-mid', label: 'Mid Tom', gm: 45 },
      { id: 'tom-hi', label: 'High Tom', gm: 48 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'crash', 'ride',
      'kick-2', 'rim', 'snare-2', 'tambourine', 'cowbell', 'tom-lo', 'tom-hi', 'ride-bell',
    ],
  },
  {
    id: 'tr606',
    name: '606-style',
    tagline: 'Thin, tight, ticking analog drums',
    description:
      'Lean analog voices: short tight kick, bright noisy snare, very tight metallic hats and a splashy cymbal, plus a few companions (clap, rim, shaker, zap) in the same thin style.',
    color: 'oklch(0.72 0.13 205)',
    voices: [
      { id: 'kick', label: 'Kick', gm: 36 },
      { id: 'kick-long', label: 'Long Kick', gm: 35 },
      { id: 'kick-short', label: 'Tight Kick', gm: 36 },
      { id: 'snare', label: 'Snare', gm: 38 },
      { id: 'snare-snappy', label: 'Snappy Snare', gm: 40 },
      { id: 'rim', label: 'Rim', gm: 37, gainDb: -2 },
      { id: 'clap', label: 'Clap', gm: 39 },
      { id: 'chh', label: 'Closed Hat', gm: 42, ...HAT },
      { id: 'phh', label: 'Pedal Hat', gm: 44, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'cymbal', label: 'Cymbal', gm: 49, gainDb: -4 },
      { id: 'shaker', label: 'Shaker', gm: 82, gainDb: -5 },
      { id: 'zap', label: 'Zap', gm: 76, gainDb: -3 },
      { id: 'tom-lo', label: 'Low Tom', gm: 41 },
      { id: 'tom-mid', label: 'Mid Tom', gm: 45 },
      { id: 'tom-hi', label: 'High Tom', gm: 48 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'cymbal', 'phh',
      'kick-long', 'rim', 'snare-snappy', 'shaker', 'zap', 'tom-lo', 'tom-hi', 'kick-short',
    ],
  },
  {
    id: 'linn',
    name: 'LinnDrum-style',
    tagline: '80s studio drums, 8-bit companded',
    description:
      'Acoustic-modelled drums with the companded 8-bit sheen of early-80s sample machines: dry punchy kick, fat snare, sidestick, real-sounding claps, hats, ride, crash, toms, congas, cabasa, tambourine and cowbell.',
    color: 'oklch(0.62 0.17 295)',
    voices: [
      { id: 'kick', label: 'Kick', gm: 36 },
      { id: 'snare', label: 'Snare', gm: 38 },
      { id: 'sidestick', label: 'Sidestick', gm: 37, gainDb: -2 },
      { id: 'clap', label: 'Claps', gm: 39 },
      { id: 'chh', label: 'Closed Hat', gm: 42, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'ride', label: 'Ride', gm: 51, gainDb: -5 },
      { id: 'crash', label: 'Crash', gm: 49, gainDb: -4 },
      { id: 'tom-lo', label: 'Low Tom', gm: 41 },
      { id: 'tom-mid', label: 'Mid Tom', gm: 45 },
      { id: 'tom-hi', label: 'High Tom', gm: 48 },
      { id: 'conga-lo', label: 'Low Conga', gm: 64, gainDb: -2 },
      { id: 'conga-hi', label: 'High Conga', gm: 63, gainDb: -2 },
      { id: 'cabasa', label: 'Cabasa', gm: 69, gainDb: -5 },
      { id: 'tambourine', label: 'Tambourine', gm: 54, gainDb: -4 },
      { id: 'cowbell', label: 'Cowbell', gm: 56, gainDb: -3 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clap', 'tom-mid', 'crash', 'ride',
      'sidestick', 'cabasa', 'tambourine', 'cowbell', 'tom-lo', 'tom-hi', 'conga-lo', 'conga-hi',
    ],
  },
  {
    id: 'cr78',
    name: 'CR-78-style',
    tagline: 'Soft, organic preset-rhythm percussion',
    description:
      'Gentle, rounded voices from the preset rhythm-box era: soft resonant kick and snare, ticking hats, guiro, maracas, tambourine, bongos, congas, clave, cowbell and the metallic "metal beat".',
    color: 'oklch(0.72 0.12 145)',
    voices: [
      { id: 'kick', label: 'Kick', gm: 36 },
      { id: 'snare', label: 'Snare', gm: 38 },
      { id: 'rim', label: 'Rim', gm: 37, gainDb: -2 },
      { id: 'chh', label: 'Hi-Hat', gm: 42, ...HAT },
      { id: 'ohh', label: 'Open Hat', gm: 46, ...HAT },
      { id: 'cymbal', label: 'Cymbal', gm: 49, gainDb: -4 },
      { id: 'metal', label: 'Metal Beat', gm: 80, gainDb: -4 },
      { id: 'maracas', label: 'Maracas', gm: 70, gainDb: -5 },
      { id: 'guiro', label: 'Guiro', gm: 73, gainDb: -4 },
      { id: 'tambourine', label: 'Tambourine', gm: 54, gainDb: -4 },
      { id: 'clave', label: 'Clave', gm: 75, gainDb: -4 },
      { id: 'cowbell', label: 'Cowbell', gm: 56, gainDb: -3 },
      { id: 'bongo-lo', label: 'Low Bongo', gm: 61, gainDb: -2 },
      { id: 'bongo-hi', label: 'High Bongo', gm: 60, gainDb: -2 },
      { id: 'conga-lo', label: 'Low Conga', gm: 64, gainDb: -2 },
      { id: 'conga-hi', label: 'High Conga', gm: 63, gainDb: -2 },
    ],
    pads: [
      'kick', 'snare', 'chh', 'ohh', 'clave', 'conga-lo', 'cymbal', 'cowbell',
      'rim', 'maracas', 'guiro', 'tambourine', 'metal', 'bongo-lo', 'bongo-hi', 'conga-hi',
    ],
  },
];

export const CLASSIC_MACHINE_IDS: readonly ClassicMachineId[] = CLASSIC_MACHINES.map((m) => m.id);

export function isClassicMachineId(v: unknown): v is ClassicMachineId {
  return typeof v === 'string' && CLASSIC_MACHINES.some((m) => m.id === v);
}

export function getClassicMachine(id: ClassicMachineId): ClassicMachine {
  const m = CLASSIC_MACHINES.find((x) => x.id === id);
  if (!m) throw new Error(`Unknown classic machine: ${id}`);
  return m;
}

export function getClassicVoice(machine: ClassicMachineId, voice: string): ClassicVoiceInfo | undefined {
  return CLASSIC_MACHINES.find((m) => m.id === machine)?.voices.find((v) => v.id === voice);
}

export function isClassicVoice(machine: unknown, voice: unknown): boolean {
  return isClassicMachineId(machine) && typeof voice === 'string' && !!getClassicVoice(machine, voice);
}

/** Display label of a classic voice ("Kick"), or the raw id when unknown. */
export function classicVoiceLabel(machine: ClassicMachineId, voice: string): string {
  return getClassicVoice(machine, voice)?.label ?? voice;
}

/** Pad sample name for a classic voice, e.g. "808 Kick". */
export function classicSampleName(machine: ClassicMachineId, voice: string): string {
  const short = getClassicMachine(machine).name.replace(/-style$/, '');
  return `${short} ${classicVoiceLabel(machine, voice)}`;
}

/** Id of a machine's built-in kit. */
export const classicKitId = (machine: ClassicMachineId): string => `classic-${machine}`;
