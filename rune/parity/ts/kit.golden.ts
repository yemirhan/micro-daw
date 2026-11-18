/**
 * Kit parity vectors: normalizePad / normalizeKit over malformed and edge inputs (with and
 * without a packs folder), createFactoryKit, createClassicKit for every machine, GM drum note
 * mapping, migrateGmDrumNotes / migrateDrumTrack / drumNoteToGm, sourceFallbackName.
 * Kit ids drawn by normalizeKit come from a deterministic `uuid-<n>` counter (reset per case).
 */
import { it, vi } from 'vitest';
import {
  createFactoryKit,
  factoryPadForGmNote,
  gmNoteToFactoryVoice,
  normalizeKit,
  normalizePad,
  setPacksDirForTests,
  sourceFallbackName,
} from '@/audio/sampler/factoryKit';
import { createClassicKit } from '@/audio/sampler/classic/kit';
import { CLASSIC_MACHINES } from '@/audio/sampler/classic/machines';
import { drumNoteToGm, migrateDrumTrack, migrateGmDrumNotes } from '@/utils/arrangementHelpers';
import type { KitState, PadSampleSource } from '@/types/sampler';
import type { RegionNote, Track } from '@/types/arrangement';
import { writeFixture } from './fixture';

const PACKS = '/u/packs';

let n = 0;
function resetIds() {
  n = 0;
}

const PAD_INPUTS: unknown[] = [
  null,
  42,
  'pad',
  true,
  [],
  [1, 2],
  {},
  { gainDb: 99, pan: -5, pitch: 40, decay: 0, filterCutoff: 1e6, mode: 'gate', chokeGroup: 2.4 },
  { gainDb: -99, pan: 5, pitch: -40, decay: 99, filterCutoff: 1, mode: 'GATE', chokeGroup: -3 },
  { gainDb: '3', pan: null, pitch: true, decay: [0.1], filterCutoff: {}, reverse: 'true', chokeGroup: '2' },
  { startSec: -1, endSec: 2 },
  { startSec: 1, endSec: 0.5 },
  { startSec: 1, endSec: 1 },
  { startSec: 1, endSec: 1.0001 },
  { startSec: '1', endSec: '2' },
  { endSec: 0 },
  { endSec: 1e300 },
  { reverse: true, mode: 'oneshot', chokeGroup: 0.5 },
  { reverse: 1, chokeGroup: 1.5 },
  { chokeGroup: -0.5 },
  { chokeGroup: 2.5 },
  { color: '' },
  { color: 'red' },
  { color: 7 },
  { decay: 0.005, filterCutoff: 20, gainDb: 12, pan: -1, pitch: 24 },
  { sample: null },
  { sample: 'kick' },
  { sample: {} },
  { sample: { name: 'x' } },
  { sample: { source: null, name: 'x' } },
  { sample: { source: { kind: 'factory', voice: 'kick' } } },
  { sample: { source: { kind: 'factory', voice: 'perc' }, name: '' } },
  { sample: { source: { kind: 'factory', voice: 'kick-sub' }, name: 'Boom' } },
  { sample: { source: { kind: 'factory', voice: 'nope' }, name: 'x' } },
  { sample: { source: { kind: 'factory', voice: 3 }, name: 'x' } },
  { sample: { source: { kind: 'factory' }, name: 'x' } },
  { sample: { source: { kind: 'classic', machine: 'tr909', voice: 'clap' } } },
  { sample: { source: { kind: 'classic', machine: 'cr78', voice: 'metal' }, name: 7 } },
  { sample: { source: { kind: 'classic', machine: 'tr999', voice: 'kick' }, name: 'x' } },
  { sample: { source: { kind: 'classic', machine: 'tr808', voice: 'nope' }, name: 'x' } },
  { sample: { source: { kind: 'classic', machine: 'tr808', voice: 42 }, name: 'x' } },
  { sample: { source: { kind: 'pack', packId: 'tr808-fischer', file: 'bass-drum/bd0010.wav' }, name: 'Kick' } },
  { sample: { source: { kind: 'pack', packId: 'p', file: 'kicks/bd.wav' } } },
  { sample: { source: { kind: 'pack', packId: 'p', file: 'kicks/.wav' } } },
  { sample: { source: { kind: 'pack', packId: '../x', file: 'a.wav' } } },
  { sample: { source: { kind: 'pack', packId: 'p', file: '../../a.wav' } } },
  { sample: { source: { kind: 'pack', packId: 'p', file: 'a.exe' } } },
  { sample: { source: { kind: 'pack', packId: 'p', file: 'con.wav' } } },
  { sample: { source: { kind: 'pack', packId: 'P', file: 'a.wav' } } },
  { sample: { source: { kind: 'file', path: '/a/b.wav' }, name: 'x' } },
  { sample: { source: { kind: 'file', path: '/a/b.wav', sampleId: 'abc' } } },
  { sample: { source: { kind: 'file', path: '/a/b.wav', sampleId: 5 } } },
  { sample: { source: { kind: 'file', path: '' }, name: 'x' } },
  { sample: { source: { kind: 'file', path: 9 }, name: 'x' } },
  { sample: { source: { kind: 'file', path: 'C:\\s\\Snare 2.aif' } } },
  { sample: { source: { kind: 'file', path: '/x/.hidden' } } },
  { sample: { source: { kind: 'file', path: '/' } } },
  { sample: { source: { kind: 'file', path: `${PACKS}/tr808-fischer/bass-drum/bd0010.wav`, sampleId: 'stale' }, name: 'Kick' } },
  { sample: { source: { kind: 'file', path: `${PACKS}/tr808-fischer/bass-drum/bd0010.wav` } } },
  { sample: { source: { kind: 'file', path: `${PACKS}/p` } } },
  { sample: { source: { kind: 'file', path: `${PACKS}/.partial-p/a.wav` } } },
  { sample: { source: { kind: 'file', path: `${PACKS}/p/../q/a.wav` } } },
  { sample: { source: { kind: 'file', path: `${PACKS}-other/p/a.wav` } } },
  { sample: { source: { kind: 'file', path: `${PACKS}\\p\\k\\a.wav` } } },
  { sample: { source: { kind: 'other', path: '/a.wav' }, name: 'x' } },
  { sample: { source: { kind: 'file', path: '/a.wav' }, name: 'x' }, extra: 1, gainDb: -6.5, color: 'oklch(0.6 0.2 30)' },
];

