import type {
  CharacterParams,
  FMPreset,
  OscParams,
  PluckPreset,
  PresetCategory,
  PresetVibe,
  SamplerPreset,
  SubtractivePreset,
  SynthPreset,
} from '@/types/audio';

/*
 * The instrument bank.
 *
 * BACKWARD COMPATIBILITY: projects/templates store `presetIndex`. Slot i of
 * this array keeps the musical role of the old preset i (0 "Classic" keys ->
 * Grand Piano, 7 "Bass" -> Analog Bass, 13 "Electric Piano" -> Rhodes ...),
 * so old indices load something close to what they were. New sounds are
 * appended after index 35. Every preset also has a stable `id`.
 * Slots 0..23 and 33..35 were Tone.PolySynth before, so they must stay
 * polyphonic (a mono preset would collapse saved chords to one note).
 */

// ---------------------------------------------------------------------------
// Builders
// ---------------------------------------------------------------------------

const ch = (c: Partial<CharacterParams> = {}): CharacterParams => ({
  drive: 0,
  chorus: 0,
  tremolo: 0,
  tremoloRate: 4.5,
  tone: 20000,
  presence: 0,
  ...c,
});

const osc = (wave: OscParams['wave'], level = 1, o: Partial<OscParams> = {}): OscParams => ({
  wave,
  level,
  semi: 0,
  fine: 0,
  unison: 1,
  spread: 0,
  ...o,
});

const OFF = osc('sawtooth', 0);

type Meta = { id: string; name: string; category: PresetCategory; vibe: PresetVibe; description?: string };

type SubInput = Meta &
  Partial<Omit<SubtractivePreset, 'engine' | keyof Meta | 'filter' | 'amp' | 'filterEnv' | 'lfo' | 'noise' | 'sub' | 'pitchEnv' | 'character'>> & {
    filter?: Partial<SubtractivePreset['filter']>;
    filterEnv?: Partial<SubtractivePreset['filterEnv']>;
    amp?: Partial<SubtractivePreset['amp']>;
    lfo?: Partial<SubtractivePreset['lfo']>;
    noise?: Partial<SubtractivePreset['noise']>;
    sub?: Partial<SubtractivePreset['sub']>;
    pitchEnv?: Partial<SubtractivePreset['pitchEnv']>;
    character?: Partial<CharacterParams>;
  };

function sub(p: SubInput): SubtractivePreset {
  return {
    engine: 'subtractive',
    gain: 0,
    polyphony: 12,
    oscA: osc('sawtooth'),
    oscB: OFF,
    mono: false,
    legato: true,
    glide: 0,
    ...p,
    sub: { level: 0, wave: 'sine', octave: -1, ...p.sub },
    noise: { level: 0, type: 'white', decay: 0, ...p.noise },
    filter: {
      type: 'lowpass',
      cutoff: 2000,
      resonance: 1,
      rolloff: -24,
      keyTrack: 0.5,
      envAmount: 2,
      velocity: 0.5,
      ...p.filter,
    },
    filterEnv: { attack: 0.005, decay: 0.4, sustain: 0.3, release: 0.4, ...p.filterEnv },
    amp: { attack: 0.004, decay: 0.3, sustain: 0.8, release: 0.3, velocity: 0.7, ...p.amp },
    pitchEnv: { amount: 0, decay: 0, ...p.pitchEnv },
    lfo: { rate: 5, vibrato: 0, filter: 0, delay: 0, ...p.lfo },
    character: ch(p.character),
  };
}

type FMInput = Meta &
  Partial<Omit<FMPreset, 'engine' | keyof Meta | 'amp' | 'modEnv' | 'ping' | 'lfo' | 'character'>> & {
    amp?: Partial<FMPreset['amp']>;
    modEnv?: Partial<FMPreset['modEnv']>;
    ping?: Partial<FMPreset['ping']>;
    lfo?: Partial<FMPreset['lfo']>;
    character?: Partial<CharacterParams>;
  };

function fm(p: FMInput): FMPreset {
  return {
    engine: 'fm',
    gain: 0,
    polyphony: 16,
    carrier: { wave: 'sine' },
    ratio: 1,
    index: 2,
    velocityToIndex: 0.7,
    keyScale: 0.5,
    detune: 0,
    ...p,
    modEnv: { attack: 0.001, decay: 0.8, sustain: 0.2, release: 0.4, ...p.modEnv },
    amp: { attack: 0.002, decay: 2, sustain: 0, release: 0.4, velocity: 0.7, ...p.amp },
    ping: { ratio: 4, level: 0, decay: 0.05, ...p.ping },
    lfo: { rate: 5, vibrato: 0, filter: 0, delay: 0, ...p.lfo },
    character: ch(p.character),
  };
}

