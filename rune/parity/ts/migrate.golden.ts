/**
 * Arrangement migration parity: every template through `loadFromTemplate` (fresh ids + migration)
 * and through `loadProject` (migration only), plus synthetic legacy projects through
 * `loadProject`. The output is `arrangementEngine.getArrangement()` right after the load, i.e.
 * `migrateArrangement(structuredClone(input))` as installed by `restoreFromSnapshot`.
 *
 * Ids from `crypto.randomUUID` are a deterministic `uuid-<n>` counter reset per case; the
 * `console.warn` messages (dropped legacy drum notes) are recorded too.
 */
import { it, vi } from 'vitest';
import { ALL_TEMPLATES } from '@/data/templates';
import { setPacksDirForTests } from '@/audio/sampler/factoryKit';
import { writeFixture } from './fixture';

vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());

const PACKS = '/u/packs';

const note = (n: number, startBeat: number, velocity: number, extra: Record<string, unknown> = {}) => ({
  note: n,
  velocity,
  startBeat,
  durationBeats: 0.25,
  ...extra,
});

const synth = (id: string, extra: Record<string, unknown> = {}) => ({
  id,
  name: `Synth ${id}`,
  instrument: { type: 'synth', presetIndex: 1 },
  regions: [{ id: `${id}-r1`, startBeat: 0, lengthBeats: 4, notes: [note(60, 0, 100), note(64, 1, 0.5), note(67, 2, 127, { foo: 'kept' }), note(72, 3, 1)] }],
  volume: -6,
  muted: false,
  solo: false,
  color: '#abc',
  ...extra,
});

const SYNTHETIC: [string, string | null, Record<string, unknown>][] = [
  [
    'velocities-no-pan-effects-no-beat-no-markers',
    null,
    { id: 'a1', name: 'Legacy', bpm: 100, lengthBeats: 2, tracks: [synth('s1'), synth('s2', { pan: null, effects: null })] },
  ],
  [
    'gm-drums-without-kit',
    null,
    {
      id: 'a2',
      name: 'GM',
      bpm: 90,
      lengthBeats: 32,
      tracks: [
        {
          id: 'd1',
          name: 'Old Drums',
          instrument: { type: 'drums', presetIndex: 0 },
          regions: [
            {
              id: 'r1',
              startBeat: 0,
              lengthBeats: 4,
              notes: [
                note(35, 0, 64, { isDrum: true }),
                note(36, 0, 127, { isDrum: true }),
                note(36, 0.00005, 0.5, { isDrum: true }),
                note(100, 1, 0.5, { isDrum: true }),
                note(42, 0.5, 90, { extra: [1, 2] }),
                note(44.4, 1.5, 0.3),
                note(81, 2, 0.3),
                note(38, 3, 0.9),
                note(40, 3, 0.95),
              ],
            },
            {
              id: 'r2',
              startBeat: 4,
              lengthBeats: 4,
              notes: [note(36, 0, 0.5)],
              audio: { sampleId: 's', samplePath: '/s.wav', offsetSeconds: 0, gainDb: 0, originalDuration: 1 },
            },
          ],
          volume: 0,
          pan: 0.25,
          muted: false,
          solo: false,
          color: '#f00',
        },
      ],
      loopMarkers: { startBeat: 4, endBeat: 12 },
    },
  ],
  [
    'kit-with-absolute-pack-paths-and-old-loop',
    PACKS,
    {
      id: 'a3',
      name: 'Packs',
      bpm: 120,
      tracks: [
        {
          id: 'd1',
          name: 'Kit',
          instrument: {
            type: 'drums',
            presetIndex: 0,
            kit: {
              name: 'Old Pack Kit',
              pads: [
                { sample: { source: { kind: 'file', path: `${PACKS}/tr808-fischer/bass-drum/bd0010.wav`, sampleId: 'stale' }, name: 'Kick' }, gainDb: 50 },
                { sample: { source: { kind: 'file', path: `${PACKS}\\p\\snare\\sd.wav` } } },
                { sample: { source: { kind: 'file', path: '/Users/me/clap.wav', sampleId: 'c1' }, name: 'Clap' }, mode: 'gate', endSec: 0.4 },
                { sample: { source: { kind: 'factory', voice: 'nope' } }, color: 'red' },
                null,
                { sample: { source: { kind: 'classic', machine: 'tr606', voice: 'zap' } } },
              ],
            },
          },
          regions: [{ id: 'r1', startBeat: 0, lengthBeats: 4, notes: [note(36, 0, 0.9, { isDrum: true }), note(51, 1, 200, { isDrum: true })] }],
          volume: 0,
          muted: false,
          solo: false,
          color: '#0f0',
        },
      ],
      beat: { activeDrumTrackId: 'gone', loop: { bars: 4, quantize: '1/8', swing: 0.4 }, selectedPad: 15.6 },
      loopMarkers: { startBeat: 8, endBeat: 8 },
    },
  ],
  [
    'two-drum-tracks-markers-groups',
    null,
    {
      id: 'a4',
      name: 'Two',
      bpm: 128,
      lengthBeats: 3,
      tracks: [
        { ...synth('s1'), pan: -0.5 },
        { id: 'd1', name: 'A', instrument: { type: 'drums', presetIndex: 0 }, regions: [], volume: 0, pan: 0, muted: false, solo: false, color: '#111' },
        { id: 'd2', name: 'B', instrument: { type: 'drums', presetIndex: 0, kit: { id: 'kit-b', name: 'B', pads: [] } }, regions: [], volume: 0, pan: 0, muted: true, solo: false, color: '#222' },
      ],
      beat: { activeDrumTrackId: 'd2', loop: { bars: 8, quantize: '1/16T', swing: 0.2, swingGrid: '1/8', countInBars: 0, metronome: false, overdub: false }, selectedPad: 9 },
      loopMarkers: { startBeat: 'x', endBeat: 16 },
      markers: [{ id: 'm1', name: 'Intro', beat: 0, color: '#fff' }],
      groups: [{ id: 'g1', name: 'G', color: '#333', trackIds: ['d1', 'zz', 's1'], collapsed: false }],
      custom: { kept: true },
    },
  ],
  [
    'valid-markers-long-arrangement',
    null,
    {
      id: 'a5',
      name: 'Valid',
      bpm: 80,
      lengthBeats: 200,
      tracks: [{ ...synth('s1'), automation: [{ parameter: 'volume', points: [{ beat: 0, value: 0.5 }], visible: true }] }],
      beat: null,
      loopMarkers: { startBeat: 2, endBeat: 6 },
    },
  ],
  [
    'string-length-and-quantize-1-8',
    null,
    { id: 'a6', name: 'Str', bpm: 120, lengthBeats: '100', tracks: [], beat: { loop: { quantize: '1/8', bars: 1 } }, loopMarkers: 5 },
  ],
];

