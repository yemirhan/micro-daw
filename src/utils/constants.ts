import type { SynthPreset, PresetCategory, PresetVibe } from '@/types/audio';
import type { CCMapping } from '@/types/effects';
import type { DrumPadId, DrumSound } from '@/types/drums';

export const PRESET_CATEGORIES: { id: PresetCategory; label: string }[] = [
  { id: 'keys', label: 'Keys' },
  { id: 'bass', label: 'Bass' },
  { id: 'lead', label: 'Lead' },
  { id: 'pad', label: 'Pad' },
  { id: 'pluck', label: 'Pluck' },
  { id: 'brass', label: 'Brass' },
  { id: 'bell', label: 'Bell' },
  { id: 'percussion', label: 'Percussion' },
  { id: 'fx', label: 'FX' },
];

export const PRESET_VIBES: { id: PresetVibe; label: string; emoji: string }[] = [
  { id: 'chill', label: 'Chill', emoji: '🌊' },
  { id: 'bright', label: 'Bright & Sparkly', emoji: '✨' },
  { id: 'dark', label: 'Dark & Moody', emoji: '🌙' },
  { id: 'punchy', label: 'Punchy & Bold', emoji: '🔥' },
  { id: 'dreamy', label: 'Dreamy & Lush', emoji: '☁️' },
  { id: 'aggressive', label: 'Aggressive', emoji: '⚡' },
  { id: 'organic', label: 'Organic & Acoustic', emoji: '🌿' },
  { id: 'experimental', label: 'Experimental', emoji: '🔮' },
];