function pluck(p: Meta & Partial<Omit<PluckPreset, 'engine' | keyof Meta | 'character'>> & { character?: Partial<CharacterParams> }): PluckPreset {
  return {
    engine: 'pluck',
    gain: 0,
    polyphony: 8,
    attackNoise: 1,
    dampening: 4000,
    resonance: 0.97,
    release: 1,
    velocity: 0.7,
    ...p,
    character: ch(p.character),
  };
}

function sampler(p: Meta & Partial<Omit<SamplerPreset, 'engine' | keyof Meta | 'character'>> & { character?: Partial<CharacterParams> }): SamplerPreset {
  return {
    engine: 'sampler',
    sample: 'piano',
    gain: 0,
    polyphony: 48,
    release: 0.45,
    velocityCurve: 1.6,
    ...p,
    character: ch(p.character),
  };
}

// Organ drawbars as harmonics of the 8' fundamental: 8', 4', 2 2/3', 2', (1 3/5'), 1 1/3', -, 1'
const drawbars = (b8: number, b4: number, b223: number, b2: number, b135: number, b113: number, b1: number) => [
  b8, b4, b223, b2, b135, b113, 0, b1,
];

// ---------------------------------------------------------------------------
// The bank
// ---------------------------------------------------------------------------

export const SYNTH_PRESETS: SynthPreset[] = [
  // 0 (was "Classic")
  sampler({
    id: 'grand-piano', name: 'Grand Piano', category: 'keys', vibe: 'organic',
    description: 'Sampled concert grand (Salamander)',
    gain: 3.5, release: 0.5, velocityCurve: 1.5, character: { tone: 16000 },
  }),
  // 1 (was "Bright")
  sub({
    id: 'saw-lead', name: 'Saw Lead', category: 'lead', vibe: 'bright', polyphony: 8, gain: 2.5,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 14 }),
    oscB: osc('sawtooth', 0.55, { fine: 7 }),
    sub: { level: 0.25, wave: 'square' },
    filter: { cutoff: 2400, resonance: 3, keyTrack: 0.6, envAmount: 2.2, velocity: 0.5 },
    filterEnv: { attack: 0.005, decay: 0.35, sustain: 0.45, release: 0.3 },
    amp: { attack: 0.003, decay: 0.2, sustain: 0.85, release: 0.25, velocity: 0.5 },
    lfo: { rate: 5.5, vibrato: 12, delay: 0.45 },
    character: { chorus: 0.15 },
  }),
  // 2 (was "FM")
  fm({
    id: 'fm-keys', name: 'FM Keys', category: 'keys', vibe: 'bright', gain: -2.5,
    description: 'Glassy DX-style electric piano',
    ratio: 1, index: 3.2, velocityToIndex: 0.85, keyScale: 0.6,
    modEnv: { attack: 0.001, decay: 1.1, sustain: 0.12, release: 0.3 },
    amp: { attack: 0.002, decay: 3.2, sustain: 0, release: 0.35, velocity: 0.7 },
    ping: { ratio: 14, level: 0.22, decay: 0.07 }, detune: 3,
    character: { chorus: 0.3 },
  }),
  // 3 (was "Warm")
  sub({
    id: 'warm-pad', name: 'Warm Pad', category: 'pad', vibe: 'chill', polyphony: 10, gain: 2,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 18 }),
    oscB: osc('triangle', 0.5, { semi: -12 }),
    filter: { cutoff: 850, resonance: 1.5, keyTrack: 0.3, envAmount: 1.2, velocity: 0.3 },
    filterEnv: { attack: 0.8, decay: 1.5, sustain: 0.6, release: 1.5 },
    amp: { attack: 0.6, decay: 1, sustain: 0.85, release: 1.6, velocity: 0.35 },
    lfo: { rate: 0.25, filter: 300 },
    character: { chorus: 0.5 },
  }),
  // 4 (was "AM")
  sub({
    id: 'sync-lead', name: 'Sync Lead', category: 'lead', vibe: 'aggressive', gain: -2,
    description: 'Screaming resonant sweep',
    oscA: osc('sawtooth', 1),
    oscB: osc('sawtooth', 0.6, { semi: 12, fine: 5 }),
    filter: { cutoff: 1100, resonance: 9, rolloff: -12, keyTrack: 0.7, envAmount: 3.5, velocity: 0.5 },
    filterEnv: { attack: 0.002, decay: 0.5, sustain: 0.3, release: 0.25 },
    amp: { attack: 0.003, decay: 0.2, sustain: 0.9, release: 0.2, velocity: 0.4 },
    lfo: { rate: 5.8, vibrato: 14, delay: 0.35 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
    character: { drive: 0.35 },
  }),
  // 5 (was "Pluck")
  sub({
    id: 'synth-pluck', name: 'Synth Pluck', category: 'pluck', vibe: 'bright', polyphony: 12, gain: 5,
    oscA: osc('sawtooth', 1, { unison: 2, spread: 10 }),
    oscB: osc('square', 0.35, { semi: 12 }),
    filter: { cutoff: 480, resonance: 3, keyTrack: 0.6, envAmount: 4, velocity: 0.7 },
    filterEnv: { attack: 0.001, decay: 0.22, sustain: 0, release: 0.2 },
    amp: { attack: 0.001, decay: 0.55, sustain: 0, release: 0.3, velocity: 0.7 },
    character: { chorus: 0.25 },
  }),
  // 6 (was "Pad")
  sub({
    id: 'supersaw-pad', name: 'Supersaw Pad', category: 'pad', vibe: 'dreamy', polyphony: 8, gain: 1,
    oscA: osc('sawtooth', 1, { unison: 7, spread: 36 }),
    oscB: osc('sawtooth', 0.35, { semi: 12, unison: 5, spread: 24 }),
    filter: { cutoff: 2000, resonance: 1, keyTrack: 0.3, envAmount: 0.8, velocity: 0.3 },
    filterEnv: { attack: 1, decay: 2, sustain: 0.7, release: 2 },
    amp: { attack: 0.5, decay: 1, sustain: 0.9, release: 2.2, velocity: 0.3 },
    lfo: { rate: 0.15, filter: 200 },
    character: { chorus: 0.35 },
  }),
  // 7 (was "Bass")
  sub({
    id: 'analog-bass', name: 'Analog Bass', category: 'bass', vibe: 'punchy', gain: -2,
    oscA: osc('sawtooth', 1),
    oscB: osc('square', 0.5, { fine: -8 }),
    sub: { level: 0.6, wave: 'sine' },
    filter: { cutoff: 340, resonance: 3, keyTrack: 0.4, envAmount: 2.6, velocity: 0.6 },
    filterEnv: { attack: 0.002, decay: 0.25, sustain: 0.25, release: 0.15 },
    amp: { attack: 0.002, decay: 0.3, sustain: 0.9, release: 0.12, velocity: 0.5 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
    character: { drive: 0.2 },
  }),
  // 8 (was "Sub Bass")
  sub({
    id: 'sub-bass', name: 'Sub Bass', category: 'bass', vibe: 'dark', gain: -5,
    oscA: osc('sine', 1),
    oscB: osc('triangle', 0.25),
    filter: { cutoff: 320, resonance: 0.5, keyTrack: 0.5, envAmount: 0.5, velocity: 0.3 },
    filterEnv: { attack: 0.003, decay: 0.2, sustain: 0.3, release: 0.1 },
    amp: { attack: 0.004, decay: 0.2, sustain: 1, release: 0.12, velocity: 0.4 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
    character: { drive: 0.15 },
  }),
  // 9 (was "Organ")
  sub({
    id: 'drawbar-organ', name: 'Drawbar Organ', category: 'keys', vibe: 'chill', polyphony: 16, gain: 4,
    description: 'Tonewheel organ, 888000000-ish with click and Leslie',
    oscA: osc('sine', 1, { partials: drawbars(0.9, 0.7, 0.45, 0.4, 0, 0.2, 0.25) }),
    sub: { level: 0.55, wave: 'sine' },
    noise: { level: 0.25, type: 'white', decay: 0.006 },
    filter: { cutoff: 9000, resonance: 0.3, rolloff: -12, keyTrack: 0, envAmount: 0, velocity: 0 },
    amp: { attack: 0.004, decay: 0.05, sustain: 1, release: 0.06, velocity: 0.1 },
    lfo: { rate: 6.4, vibrato: 7 },
    character: { chorus: 0.45, tremolo: 0.18, tremoloRate: 6.4, drive: 0.12 },
  }),
  // 10 (was "Lead")
  sub({
    id: 'square-lead', name: 'Square Lead', category: 'lead', vibe: 'punchy', gain: -2.5,
    oscA: osc('square', 1, { unison: 2, spread: 10 }),
    oscB: osc('square', 0.4, { semi: -12 }),
    filter: { cutoff: 2800, resonance: 2, keyTrack: 0.5, envAmount: 1.5, velocity: 0.5 },
    filterEnv: { attack: 0.004, decay: 0.3, sustain: 0.5, release: 0.25 },
    amp: { attack: 0.003, decay: 0.2, sustain: 0.85, release: 0.2, velocity: 0.5 },
    lfo: { rate: 5.5, vibrato: 15, delay: 0.3 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
  }),
  // 11 (was "Bell")
  fm({
    id: 'tubular-bell', name: 'Tubular Bell', category: 'bell', vibe: 'bright', gain: -2,
    ratio: 3.5, index: 4, velocityToIndex: 0.7, keyScale: 0.4,
    modEnv: { attack: 0.001, decay: 2.5, sustain: 0.1, release: 1.5 },
    amp: { attack: 0.001, decay: 4.5, sustain: 0, release: 2.5, velocity: 0.7 },
    ping: { ratio: 2.76, level: 0.3, decay: 0.6 },
  }),
  // 12 (was "Strings")
  sub({
    id: 'string-ensemble', name: 'String Ensemble', category: 'pad', vibe: 'dreamy', polyphony: 10, gain: 1,
    oscA: osc('sawtooth', 1, { unison: 5, spread: 22 }),
    oscB: osc('pulse', 0.3, { semi: 12, width: 0.3 }),
    filter: { cutoff: 3000, resonance: 0.5, keyTrack: 0.5, envAmount: 0.6, velocity: 0.4 },
    filterEnv: { attack: 0.4, decay: 1, sustain: 0.7, release: 0.8 },
    amp: { attack: 0.35, decay: 0.5, sustain: 0.9, release: 0.9, velocity: 0.5 },
    lfo: { rate: 5, vibrato: 7, delay: 0.6 },
    character: { chorus: 0.6 },
  }),
  // 13 (was "Electric Piano")
  fm({
    id: 'rhodes', name: 'Rhodes EP', category: 'keys', vibe: 'chill', gain: 6,
    description: 'Tine electric piano with stereo tremolo; play harder for bark',
    ratio: 1, index: 2.2, velocityToIndex: 1, keyScale: 0.6,
    modEnv: { attack: 0.001, decay: 0.9, sustain: 0.12, release: 0.4 },
    amp: { attack: 0.002, decay: 3.5, sustain: 0, release: 0.4, velocity: 0.75 },
    ping: { ratio: 14, level: 0.2, decay: 0.05 }, detune: 2,
    character: { tremolo: 0.35, tremoloRate: 4.5, chorus: 0.15, drive: 0.08 },
  }),
  // 14 (was "Brass")
  sub({
    id: 'synth-brass', name: 'Synth Brass', category: 'brass', vibe: 'punchy', polyphony: 10, gain: 1.5,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 12 }),
    oscB: osc('sawtooth', 0.7, { fine: -10 }),
    filter: { cutoff: 700, resonance: 1.5, keyTrack: 0.5, envAmount: 2.2, velocity: 0.6 },
    filterEnv: { attack: 0.06, decay: 0.5, sustain: 0.45, release: 0.3 },
    amp: { attack: 0.03, decay: 0.3, sustain: 0.85, release: 0.25, velocity: 0.6 },
    lfo: { rate: 5.5, vibrato: 6, delay: 0.5 },
    character: { chorus: 0.2 },
  }),
  // 15 (was "DX Piano")
  sampler({
    id: 'bright-piano', name: 'Bright Piano', category: 'keys', vibe: 'bright',
    description: 'Grand piano voiced for pop/rock mixes',
    gain: 3, release: 0.4, velocityCurve: 1.3, character: { presence: 5, drive: 0.05 },
  }),
  // 16 (was "Marimba")
  fm({
    id: 'marimba', name: 'Marimba', category: 'bell', vibe: 'organic', gain: 1.5,
    ratio: 4, index: 1.6, velocityToIndex: 0.8, keyScale: 0.5,
    modEnv: { attack: 0.001, decay: 0.12, sustain: 0, release: 0.1 },
    amp: { attack: 0.001, decay: 0.8, sustain: 0, release: 0.35, velocity: 0.7 },
    ping: { ratio: 10, level: 0.1, decay: 0.02 },
  }),
  // 17 (was "FM Bell")
  fm({
    id: 'music-box', name: 'Music Box', category: 'bell', vibe: 'bright', gain: -1,
    ratio: 7, index: 1.2, velocityToIndex: 0.6, keyScale: 0.3,
    modEnv: { attack: 0.001, decay: 0.15, sustain: 0, release: 0.2 },
    amp: { attack: 0.001, decay: 2.2, sustain: 0, release: 1.2, velocity: 0.6 },
    ping: { ratio: 4.2, level: 0.25, decay: 0.25 },
  }),
  // 18 (was "Glass Bell")
  fm({
    id: 'vibraphone', name: 'Vibraphone', category: 'bell', vibe: 'dreamy', gain: 3.5,
    ratio: 4, index: 0.8, velocityToIndex: 0.7, keyScale: 0.5,
    modEnv: { attack: 0.001, decay: 0.3, sustain: 0, release: 0.3 },
    amp: { attack: 0.001, decay: 3.5, sustain: 0, release: 1.2, velocity: 0.7 },
    ping: { ratio: 10, level: 0.08, decay: 0.03 },
    character: { tremolo: 0.45, tremoloRate: 5.2 },
  }),
  // 19 (was "FM Bass")
  fm({
    id: 'fm-bass', name: 'FM Bass', category: 'bass', vibe: 'dark', polyphony: 4, gain: -3.5,
    ratio: 1, index: 3, velocityToIndex: 0.8, keyScale: 0,
    modEnv: { attack: 0.001, decay: 0.25, sustain: 0.2, release: 0.1 },
    amp: { attack: 0.002, decay: 0.8, sustain: 0.7, release: 0.1, velocity: 0.5 },
    character: { drive: 0.1 },
  }),
  // 20 (was "Acid Bass")
  sub({
    id: 'acid-bass', name: 'Acid Bass', category: 'bass', vibe: 'aggressive', gain: -2.5,
    description: '303-style squelch: velocity = accent',
    oscA: osc('sawtooth', 1),
    filter: { cutoff: 300, resonance: 10, keyTrack: 0.5, envAmount: 3.5, velocity: 0.9 },
    filterEnv: { attack: 0.002, decay: 0.28, sustain: 0.05, release: 0.12 },
    amp: { attack: 0.002, decay: 0.3, sustain: 0.8, release: 0.08, velocity: 0.5 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
    character: { drive: 0.45 },
  }),
  // 21 (was "AM Lead")
  sub({
    id: 'pulse-lead', name: 'Pulse Lead', category: 'lead', vibe: 'punchy', polyphony: 6, gain: -10,
    oscA: osc('pulse', 1, { width: 0.25 }),
    oscB: osc('pulse', 0.7, { width: 0.6, fine: 8 }),
    filter: { cutoff: 2600, resonance: 2, keyTrack: 0.5, envAmount: 1.2, velocity: 0.5 },
    filterEnv: { attack: 0.003, decay: 0.3, sustain: 0.5, release: 0.25 },
    amp: { attack: 0.004, decay: 0.2, sustain: 0.85, release: 0.25, velocity: 0.5 },
    lfo: { rate: 5.8, vibrato: 10, delay: 0.35 },
    character: { chorus: 0.2 },
  }),
  // 22 (was "Tremolo Lead")
  sub({
    id: 'theremin', name: 'Theremin', category: 'lead', vibe: 'experimental', gain: -8.5,
    oscA: osc('sine', 1),
    oscB: osc('triangle', 0.15, { semi: 12 }),
    filter: { cutoff: 6000, resonance: 0.3, keyTrack: 0.3, envAmount: 0, velocity: 0 },
    amp: { attack: 0.08, decay: 0.1, sustain: 1, release: 0.35, velocity: 0.3 },
    lfo: { rate: 5.8, vibrato: 28, delay: 0.25 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
  }),
  // 23 (was "AM Pad")
  sub({
    id: 'airy-pad', name: 'Airy Pad', category: 'pad', vibe: 'dreamy', polyphony: 10, gain: 3.5,
    oscA: osc('triangle', 0.8, { unison: 3, spread: 15 }),
    oscB: osc('sawtooth', 0.35, { semi: 12, unison: 3, spread: 20 }),
    noise: { level: 0.25, type: 'pink', decay: 0 },
    filter: { cutoff: 3200, resonance: 1, keyTrack: 0.4, envAmount: 0.5, velocity: 0.2 },
    filterEnv: { attack: 1.5, decay: 2, sustain: 0.8, release: 2.5 },
    amp: { attack: 1.2, decay: 1, sustain: 0.9, release: 2.8, velocity: 0.25 },
    lfo: { rate: 0.12, filter: 500 },
    character: { chorus: 0.55 },
  }),
  // 24 (was "Mono Bass")
  sub({
    id: 'reese-bass', name: 'Reese Bass', category: 'bass', vibe: 'dark', gain: -1,
    oscA: osc('sawtooth', 1, { fine: -9 }),
    oscB: osc('sawtooth', 1, { fine: 9 }),
    sub: { level: 0.6, wave: 'sine' },
    filter: { cutoff: 600, resonance: 2, keyTrack: 0.3, envAmount: 0.6, velocity: 0.4 },
    filterEnv: { attack: 0.02, decay: 0.6, sustain: 0.6, release: 0.3 },
    amp: { attack: 0.005, decay: 0.3, sustain: 1, release: 0.15, velocity: 0.4 },
    lfo: { rate: 0.2, filter: 250 },
    mono: true, legato: true, glide: 0.03,
    character: { drive: 0.3 },
  }),
  // 25 (was "Squelch Bass")
  sub({
    id: 'pluck-bass', name: 'Pluck Bass', category: 'bass', vibe: 'aggressive', gain: 0.5,
    oscA: osc('sawtooth', 1),
    oscB: osc('square', 0.5, { semi: -12 }),
    filter: { cutoff: 220, resonance: 6, keyTrack: 0.4, envAmount: 3.8, velocity: 0.8 },
    filterEnv: { attack: 0.001, decay: 0.14, sustain: 0.05, release: 0.1 },
    amp: { attack: 0.001, decay: 0.35, sustain: 0.25, release: 0.1, velocity: 0.6 },
    mono: true, legato: false, glide: 0,
    character: { drive: 0.25 },
  }),
  // 26 (was "Mono Lead")
  sub({
    id: 'mono-lead', name: 'Mono Lead', category: 'lead', vibe: 'aggressive', gain: -1,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 14 }),
    oscB: osc('square', 0.5, { semi: -12 }),
    filter: { cutoff: 1500, resonance: 4, keyTrack: 0.6, envAmount: 2.8, velocity: 0.5 },
    filterEnv: { attack: 0.003, decay: 0.4, sustain: 0.35, release: 0.25 },
    amp: { attack: 0.003, decay: 0.2, sustain: 0.9, release: 0.2, velocity: 0.4 },
    lfo: { rate: 5.6, vibrato: 14, delay: 0.35 },
    mono: true, legato: true, glide: 0.04,
    character: { drive: 0.3 },
  }),
  // 27 (was "Portamento Lead")
  sub({
    id: 'glide-lead', name: 'Glide Lead', category: 'lead', vibe: 'chill', gain: -8.5,
    oscA: osc('square', 1),
    oscB: osc('triangle', 0.4, { semi: 12 }),
    filter: { cutoff: 2000, resonance: 2, keyTrack: 0.5, envAmount: 1.5, velocity: 0.4 },
    filterEnv: { attack: 0.02, decay: 0.3, sustain: 0.5, release: 0.3 },
    amp: { attack: 0.01, decay: 0.2, sustain: 0.85, release: 0.3, velocity: 0.4 },
    lfo: { rate: 5.2, vibrato: 12, delay: 0.4 },
    mono: true, legato: true, glide: 0.12,
    character: { chorus: 0.15 },
  }),
  // 28 (was "Nylon Guitar")
  pluck({
    id: 'nylon-guitar', name: 'Nylon Guitar', category: 'pluck', vibe: 'organic', gain: -2.5,
    attackNoise: 1.2, dampening: 3200, resonance: 0.975, release: 1, velocity: 0.7,
    character: { presence: -2 },
  }),
  // 29 (was "Harp")
  pluck({
    id: 'harp', name: 'Harp', category: 'pluck', vibe: 'dreamy', gain: -1.5,
    attackNoise: 0.6, dampening: 5200, resonance: 0.985, release: 2.2, velocity: 0.5,
    character: { chorus: 0.15 },
  }),
  // 30 (was "Kalimba")
  fm({
    id: 'kalimba', name: 'Kalimba', category: 'pluck', vibe: 'organic', gain: 0.5,
    ratio: 5.4, index: 0.9, velocityToIndex: 0.7, keyScale: 0.4,
    modEnv: { attack: 0.001, decay: 0.06, sustain: 0, release: 0.05 },
    amp: { attack: 0.001, decay: 1.4, sustain: 0, release: 0.6, velocity: 0.7 },
    ping: { ratio: 3.1, level: 0.2, decay: 0.09 },
  }),
  // 31 (was "Timpani")
  sub({
    id: 'timpani', name: 'Timpani', category: 'percussion', vibe: 'punchy', polyphony: 6, gain: 0.5,
    oscA: osc('sine', 1),
    oscB: osc('sine', 0.35, { semi: 7, fine: 2 }),
    sub: { level: 0.3, wave: 'sine' },
    noise: { level: 0.4, type: 'brown', decay: 0.06 },
    filter: { cutoff: 1200, resonance: 0.5, keyTrack: 0.5, envAmount: 1.2, velocity: 0.6 },
    filterEnv: { attack: 0.001, decay: 0.4, sustain: 0.2, release: 1.2 },
    amp: { attack: 0.002, decay: 1.8, sustain: 0, release: 1, velocity: 0.8 },
    pitchEnv: { amount: 1, decay: 0.12 },
  }),
  // 32 (was "Steel Drum")
  fm({
    id: 'steel-drum', name: 'Steel Drum', category: 'percussion', vibe: 'bright', gain: 1,
    ratio: 1, index: 1.5, velocityToIndex: 0.7, keyScale: 0.4,
    modEnv: { attack: 0.001, decay: 0.25, sustain: 0.1, release: 0.2 },
    amp: { attack: 0.001, decay: 1.2, sustain: 0, release: 0.5, velocity: 0.7 },
    ping: { ratio: 3, level: 0.25, decay: 0.2 },
  }),
  // 33 (was "Wobble")
  sub({
    id: 'wobble-bass', name: 'Wobble Bass', category: 'fx', vibe: 'experimental', gain: -1,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 20 }),
    oscB: osc('square', 0.6, { semi: -12 }),
    sub: { level: 0.5, wave: 'sine' },
    filter: { cutoff: 400, resonance: 5, keyTrack: 0.3, envAmount: 0, velocity: 0.2 },
    amp: { attack: 0.005, decay: 0.2, sustain: 1, release: 0.2, velocity: 0.4 },
    lfo: { rate: 2.5, filter: 3000 },
    polyphony: 8, // legacy slot was a PolySynth: keep chords (mono/glide sounds live in 24-27)
    character: { drive: 0.4 },
  }),
  // 34 (was "Sweep")
  sub({
    id: 'sweep-pad', name: 'Sweep Pad', category: 'fx', vibe: 'dreamy', polyphony: 8, gain: -0.5,
    oscA: osc('sawtooth', 1, { unison: 5, spread: 30 }),
    oscB: osc('sawtooth', 0.4, { semi: 7, unison: 3, spread: 20 }),
    noise: { level: 0.2, type: 'white', decay: 0 },
    filter: { cutoff: 300, resonance: 6, keyTrack: 0.3, envAmount: 5, velocity: 0.2 },
    filterEnv: { attack: 3.5, decay: 3, sustain: 0.5, release: 3 },
    amp: { attack: 1.5, decay: 1, sustain: 0.9, release: 3, velocity: 0.3 },
    lfo: { rate: 0.1, filter: 400 },
    character: { chorus: 0.4 },
  }),
  // 35 (was "Noise Hit")
  sub({
    id: 'laser-zap', name: 'Laser Zap', category: 'fx', vibe: 'experimental', gain: -5,
    oscA: osc('square', 1),
    oscB: osc('sawtooth', 0.5, { semi: 7 }),
    noise: { level: 0.5, type: 'white', decay: 0.05 },
    filter: { cutoff: 4000, resonance: 6, keyTrack: 0.3, envAmount: -2, velocity: 0.4 },
    filterEnv: { attack: 0.001, decay: 0.2, sustain: 0, release: 0.1 },
    amp: { attack: 0.001, decay: 0.25, sustain: 0, release: 0.1, velocity: 0.6 },
    pitchEnv: { amount: 24, decay: 0.18 },
  }),

  // ─── New sounds (36+) ───
  fm({
    id: 'wurli', name: 'Wurlitzer', category: 'keys', vibe: 'chill', gain: 6,
    description: 'Reedy EP with tremolo; bites when played hard',
    ratio: 1, index: 2.6, velocityToIndex: 0.9, keyScale: 0.5,
    modEnv: { attack: 0.001, decay: 0.5, sustain: 0.25, release: 0.2 },
    amp: { attack: 0.002, decay: 2.4, sustain: 0.1, release: 0.25, velocity: 0.75 },
    ping: { ratio: 7, level: 0.1, decay: 0.04 },
    character: { tremolo: 0.5, tremoloRate: 5.5, drive: 0.2 },
  }),
  sub({
    id: 'rock-organ', name: 'Rock Organ', category: 'keys', vibe: 'aggressive', polyphony: 16, gain: 5.5,
    oscA: osc('sine', 1, { partials: drawbars(1, 1, 0.9, 0.8, 0, 0.6, 0.7) }),
    sub: { level: 0.8, wave: 'sine' },
    noise: { level: 0.3, type: 'white', decay: 0.005 },
    filter: { cutoff: 10000, resonance: 0.3, rolloff: -12, keyTrack: 0, envAmount: 0, velocity: 0 },
    amp: { attack: 0.003, decay: 0.05, sustain: 1, release: 0.05, velocity: 0.1 },
    lfo: { rate: 6.8, vibrato: 6 },
    character: { drive: 0.45, chorus: 0.5, tremolo: 0.3, tremoloRate: 6.8 },
  }),
  sub({
    id: 'trance-lead', name: 'Supersaw Lead', category: 'lead', vibe: 'bright', polyphony: 6, gain: -1.5,
    oscA: osc('sawtooth', 1, { unison: 7, spread: 40 }),
    oscB: osc('sawtooth', 0.4, { semi: 12, unison: 5, spread: 25 }),
    filter: { cutoff: 5000, resonance: 1, keyTrack: 0.4, envAmount: 0.8, velocity: 0.4 },
    filterEnv: { attack: 0.005, decay: 0.4, sustain: 0.7, release: 0.3 },
    amp: { attack: 0.005, decay: 0.3, sustain: 0.9, release: 0.35, velocity: 0.4 },
    character: { chorus: 0.25 },
  }),
  sub({
    id: 'brass-section', name: 'Brass Section', category: 'brass', vibe: 'organic', polyphony: 10, gain: 3.5,
    oscA: osc('sawtooth', 1, { unison: 3, spread: 8 }),
    oscB: osc('sawtooth', 0.25, { semi: 12 }),
    noise: { level: 0.12, type: 'pink', decay: 0.08 },
    filter: { cutoff: 600, resonance: 1, keyTrack: 0.6, envAmount: 2.6, velocity: 0.8 },
    filterEnv: { attack: 0.09, decay: 0.6, sustain: 0.5, release: 0.25 },
    amp: { attack: 0.05, decay: 0.4, sustain: 0.85, release: 0.3, velocity: 0.7 },
    lfo: { rate: 5.2, vibrato: 10, delay: 0.5 },
    character: { chorus: 0.2 },
  }),
  fm({
    id: 'glass-pad', name: 'Glass Pad', category: 'pad', vibe: 'dreamy', polyphony: 10, gain: -7,
    ratio: 3, index: 1, velocityToIndex: 0.4, keyScale: 0.4,
    modEnv: { attack: 1.5, decay: 2, sustain: 0.6, release: 2 },
    amp: { attack: 0.9, decay: 1, sustain: 0.85, release: 2.5, velocity: 0.3 },
    lfo: { rate: 4.5, vibrato: 4 },
    character: { chorus: 0.5 },
  }),
];