/** Template-style inputs whose ids loadFromTemplate remaps (beat / markers / groups referencing tracks). */
const SYNTHETIC_TEMPLATES: [string, Record<string, unknown>][] = [
  ['remap-beat-groups', SYNTHETIC[3][2]],
  [
    'remap-missing-active',
    {
      id: 't',
      name: 'T',
      bpm: 120,
      lengthBeats: 16,
      tracks: [{ id: 'x', name: 'D', instrument: { type: 'drums', presetIndex: 0 }, regions: [{ id: 'r', startBeat: 0, lengthBeats: 4, notes: [note(38, 0, 100)] }], volume: 0, muted: false, solo: false, color: '#000' }],
      beat: { activeDrumTrackId: 'nope', selectedPad: 2 },
      groups: [{ id: 'g', name: 'G', color: '#000', trackIds: ['x', 'x'] }],
      markers: [],
    },
  ],
];

it('dumps migration vectors', async () => {
  let n = 0;
  vi.spyOn(crypto, 'randomUUID').mockImplementation(() => `uuid-${++n}` as `${string}-${string}-${string}-${string}-${string}`);
  const warnings: string[] = [];
  vi.spyOn(console, 'warn').mockImplementation((...args: unknown[]) => void warnings.push(args.map(String).join(' ')));
  const { arrangementEngine } = await import('@/services/ArrangementEngine');

  const run = (kind: 'template' | 'project', name: string, packsDir: string | null, input: unknown) => {
    setPacksDirForTests(packsDir);
    n = 0;
    warnings.length = 0;
    const arg = structuredClone(input) as never;
    if (kind === 'template') arrangementEngine.loadFromTemplate(arg);
    else arrangementEngine.loadProject(arg);
    const output = JSON.parse(JSON.stringify(arrangementEngine.getArrangement()));
    return { kind, name, packsDir, input, output, warnings: [...warnings] };
  };

  const cases = [];
  for (const t of ALL_TEMPLATES) {
    cases.push(run('template', t.id, null, t.arrangement));
    cases.push(run('project', t.id, null, t.arrangement));
  }
  for (const [name, dir, input] of SYNTHETIC) cases.push(run('project', name, dir, input));
  for (const [name, input] of SYNTHETIC_TEMPLATES) cases.push(run('template', name, null, input));
  setPacksDirForTests(undefined);
  writeFixture('migrate', { cases });
});
