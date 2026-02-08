import type { SynthPreset } from '@/types/audio';
import type { CCMapping } from '@/types/effects';
import type { DrumPadId, DrumSound } from '@/types/drums';

export const SYNTH_PRESETS: SynthPreset[] = [
  {
    name: 'Classic',
    oscillator: { type: 'triangle' },
    envelope: { attack: 0.01, decay: 0.3, sustain: 0.4, release: 0.8 },
  },
  {
    name: 'Bright',
    oscillator: { type: 'sawtooth' },
    envelope: { attack: 0.005, decay: 0.2, sustain: 0.3, release: 0.5 },
  },
  {
    name: 'FM',
    oscillator: { type: 'fmsquare', modulationType: 'sine', modulationIndex: 3, harmonicity: 2 },
    envelope: { attack: 0.01, decay: 0.4, sustain: 0.2, release: 0.6 },
  },
  {
    name: 'Warm',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.05, decay: 0.5, sustain: 0.6, release: 1.2 },
  },
  {
    name: 'AM',
    oscillator: { type: 'amsquare', modulationType: 'sine', harmonicity: 1.5 },
    envelope: { attack: 0.01, decay: 0.3, sustain: 0.35, release: 0.7 },
  },
  {
    name: 'Pluck',
    oscillator: { type: 'fmtriangle', modulationIndex: 2, harmonicity: 3 },
    envelope: { attack: 0.001, decay: 0.25, sustain: 0, release: 0.3 },
  },
  {
    name: 'Pad',
    oscillator: { type: 'fatsawtooth', spread: 30 },
    envelope: { attack: 0.4, decay: 0.8, sustain: 0.7, release: 2.0 },
  },
  {
    name: 'Bass',
    oscillator: { type: 'fatsawtooth', spread: 15 },
    envelope: { attack: 0.005, decay: 0.3, sustain: 0.5, release: 0.3 },
  },
  {
    name: 'Sub Bass',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.005, decay: 0.1, sustain: 0.9, release: 0.4 },
  },
  {
    name: 'Organ',
    oscillator: { type: 'fatsine', spread: 20 },
    envelope: { attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.05 },
  },
  {
    name: 'Lead',
    oscillator: { type: 'fatsquare', spread: 20 },
    envelope: { attack: 0.01, decay: 0.2, sustain: 0.6, release: 0.4 },
  },
  {
    name: 'Bell',
    oscillator: { type: 'fmsine', modulationIndex: 8, harmonicity: 5.4 },
    envelope: { attack: 0.001, decay: 1.5, sustain: 0, release: 2.0 },
  },
  {
    name: 'Strings',
    oscillator: { type: 'fatsawtooth', spread: 40 },
    envelope: { attack: 0.3, decay: 0.5, sustain: 0.8, release: 1.0 },
  },
  {
    name: 'Electric Piano',
    oscillator: { type: 'fmtriangle', modulationIndex: 1.5, harmonicity: 3.5 },
    envelope: { attack: 0.005, decay: 0.6, sustain: 0.3, release: 0.8 },
  },
  {
    name: 'Brass',
    oscillator: { type: 'fatsquare', spread: 25 },
    envelope: { attack: 0.08, decay: 0.3, sustain: 0.7, release: 0.3 },
  },
];

// Piano range: C2 (36) to C6 (84)
export const PIANO_START = 36;
export const PIANO_END = 84;

export const DEFAULT_VOLUME = -12;
export const MIN_VOLUME = -40;
export const MAX_VOLUME = 0;

export const MAX_POLYPHONY = 16;

export const MPK_MINI_PATTERN = /mpk\s*mini/i;

// CC mappings for effects — both standard CCs and MPK Mini knob CCs
export const CC_MAPPINGS: CCMapping[] = [
  { cc: 1, effect: 'filterCutoff', label: 'Filter' },
  { cc: 2, effect: 'filterResonance', label: 'Resonance' },
  { cc: 3, effect: 'reverbWet', label: 'Reverb' },
  { cc: 4, effect: 'chorusDepth', label: 'Chorus' },
  { cc: 70, effect: 'filterCutoff', label: 'Filter' },
  { cc: 71, effect: 'filterResonance', label: 'Resonance' },
  { cc: 72, effect: 'reverbWet', label: 'Reverb' },
  { cc: 73, effect: 'chorusDepth', label: 'Chorus' },
];

// Volume CCs (separate from effects)
export const VOLUME_CCS = [5, 74];

// Drum sounds for 8 pads — GM standard MIDI note numbers
export const DRUM_SOUNDS: DrumSound[] = [
  { id: 0, name: 'Kick', shortName: 'KCK', midiNote: 36, color: 'oklch(0.62 0.22 25)' },
  { id: 1, name: 'Snare', shortName: 'SNR', midiNote: 38, color: 'oklch(0.70 0.18 65)' },
  { id: 2, name: 'Closed HH', shortName: 'CHH', midiNote: 42, color: 'oklch(0.70 0.20 150)' },
  { id: 3, name: 'Open HH', shortName: 'OHH', midiNote: 46, color: 'oklch(0.65 0.18 180)' },
  { id: 4, name: 'Clap', shortName: 'CLP', midiNote: 39, color: 'oklch(0.62 0.22 310)' },
  { id: 5, name: 'Tom', shortName: 'TOM', midiNote: 45, color: 'oklch(0.65 0.20 265)' },
  { id: 6, name: 'Crash', shortName: 'CRS', midiNote: 49, color: 'oklch(0.75 0.17 90)' },
  { id: 7, name: 'Ride', shortName: 'RDE', midiNote: 51, color: 'oklch(0.65 0.18 210)' },
];

// GM drum note → pad ID lookup
export const DRUM_NOTE_TO_PAD = new Map<number, DrumPadId>(
  DRUM_SOUNDS.map(s => [s.midiNote, s.id])
);

// MIDI channel 10 = drums (GM standard)
export const DRUM_CHANNEL = 10;

export const DEFAULT_BPM = 120;
export const MIN_BPM = 40;
export const MAX_BPM = 240;
export const RECORDINGS_STORAGE_KEY = 'micro-daw-recordings';
export const MAX_RECORDINGS = 20;

export type RoutingMode = 'auto' | 'keys' | 'split';

export const ARRANGEMENT_STORAGE_KEY = 'micro-daw-arrangement';

export const TRACK_COLORS = [
  'oklch(0.65 0.20 250)',   // blue
  'oklch(0.65 0.20 155)',   // green
  'oklch(0.62 0.22 25)',    // red
  'oklch(0.70 0.18 65)',    // orange
  'oklch(0.62 0.22 310)',   // purple
  'oklch(0.75 0.17 90)',    // yellow
  'oklch(0.62 0.16 195)',   // teal
  'oklch(0.65 0.20 345)',   // pink
];

export const DEFAULT_PX_PER_BEAT = 30;
export const MIN_PX_PER_BEAT = 10;
export const MAX_PX_PER_BEAT = 80;
export const SNAP_VALUES = [0.25, 0.5, 1, 2, 4] as const;
export const DEFAULT_ARRANGEMENT_LENGTH = 64; // beats (16 bars)

export const SETTINGS_STORAGE_KEY = 'micro-daw-settings';
export const SAMPLE_LIBRARY_STORAGE_KEY = 'micro-daw-sample-library';

export const DEFAULT_SETTINGS: import('@/types/settings').AppSettings = {
  general: { autoCheckUpdates: true },
  audio: { defaultMasterVolume: -12, bufferSizeHint: 256 },
  midi: { autoConnectLastDevice: true },
};