/**
 * Synthesized piano used while the sampled piano is still decoding (and if
 * decoding ever fails). Not listed in the bank. Its gain is relative to the
 * SampledPiano it plays inside (sampler makeup + the piano preset's gain).
 */
export const FALLBACK_PIANO: FMPreset = fm({
  id: 'fallback-piano', name: 'Piano (synth)', category: 'keys', vibe: 'organic', gain: -11,
  ratio: 1, index: 1.4, velocityToIndex: 0.9, keyScale: 0.7,
  modEnv: { attack: 0.001, decay: 0.6, sustain: 0.05, release: 0.3 },
  amp: { attack: 0.001, decay: 3, sustain: 0, release: 0.4, velocity: 0.8 },
  ping: { ratio: 4, level: 0.12, decay: 0.03 },
  detune: 3,
});

export const PRESET_CATEGORIES: { id: PresetCategory; label: string }[] = [
  { id: 'keys', label: 'Keys' },
  { id: 'bass', label: 'Bass' },
  { id: 'lead', label: 'Lead' },
  { id: 'pad', label: 'Pad' },
  { id: 'pluck', label: 'Pluck' },
  { id: 'brass', label: 'Brass' },
  { id: 'bell', label: 'Bells & Mallets' },
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

/** Number of presets the previous engine shipped (indices 0..35 keep their role). */
export const LEGACY_PRESET_COUNT = 36;

const INDEX_BY_ID = new Map(SYNTH_PRESETS.map((p, i) => [p.id, i]));

export function getPresetIndexById(id: string): number {
  return INDEX_BY_ID.get(id) ?? -1;
}

export function getPresetById(id: string): SynthPreset | undefined {
  const i = INDEX_BY_ID.get(id);
  return i === undefined ? undefined : SYNTH_PRESETS[i];
}
