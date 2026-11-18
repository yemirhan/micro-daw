/**
 * Export parity: `exportMidi` (the REAL midi-writer-js, byte output as base64) and
 * `getArrangementEndSeconds` (with injected pad buffer durations) for every template and a set of
 * synthetic projects. Every input goes through `arrangementEngine.loadProject` first (migration,
 * as the app exports `getArrangement()`), and the migrated arrangement is what is stored.
 */
import { it, vi } from 'vitest';
import { ALL_TEMPLATES } from '@/data/templates';
import type { PadSettings } from '@/types/sampler';
import { writeFixture } from './fixture';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

const n = (note: number, startBeat: number, velocity: number, durationBeats = 0.25, extra: Record<string, unknown> = {}) => ({
  note,
  velocity,
  startBeat,
  durationBeats,
  ...extra,
});

let trackSeq = 0;
const synth = (name: string, notes: unknown[], extra: Record<string, unknown> = {}, regionStart = 0) => ({
  id: `s${++trackSeq}`,
  name,
  instrument: { type: 'synth', presetIndex: 1 },
  regions: [{ id: `r${trackSeq}`, startBeat: regionStart, lengthBeats: 16, notes }],
  volume: 0,
  pan: 0,
  muted: false,
  solo: false,
  color: '#abc',
  ...extra,
});

const drums = (name: string, notes: unknown[], extra: Record<string, unknown> = {}, kit?: unknown) => ({
  id: `d${++trackSeq}`,
  name,
  instrument: kit === undefined ? { type: 'drums', presetIndex: 0 } : { type: 'drums', presetIndex: 0, kit },
  regions: [{ id: `r${trackSeq}`, startBeat: 0, lengthBeats: 16, notes }],
  volume: 0,
  pan: 0,
  muted: false,
  solo: false,
  color: '#f00',
  ...extra,
});

const audio = (name: string, extra: Record<string, unknown> = {}) => ({
  id: `a${++trackSeq}`,
  name,
  instrument: { type: 'audio', presetIndex: 0 },
  regions: [
    {
      id: `r${trackSeq}`,
      startBeat: 2,
      lengthBeats: 7.5,
      notes: [],
      audio: { sampleId: 'smp', samplePath: '/x.wav', offsetSeconds: 0, gainDb: 0, originalDuration: 4 },
    },
  ],
  volume: 0,
  pan: 0,
  muted: false,
  solo: false,
  color: '#0ff',
  ...extra,
});

const arr = (id: string, bpm: number, tracks: unknown[], extra: Record<string, unknown> = {}) => ({
  id,
  name: id,
  bpm,
  lengthBeats: 64,
  tracks,
  ...extra,
});

const pad = (p: number) => 36 + p;
const delay = (time: number, feedback: number, wet = 0.4, enabled = true) => ({
  reverb: { enabled: false, decay: 2, wet: 0.3 },
  delay: { enabled, time, feedback, wet },
  chorus: { enabled: false, frequency: 1.5, depth: 0.5, wet: 0.3 },
  distortion: { enabled: false, amount: 0.3, wet: 0.5 },
  eq: { enabled: false, low: 0, mid: 0, high: 0 },
  compressor: { enabled: false, threshold: -24, ratio: 4, attack: 0.003, release: 0.25 },
  filter: { enabled: false, type: 'lowpass', cutoff: 20000, resonance: 1 },
});

const customKit = {
  id: 'kit-c',
  name: 'Custom',
  pads: [
    { sample: { source: { kind: 'factory', voice: 'kick' }, name: 'Kick' } },
    { sample: { source: { kind: 'classic', machine: 'tr808', voice: 'sd' }, name: 'Snare 808' }, mode: 'gate', decay: 0.3 },
    { sample: { source: { kind: 'file', path: '/u/a.wav' }, name: 'File A' }, startSec: 0.2, endSec: 1.4, pitch: 7 },
    { sample: { source: { kind: 'file', path: '/u/b.wav' }, name: 'Long B' }, mode: 'gate', decay: 2, pitch: -12 },
    { sample: { source: { kind: 'file', path: '/u/c.wav' }, name: 'Tiny' }, startSec: 0.5, endSec: 0.5002 },
    { sample: { source: { kind: 'file', path: '/u/d.wav' }, name: 'Rev' }, reverse: true, startSec: 0.1 },
  ],
};