export const SYNTH_PRESETS: SynthPreset[] = [
  // ─── Existing presets (indices 0–14, backward compatible) ───
  {
    name: 'Classic',
    engine: 'synth',
    category: 'keys',
    vibe: 'chill',
    oscillator: { type: 'triangle' },
    envelope: { attack: 0.01, decay: 0.3, sustain: 0.4, release: 0.8 },
  },
  {
    name: 'Bright',
    engine: 'synth',
    category: 'lead',
    vibe: 'bright',
    oscillator: { type: 'sawtooth' },
    envelope: { attack: 0.005, decay: 0.2, sustain: 0.3, release: 0.5 },
  },
  {
    name: 'FM',
    engine: 'synth',
    category: 'keys',
    vibe: 'bright',
    oscillator: { type: 'fmsquare', modulationType: 'sine', modulationIndex: 3, harmonicity: 2 },
    envelope: { attack: 0.01, decay: 0.4, sustain: 0.2, release: 0.6 },
  },
  {
    name: 'Warm',
    engine: 'synth',
    category: 'pad',
    vibe: 'chill',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.05, decay: 0.5, sustain: 0.6, release: 1.2 },
  },
  {
    name: 'AM',
    engine: 'synth',
    category: 'lead',
    vibe: 'experimental',
    oscillator: { type: 'amsquare', modulationType: 'sine', harmonicity: 1.5 },
    envelope: { attack: 0.01, decay: 0.3, sustain: 0.35, release: 0.7 },
  },
  {
    name: 'Pluck',
    engine: 'synth',
    category: 'pluck',
    vibe: 'bright',
    oscillator: { type: 'fmtriangle', modulationIndex: 2, harmonicity: 3 },
    envelope: { attack: 0.001, decay: 0.25, sustain: 0, release: 0.3 },
  },
  {
    name: 'Pad',
    engine: 'synth',
    category: 'pad',
    vibe: 'dreamy',
    oscillator: { type: 'fatsawtooth', spread: 30 },
    envelope: { attack: 0.4, decay: 0.8, sustain: 0.7, release: 2.0 },
  },
  {
    name: 'Bass',
    engine: 'synth',
    category: 'bass',
    vibe: 'punchy',
    oscillator: { type: 'fatsawtooth', spread: 15 },
    envelope: { attack: 0.005, decay: 0.3, sustain: 0.5, release: 0.3 },
  },
  {
    name: 'Sub Bass',
    engine: 'synth',
    category: 'bass',
    vibe: 'dark',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.005, decay: 0.1, sustain: 0.9, release: 0.4 },
  },
  {
    name: 'Organ',
    engine: 'synth',
    category: 'keys',
    vibe: 'chill',
    oscillator: { type: 'fatsine', spread: 20 },
    envelope: { attack: 0.005, decay: 0.1, sustain: 1.0, release: 0.05 },
  },
  {
    name: 'Lead',
    engine: 'synth',
    category: 'lead',
    vibe: 'punchy',
    oscillator: { type: 'fatsquare', spread: 20 },
    envelope: { attack: 0.01, decay: 0.2, sustain: 0.6, release: 0.4 },
  },
  {
    name: 'Bell',
    engine: 'synth',
    category: 'bell',
    vibe: 'bright',
    oscillator: { type: 'fmsine', modulationIndex: 8, harmonicity: 5.4 },
    envelope: { attack: 0.001, decay: 1.5, sustain: 0, release: 2.0 },
  },
  {
    name: 'Strings',
    engine: 'synth',
    category: 'pad',
    vibe: 'dreamy',
    oscillator: { type: 'fatsawtooth', spread: 40 },
    envelope: { attack: 0.3, decay: 0.5, sustain: 0.8, release: 1.0 },
  },
  {
    name: 'Electric Piano',
    engine: 'synth',
    category: 'keys',
    vibe: 'chill',
    oscillator: { type: 'fmtriangle', modulationIndex: 1.5, harmonicity: 3.5 },
    envelope: { attack: 0.005, decay: 0.6, sustain: 0.3, release: 0.8 },
  },
  {
    name: 'Brass',
    engine: 'synth',
    category: 'brass',
    vibe: 'punchy',
    oscillator: { type: 'fatsquare', spread: 25 },
    envelope: { attack: 0.08, decay: 0.3, sustain: 0.7, release: 0.3 },
  },

  // ─── FM Synth presets ───
  {
    name: 'DX Piano',
    engine: 'fmsynth',
    category: 'keys',
    vibe: 'bright',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.005, decay: 0.8, sustain: 0.2, release: 1.0 },
    modulationIndex: 12,
    harmonicity: 1,
    modulationEnvelope: { attack: 0.001, decay: 0.4, sustain: 0.1, release: 0.5 },
  },
  {
    name: 'Marimba',
    engine: 'fmsynth',
    category: 'keys',
    vibe: 'organic',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.5, sustain: 0, release: 0.3 },
    modulationIndex: 4,
    harmonicity: 4,
    modulationEnvelope: { attack: 0.001, decay: 0.1, sustain: 0, release: 0.2 },
  },
  {
    name: 'FM Bell',
    engine: 'fmsynth',
    category: 'bell',
    vibe: 'bright',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 2.0, sustain: 0, release: 2.5 },
    modulationIndex: 20,
    harmonicity: 5.4,
    modulationEnvelope: { attack: 0.001, decay: 1.5, sustain: 0, release: 2.0 },
  },
  {
    name: 'Glass Bell',
    engine: 'fmsynth',
    category: 'bell',
    vibe: 'dreamy',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 1.5, sustain: 0, release: 1.8 },
    modulationIndex: 15,
    harmonicity: 7,
    modulationEnvelope: { attack: 0.001, decay: 0.8, sustain: 0, release: 1.2 },
  },
  {
    name: 'FM Bass',
    engine: 'fmsynth',
    category: 'bass',
    vibe: 'dark',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.005, decay: 0.3, sustain: 0.4, release: 0.2 },
    modulationIndex: 8,
    harmonicity: 1,
    modulationEnvelope: { attack: 0.001, decay: 0.2, sustain: 0.1, release: 0.1 },
  },
  {
    name: 'Acid Bass',
    engine: 'fmsynth',
    category: 'bass',
    vibe: 'aggressive',
    oscillator: { type: 'square' },
    envelope: { attack: 0.005, decay: 0.2, sustain: 0.3, release: 0.15 },
    modulationIndex: 6,
    harmonicity: 0.5,
    modulationEnvelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.1 },
  },

  // ─── AM Synth presets ───
  {
    name: 'AM Lead',
    engine: 'amsynth',
    category: 'lead',
    vibe: 'punchy',
    oscillator: { type: 'sawtooth' },
    envelope: { attack: 0.01, decay: 0.3, sustain: 0.5, release: 0.4 },
    harmonicity: 2,
    modulationEnvelope: { attack: 0.01, decay: 0.2, sustain: 0.3, release: 0.3 },
  },
  {
    name: 'Tremolo Lead',
    engine: 'amsynth',
    category: 'lead',
    vibe: 'experimental',
    oscillator: { type: 'square' },
    envelope: { attack: 0.01, decay: 0.2, sustain: 0.6, release: 0.5 },
    harmonicity: 1.5,
    modulationEnvelope: { attack: 0.5, decay: 0.3, sustain: 0.8, release: 0.5 },
  },
  {
    name: 'AM Pad',
    engine: 'amsynth',
    category: 'pad',
    vibe: 'dreamy',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.5, decay: 1.0, sustain: 0.7, release: 2.0 },
    harmonicity: 3,
    modulationEnvelope: { attack: 0.8, decay: 0.5, sustain: 0.6, release: 1.5 },
  },

  // ─── Mono Synth presets ───
  {
    name: 'Mono Bass',
    engine: 'monosynth',
    category: 'bass',
    vibe: 'dark',
    oscillator: { type: 'sawtooth' },
    envelope: { attack: 0.005, decay: 0.2, sustain: 0.5, release: 0.2 },
    filterEnvelope: { attack: 0.01, decay: 0.2, sustain: 0.2, release: 0.1, baseFrequency: 200, octaves: 3 },
  },
  {
    name: 'Squelch Bass',
    engine: 'monosynth',
    category: 'bass',
    vibe: 'aggressive',
    oscillator: { type: 'square' },
    envelope: { attack: 0.005, decay: 0.15, sustain: 0.3, release: 0.15 },
    filterEnvelope: { attack: 0.005, decay: 0.1, sustain: 0.1, release: 0.05, baseFrequency: 150, octaves: 4 },
  },
  {
    name: 'Mono Lead',
    engine: 'monosynth',
    category: 'lead',
    vibe: 'aggressive',
    oscillator: { type: 'sawtooth' },
    envelope: { attack: 0.01, decay: 0.15, sustain: 0.6, release: 0.3 },
    filterEnvelope: { attack: 0.01, decay: 0.15, sustain: 0.4, release: 0.2, baseFrequency: 800, octaves: 2.5 },
  },
  {
    name: 'Portamento Lead',
    engine: 'monosynth',
    category: 'lead',
    vibe: 'chill',
    oscillator: { type: 'square' },
    envelope: { attack: 0.02, decay: 0.2, sustain: 0.7, release: 0.4 },
    filterEnvelope: { attack: 0.02, decay: 0.3, sustain: 0.5, release: 0.3, baseFrequency: 600, octaves: 3 },
  },

  // ─── Pluck Synth presets ───
  {
    name: 'Nylon Guitar',
    engine: 'plucksynth',
    category: 'pluck',
    vibe: 'organic',
    attackNoise: 1,
    resonance: 0.96,
    dampening: 3000,
    release: 1.2,
  },
  {
    name: 'Harp',
    engine: 'plucksynth',
    category: 'pluck',
    vibe: 'dreamy',
    attackNoise: 0.5,
    resonance: 0.99,
    dampening: 5000,
    release: 2.0,
  },
  {
    name: 'Kalimba',
    engine: 'plucksynth',
    category: 'pluck',
    vibe: 'organic',
    attackNoise: 2,
    resonance: 0.92,
    dampening: 2000,
    release: 0.8,
  },

  // ─── Membrane Synth presets ───
  {
    name: 'Timpani',
    engine: 'membranesynth',
    category: 'percussion',
    vibe: 'punchy',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.8, sustain: 0, release: 0.5 },
    pitchDecay: 0.05,
    octaves: 6,
  },
  {
    name: 'Steel Drum',
    engine: 'membranesynth',
    category: 'percussion',
    vibe: 'bright',
    oscillator: { type: 'sine' },
    envelope: { attack: 0.001, decay: 0.4, sustain: 0.05, release: 0.3 },
    pitchDecay: 0.01,
    octaves: 3,
  },

  // ─── FX presets (synth engine) ───
  {
    name: 'Wobble',
    engine: 'synth',
    category: 'fx',
    vibe: 'experimental',
    oscillator: { type: 'fatsawtooth', spread: 50 },
    envelope: { attack: 0.1, decay: 0.5, sustain: 0.3, release: 0.8 },
  },
  {
    name: 'Sweep',
    engine: 'synth',
    category: 'fx',
    vibe: 'dreamy',
    oscillator: { type: 'fatsine', spread: 60 },
    envelope: { attack: 1.0, decay: 0.5, sustain: 0.4, release: 2.0 },
  },
  {
    name: 'Noise Hit',
    engine: 'synth',
    category: 'fx',
    vibe: 'experimental',
    oscillator: { type: 'fmtriangle', modulationIndex: 20, harmonicity: 0.5 },
    envelope: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.1 },
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

export const MARKER_COLORS = [
  'oklch(0.65 0.20 25)',    // red
  'oklch(0.70 0.18 65)',    // orange
  'oklch(0.75 0.17 90)',    // yellow
  'oklch(0.65 0.20 155)',   // green
  'oklch(0.65 0.20 250)',   // blue
  'oklch(0.62 0.22 310)',   // purple
  'oklch(0.65 0.20 345)',   // pink
  'oklch(0.62 0.16 195)',   // teal
];

export const DEFAULT_MARKER_NAMES = [
  'Intro', 'Verse', 'Chorus', 'Bridge', 'Drop', 'Breakdown', 'Outro', 'Section',
];

export const SETTINGS_STORAGE_KEY = 'micro-daw-settings';
export const SAMPLE_LIBRARY_STORAGE_KEY = 'micro-daw-sample-library';

export const PRACTICE_STATS_STORAGE_KEY = 'micro-daw-practice-stats';

export const DEFAULT_SETTINGS: import('@/types/settings').AppSettings = {
  general: { autoCheckUpdates: true, hasCompletedOnboarding: false },
  audio: { defaultMasterVolume: -12, bufferSizeHint: 256 },
  midi: { autoConnectLastDevice: true },
};