const KIT_INPUTS: unknown[] = [
  null,
  'kit',
  3,
  [],
  {},
  { id: '', name: '' },
  { id: 5, name: 7, pads: 'x' },
  { id: 'k', name: 'Mine', pads: [] },
  { id: 'k', name: 'K', pads: PAD_INPUTS.slice(0, 20) },
  { id: 'k2', pads: PAD_INPUTS.slice(20, 45) },
  { name: 'NoId', pads: PAD_INPUTS.slice(45) },
  { id: 'k3', name: 'K3', pads: { 0: { gainDb: 3 } } },
  JSON.parse(JSON.stringify(createFactoryKit())),
];

const hit = (note: number, startBeat: number, velocity = 0.8, extra: Record<string, unknown> = {}): RegionNote =>
  ({ note, startBeat, velocity, durationBeats: 0.25, isDrum: true, ...extra }) as RegionNote;

const GM_NOTE_SETS: RegionNote[][] = [
  [],
  [hit(36, 0), hit(38, 1), hit(42, 0.5), hit(46, 1.5), hit(49, 0), hit(51, 2), hit(39, 3), hit(100, 0)],
  [hit(35, 0, 0.4), hit(36, 0, 0.7)],
  [hit(36, 0, 0.9), hit(35, 0.00005, 0.3), hit(35, 0.0002, 0.95)],
  [hit(40, 1, 0.5), hit(38, 1, 0.5), hit(38.4, 2), hit(37.5, 3), hit(34.5, 4), hit(-1, 0), hit(127, 0)],
  [hit(82, 0, 0.2, { isDrum: false }), hit(70, 0, 0.6), hit(69, 0.5), hit(54, 0.5, 1), hit(77, 7.75), hit(76, 7.75, 0.1), hit(75, 1)],
  Array.from({ length: 60 }, (_, i) => hit(30 + i, (i % 8) * 0.25, ((i * 37) % 100) / 100)),
];