const SYNTHETIC: [string, Record<string, unknown>][] = [
  [
    'swing-16',
    arr('swing-16', 120, [drums('Drums', [0, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 3.75, 7.25].map((b, i) => n(pad(i % 4), b, 0.8, 0.25, { isDrum: true })))], {
      beat: { activeDrumTrackId: null, loop: { bars: 2, quantize: '1/16', swing: 0.5, swingGrid: '1/16' }, selectedPad: 0 },
    }),
  ],
  [
    'swing-8-grid',
    arr('swing-8-grid', 97, [drums('Drums', [0, 0.25, 0.5, 0.75, 1.5, 2.5, 3.5, 5.5].map((b, i) => n(pad(i % 6), b, 0.6, 0.1, { isDrum: true })))], {
      beat: { activeDrumTrackId: null, loop: { bars: 4, quantize: '1/8', swing: 0.66, swingGrid: '1/8' }, selectedPad: 0 },
    }),
  ],
  [
    'overlapping-and-chords',
    arr('overlapping-and-chords', 128, [
      synth('Chords', [
        n(60, 0, 0.8, 1),
        n(64, 0, 0.8, 1),
        n(67, 0, 0.8, 2),
        n(72, 1, 0.7, 4),
        n(48, 0.5, 0.9, 3),
        n(76, 1, 0.5, 0.5),
        n(79, 2, 0.5, 0.25),
        n(81, 1.5, 0.5, 0.5),
        n(60, 2, 0.3, 1),
        n(60, 2.5, 0.3, 1),
        n(62, 3, 0.4, 0.001),
        n(62, 3, 0.4, 0),
        n(55, 0.002, 0.4, 0.5),
      ]),
      synth('Late region', [n(50, 0, 0.5, 1), n(52, 0.5, 0.5, 0.5)], {}, 1000),
    ]),
  ],
  [
    'velocity-edges',
    arr('velocity-edges', 140, [
      synth(
        'Velocities',
        [0, 0.004, 0.005, 0.0051, 0.01, 0.1, 0.25, 0.3937, 0.5, 0.505, 0.75, 0.994, 0.995, 0.999, 1].map((v, i) => n(40 + i, i * 0.5, v, 0.5)),
      ),
    ]),
  ],
  [
    'muted-audio-mixed',
    arr('muted-audio-mixed', 90, [
      synth('Muted', [n(60, 1, 0.5)], { muted: true }),
      audio('Audio'),
      synth('Empty synth', []),
      drums('Kit drums', [n(pad(0), 1, 0.9, 0.25, { isDrum: true }), n(pad(1), 2, 0.9, 1, { isDrum: true }), n(pad(2), 3, 0.5, 0.5, { isDrum: true }), n(pad(3), 4, 0.5, 2, { isDrum: true }), n(pad(4), 5, 0.5, 0.5, { isDrum: true }), n(pad(5), 6, 0.5, 0.5, { isDrum: true }), n(pad(7), 6.5, 0.5, 0.5, { isDrum: true })], { effects: delay(0.375, 0.45) }, customKit),
      audio('Muted audio', { muted: true }),
    ]),
  ],
  [
    'single-track-format0',
    arr('single-track-format0', 93.7, [synth('Solo', [n(60, 0.5, 0.5, 0.5), n(62, 1.25, 0.5, 0.75)], { effects: delay(0.5, 0.995) })]),
  ],
  [
    'names-and-long-ticks',
    arr('names-and-long-ticks', 60, [
      synth('Çello – 🎻 ünïcödé', [n(36, 4, 0.5, 1)], { effects: delay(3, 0.9) }),
      synth('x'.repeat(200), [n(37, 2000, 0.5, 300), n(38, 20000, 0.5, 0.5)], { effects: delay(0.5, 0) }),
      synth('', [n(39, 1, 0.5, 1)], { effects: delay(0.25, 0.5, 0) }),
    ]),
  ],
  [
    'pitch-edges',
    arr('pitch-edges', 120, [synth('Pitches', [n(0, 1, 0.5), n(127, 1.5, 0.5), n(60.5, 2, 0.5), n(128, 2.5, 0.5), n(-1, 3, 0.5), n(44.4, 3.5, 0.5)])]),
  ],
  [
    'drums-default-kit-and-off-pad',
    arr('drums-default-kit-and-off-pad', 100, [
      drums('Drums', [n(pad(0), 0, 0.9, 0.25, { isDrum: true }), n(pad(1), 0, 0.9, 0.25, { isDrum: true }), n(pad(15), 0.5, 0.9, 0.25, { isDrum: true }), n(pad(9), 1, 0.4, 0.25, { isDrum: true }), n(pad(12), 1, 0.4, 0.25, { isDrum: true })]),
    ]),
  ],
  ['empty-no-tracks', arr('empty-no-tracks', 120, [])],
  ['empty-all-muted-or-audio', arr('empty-all-muted-or-audio', 120, [synth('M', [n(60, 0, 0.5)], { muted: true }), audio('A')])],
];

/** Deterministic injected buffer durations (s) per pad. */
const DURATIONS: [string, (pad: PadSettings) => number | null][] = [
  ['null', () => null],
  ['six', () => 6],
  ['by-name', (p) => (p.sample ? (p.sample.name.length % 5 === 0 ? null : (p.sample.name.length % 7) * 0.37 + 0.05) : null)],
  ['zero', () => 0],
];

it('dumps MIDI export and end-seconds vectors', async () => {
  const { arrangementEngine } = await import('@/services/ArrangementEngine');
  const { exportMidi, getArrangementEndSeconds } = await import('@/services/ExportService');
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);

  const inputs: [string, unknown][] = [
    ...ALL_TEMPLATES.map((t) => [`template:${t.id}`, t.arrangement] as [string, unknown]),
    ...SYNTHETIC,
  ];
  const cases = [];
  for (const [name, input] of inputs) {
    arrangementEngine.loadProject(structuredClone(input) as never);
    const arrangement = JSON.parse(JSON.stringify(arrangementEngine.getArrangement()));
    let midi: string | null = null;
    let error: string | null = null;
    try {
      const blob = await exportMidi(structuredClone(arrangement));
      midi = Buffer.from(await blob.arrayBuffer()).toString('base64');
    } catch (e) {
      error = (e as Error).message;
    }
    const endSeconds = Object.fromEntries(DURATIONS.map(([k, f]) => [k, getArrangementEndSeconds(structuredClone(arrangement), f)]));
    cases.push({ name, arrangement, midi, error, endSeconds });
  }
  writeFixture('midi_export', { cases });
});