const drumTrack = (notes: RegionNote[], kit?: unknown, audio = false): Track =>
  ({
    id: 't1',
    name: 'Drums',
    instrument: { type: 'drums', presetIndex: 0, ...(kit !== undefined ? { kit } : {}) },
    regions: [
      { id: 'r1', startBeat: 0, lengthBeats: 4, notes },
      ...(audio
        ? [{ id: 'r2', startBeat: 4, lengthBeats: 4, notes: [hit(36, 0)], audio: { sampleId: 's', samplePath: '/s.wav', offsetSeconds: 0, gainDb: 0, originalDuration: 1 } }]
        : []),
    ],
    volume: 0,
    pan: 0,
    muted: false,
    solo: false,
    color: '#fff',
  }) as Track;

it('dumps kit vectors', () => {
  vi.spyOn(crypto, 'randomUUID').mockImplementation(() => `uuid-${++n}` as `${string}-${string}-${string}-${string}-${string}`);
  const pads: unknown[] = [];
  const kits: unknown[] = [];
  for (const dir of [null, PACKS]) {
    setPacksDirForTests(dir);
    for (const p of PAD_INPUTS) pads.push([p, dir, normalizePad(p)]);
    for (const k of KIT_INPUTS) {
      resetIds();
      kits.push([k, dir, normalizeKit(k)]);
    }
  }
  setPacksDirForTests(null);

  const factory = createFactoryKit();
  const classic = CLASSIC_MACHINES.map((m) => [m.id, createClassicKit(m.id)]);

  const gmVoice: unknown[] = [];
  for (let x = 20; x <= 100; x += 0.25) gmVoice.push([x, gmNoteToFactoryVoice(x), factoryPadForGmNote(x)]);

  const gmNotes = GM_NOTE_SETS.map((notes) => [notes, migrateGmDrumNotes(structuredClone(notes))]);

  const tracks: unknown[] = [];
  const trackInputs: [Track, string | null][] = [
    ...GM_NOTE_SETS.map((s) => [drumTrack(s), null] as [Track, string | null]),
    [drumTrack(GM_NOTE_SETS[1], undefined, true), null],
    [drumTrack(GM_NOTE_SETS[1], createFactoryKit()), null],
    [drumTrack(GM_NOTE_SETS[2], { name: 'Partial', pads: [{ gainDb: 40 }] }), null],
    [drumTrack(GM_NOTE_SETS[2], 'truthy-kit'), null],
    [drumTrack(GM_NOTE_SETS[2], null), null],
    [drumTrack(GM_NOTE_SETS[2], { id: 'k', pads: [{ sample: { source: { kind: 'file', path: `${PACKS}/p/a.wav` } } }] }), PACKS],
    [{ ...drumTrack(GM_NOTE_SETS[1]), instrument: { type: 'synth', presetIndex: 2 } } as Track, null],
  ];
  for (const [t, dir] of trackInputs) {
    resetIds();
    setPacksDirForTests(dir);
    const track = structuredClone(t);
    const dropped = migrateDrumTrack(track);
    tracks.push([t, dir, track, dropped]);
  }
  setPacksDirForTests(null);

  const gmExport: unknown[] = [];
  const exportKits: [string, KitState | undefined][] = [
    ['none', undefined],
    ['factory', factory],
    ...CLASSIC_MACHINES.map((m) => [m.id, createClassicKit(m.id)] as [string, KitState]),
  ];
  for (const [name, kit] of exportKits) {
    for (let note = 30; note <= 56; note += 0.5) gmExport.push([name, note, drumNoteToGm(note, kit)]);
  }

  const sources: PadSampleSource[] = [
    { kind: 'factory', voice: 'perc' },
    { kind: 'classic', machine: 'tr808', voice: 'kick' },
    { kind: 'classic', machine: 'linn', voice: 'unknown-voice' },
    { kind: 'pack', packId: 'p', file: 'kicks/Kick 01.wav' },
    { kind: 'pack', packId: 'p', file: 'a/b/.wav' },
    { kind: 'file', path: '/x/y/' },
    { kind: 'file', path: '' },
    { kind: 'file', path: 'C:\\a\\b.c.d' },
  ];
  const fallbackNames = sources.map((s) => [s, sourceFallbackName(s)]);

  writeFixture('kit', { pads, kits, factory, classic, gmVoice, gmNotes, tracks, gmExport, fallbackNames });
});
