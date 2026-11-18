/**
 * Chromium reference capture for the WAV export: the app's real `ExportService.exportWav`
 * (Tone.Offline, the real per-track chain, master bus, SamplerKit, Tone.Part scheduling with
 * swing and gate releases, 16-bit WAV encoder) at 48 kHz, for drum-only projects (factory kit,
 * classic kits, swing on the 1/16 and 1/8 grids incl. fractional ticks, gate pads, choke groups,
 * pitch / reverse / pan, a muted track, a soloed track — solo is ignored by the export), and
 * projects with synth tracks (phase 4): the "Ambient Pad" and "EDM Drop" templates as the app
 * loads them (`loadFromTemplate`: the migrated outputs of rune/parity/fixtures/migrate.json),
 * a keys project (FM with overrides, rhodes, a pluck harp, the sampled grand piano with
 * re-strikes, a mono legato bass) and an FX project (filter LFO, chorus, reverb, delay,
 * distortion, EQ and compressor on synth and drum tracks).
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-export.steps.mjs \
 *        --out /tmp/parity-export.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each WAV (<id>.wav).
 *
 * Page side: ./capture-export-page.ts (on ./blank.html, context forced to 48 kHz, seeded
 * Math.random for the factory voices' noise exactly like capture-factory.steps.mjs).
 * Output: rune/parity/fixtures/export_reference.json — per project the arrangement JSON (the
 * Rust test exports the same one), the WAV length / header fields, the SamplerKit trigger count
 * and the duplicated triggers, and per channel the full-length block evidence of ./stats.mjs over
 * the decoded 16-bit samples (x = int16 / 32768), for the app as-is (`evidence`) and for a second
 * export where a repeated (kit, pad, time) trigger is skipped (`evidenceDeduped`): Tone's offline
 * clock hands some Part events out twice (see capture-export-page.ts); synth
 * `triggerAttackRelease` calls are logged and de-duplicated the same way. Synth noise: Tone's
 * noise buffers from SYNTH_NOISE_SEEDS and `Math.random` seeded with SYNTH_SEED before each
 * export (the Rust `DEFAULT_NOISE_SEED`; exact for projects with one noise-drawing instrument).
 * Rust: rune/crates/mdaw-engine/src/engine/tests_export.rs.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-export-page.ts')";
const SR = 48000;
const NOISE_SEED = 0x5eed0001;
const OFFSET_SEED = 0x5eed1000;
const VOICES = [
  'kick', 'kick-sub', 'snare', 'rim', 'clap', 'chh', 'phh', 'ohh',
  'tom-lo', 'tom-mid', 'tom-hi', 'crash', 'ride', 'shaker', 'perc', 'snap',
];
/** Tone's white / pink / brown noise buffers (capture-synth.steps.mjs NOISE_SEEDS = Rust `NoiseSeeds::default()`). */
const SYNTH_NOISE_SEEDS = { white: 0x5eed_2001, pink: 0x5eed_2002, brown: 0x5eed_2003 };
/** Noise start offsets of the synth instruments (Rust `DEFAULT_NOISE_SEED`). */
const SYNTH_SEED = 0x5eed_3000;
const f10 = (x) => Number(x.toPrecision(10));

function initScript() {
  const q = new URLSearchParams(location.search);
  const sr = Number(q.get('paritySr'));
  if (sr > 0) {
    for (const name of ['AudioContext', 'webkitAudioContext']) {
      const Native = window[name];
      if (!Native) continue;
      window[name] = class extends Native {
        constructor(opts = {}) {
          super({ ...opts, sampleRate: sr });
        }
      };
    }
  }
  let calls = 0;
  window.__paritySeed = (seed) => {
    let a = seed | 0;
    calls = 0;
    Math.random = () => {
      calls++;
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  };
  window.__parityRandomCalls = () => calls;
}

const DEF = { sample: null, gainDb: 0, pan: 0, pitch: 0, startSec: 0, endSec: null, decay: 0.05, filterCutoff: 20000, reverse: false, mode: 'oneshot', chokeGroup: 0 };

function classicKit(id, machine, voices) {
  const pads = Array.from({ length: 16 }, () => ({ ...DEF }));
  voices.forEach(([pad, voice, extra = {}]) => {
    pads[pad] = { ...DEF, sample: { name: voice, source: { kind: 'classic', machine, voice } }, ...extra };
  });
  return { id, name: id, pads };
}

const n = (pad, startBeat, velocity = 0.8, durationBeats = 0.25) => ({ note: 36 + pad, velocity, startBeat, durationBeats, isDrum: true });

function track(id, kit, notes, extra = {}) {
  return {
    id,
    name: id,
    instrument: { type: 'drums', presetIndex: 0, kit },
    regions: [{ id: `${id}-r`, startBeat: 0, lengthBeats: 8, notes }],
    volume: -6,
    pan: 0,
    muted: false,
    solo: false,
    color: '#f00',
    ...extra,
  };
}

function project(id, bpm, tracks, loop) {
  return {
    id,
    name: id,
    bpm,
    lengthBeats: 16,
    tracks,
    loopMarkers: { startBeat: 0, endBeat: 8 },
    beat: {
      activeDrumTrackId: tracks[0].id,
      selectedPad: 0,
      loop: { bars: 2, quantize: '1/16', swing: 0, swingGrid: '1/16', countInBars: 1, metronome: true, overdub: true, ...loop },
    },
  };
}

const sn = (note, startBeat, durationBeats, velocity = 0.8) => ({ note, velocity, startBeat, durationBeats });

function synthTrack(id, presetIndex, notes, extra = {}) {
  return {
    id,
    name: id,
    instrument: { type: 'synth', presetIndex },
    regions: [{ id: `${id}-r`, startBeat: 0, lengthBeats: 16, notes }],
    volume: -6,
    pan: 0,
    muted: false,
    solo: false,
    color: '#0af',
    ...extra,
  };
}

const FX_OFF = {
  reverb: { enabled: false, wet: 0.25 },
  delay: { enabled: false, time: 0.25, feedback: 0.3, wet: 0.3 },
  chorus: { enabled: false, depth: 0.5 },
  distortion: { enabled: false, amount: 0.3, wet: 0.5 },
  eq: { enabled: false, low: 0, mid: 0, high: 0 },
  compressor: { enabled: false, threshold: -24, ratio: 4, attack: 0.003, release: 0.25 },
  filter: { enabled: false, type: 'lowpass', cutoff: 18000, resonance: 1, lfoRate: 1, lfoDepth: 0, lfoWave: 'sine' },
};
const fx = (over) => Object.fromEntries(Object.entries(FX_OFF).map(([k, v]) => [k, { ...v, ...(over[k] ?? {}) }]));

/** The templates as `loadFromTemplate` installs them (migrate golden: fresh ids + migration). */
function templates() {
  const golden = JSON.parse(readFileSync(path.join(FIXTURES, 'migrate.json'), 'utf8'));
  const pick = (name) => {
    const c = golden.cases.find((x) => x.kind === 'template' && x.name === name && x.packsDir === null);
    if (!c) throw new Error(`template ${name} not in migrate.json`);
    return { ...structuredClone(c.output), id: `tpl_${name.replace('-', '_')}` };
  };
  return [pick('ambient-pad'), pick('edm-drop')];
}

function synthProjects(factory) {
  // keys_piano: FM keys with overrides, rhodes, the sampled grand piano (re-strike, overlaps),
  // a mono legato bass with overlapping notes
  const chords = [];
  [[0, [60, 64, 67], 1.5], [2, [62, 65, 69], 1], [3.5, [64, 67, 71], 2], [6, [60, 65, 69], 1.75]].forEach(([b, ns, d], i) =>
    ns.forEach((n, k) => chords.push(sn(n, b, d, 0.55 + 0.1 * k + 0.05 * i))),
  );
  const rhodes = [sn(72, 0.5, 0.5, 0.7), sn(76, 1.25, 0.75, 0.5), sn(79, 4, 1, 0.9), sn(74, 5.5, 0.25, 0.4), sn(77, 6.5, 1.5, 0.75)];
  const harp = [];
  for (let i = 0; i < 16; i++) harp.push(sn([55, 59, 62, 67][i % 4] + (i >= 8 ? 2 : 0), i * 0.5, 0.4, 0.5 + 0.03 * (i % 5)));
  const piano = [sn(48, 0, 2, 0.8), sn(52, 1, 1, 0.6), sn(55, 2, 1.5, 0.7), sn(48, 2, 0.5, 0.9), sn(60, 3, 0.25, 1), sn(84, 4, 1, 0.5), sn(36, 5, 2.5, 0.85), sn(71, 6.75, 0.75, 0.3)];
  const bass = [sn(33, 0, 1.25, 0.9), sn(36, 1, 1.25, 0.8), sn(38, 2, 0.5, 0.8), sn(40, 3, 1.5, 0.7), sn(41, 4.25, 0.25, 0.9), sn(43, 5, 2, 0.8)];
  const keys = {
    id: 'keys_piano',
    name: 'keys_piano',
    bpm: 112,
    lengthBeats: 16,
    tracks: [
      synthTrack('fm', 2, chords, { volume: -8, pan: -0.3, instrument: { type: 'synth', presetIndex: 2, synthOverrides: { index: 2.4, gain: -1, character: { chorus: 0.2 } } } }),
      synthTrack('rhodes', 13, rhodes, { volume: -10, pan: 0.25 }),
      synthTrack('piano', 0, piano, { volume: -4 }),
      synthTrack('bass', 24, bass, { volume: -9 }),
    ],
    loopMarkers: { startBeat: 0, endBeat: 8 },
  };

  // fx_tracks: track FX on synth and drum tracks
  const kd = [];
  for (let b = 0; b < 8; b++) {
    kd.push(n(0, b, b % 2 ? 0.8 : 1), n(5, b + 0.5, 0.5));
    if (b % 2 === 1) kd.push(n(1, b, 0.9));
  }
  const pad = [sn(57, 0, 4, 0.7), sn(60, 0, 4, 0.6), sn(64, 0, 4, 0.6), sn(55, 4, 4, 0.7), sn(59, 4, 4, 0.6), sn(62, 4, 4, 0.6)];
  const lead = [sn(69, 0.5, 0.5, 0.9), sn(72, 1, 0.25, 0.7), sn(74, 1.5, 1, 0.8), sn(76, 3, 0.5, 0.9), sn(74, 3.75, 0.25, 0.6), sn(72, 4.5, 1.5, 0.8), sn(67, 6.5, 0.75, 0.7)];
  const fxProject = {
    id: 'fx_tracks',
    name: 'fx_tracks',
    bpm: 100,
    lengthBeats: 16,
    tracks: [
      synthTrack('pad', 3, pad, {
        volume: -8,
        effects: fx({ filter: { enabled: true, type: 'lowpass', cutoff: 2400, resonance: 3, lfoRate: 0.7, lfoDepth: 0.6, lfoWave: 'sine' }, chorus: { enabled: true, depth: 0.6 }, reverb: { enabled: true, wet: 0.35 } }),
      }),
      synthTrack('lead', 10, lead, {
        volume: -10,
        pan: 0.2,
        effects: fx({
          distortion: { enabled: true, amount: 0.4, wet: 0.5 },
          eq: { enabled: true, low: 3, mid: -2, high: 4 },
          delay: { enabled: true, time: 0.3, feedback: 0.4, wet: 0.3 },
          compressor: { enabled: true, threshold: -20, ratio: 4, attack: 0.005, release: 0.2 },
          filter: { enabled: true, type: 'bandpass', cutoff: 1800, resonance: 1.5, lfoRate: 3, lfoDepth: 0.4, lfoWave: 'triangle' },
        }),
      }),
      track('drums', structuredClone(factory), kd, {
        volume: -6,
        effects: fx({
          reverb: { enabled: true, wet: 0.2 },
          delay: { enabled: true, time: 0.1875, feedback: 0.3, wet: 0.25 },
          compressor: { enabled: true, threshold: -18, ratio: 6, attack: 0.003, release: 0.15 },
          filter: { enabled: true, type: 'highpass', cutoff: 120, resonance: 1, lfoRate: 2, lfoDepth: 0.3, lfoWave: 'square' },
          chorus: { enabled: true, depth: 0.3 },
        }),
      }),
    ],
    loopMarkers: { startBeat: 0, endBeat: 8 },
    beat: { activeDrumTrackId: 'drums', selectedPad: 0, loop: { bars: 2, quantize: '1/16', swing: 0, swingGrid: '1/16', countInBars: 1, metronome: true, overdub: true } },
  };
  // pluck_harp: the pluck voices in Tone.Offline (the only noise user: offsets from SYNTH_SEED;
  // beat 7.5 is fired a transport tick early, a fresh A4 voice's comb params are swapped in a
  // hair after its first quantum); saw_lead: saw-lead (sub oscillator + fat stack built at
  // Tone's clock time). See docs/rune-port/phase-4-instruments.md.
  const pluck = {
    id: 'pluck_harp',
    name: 'pluck_harp',
    bpm: 112,
    lengthBeats: 16,
    tracks: [synthTrack('harp', 29, harp, { volume: -7, pan: 0.4 })],
    loopMarkers: { startBeat: 0, endBeat: 8 },
  };
  const sawLead = {
    id: 'saw_lead',
    name: 'saw_lead',
    bpm: 100,
    lengthBeats: 16,
    tracks: [synthTrack('lead', 1, lead, { volume: -10 })],
    loopMarkers: { startBeat: 0, endBeat: 8 },
  };
  return [...templates(), keys, fxProject, pluck, sawLead, ...debugProjects(), ...soloProjects([keys, fxProject])];
}

/** PARITY_EXPORT_DEBUG=1: every track of `projects` alone (`<id>__<track>`). */
function soloProjects(projects) {
  if (!process.env.PARITY_EXPORT_DEBUG) return [];
  return projects.flatMap((p) => p.tracks.map((t) => ({ ...structuredClone(p), id: `${p.id}__${t.id}`, tracks: [structuredClone(t)] })));
}

function leadProbes() {
  const lead = [sn(69, 0.5, 0.5, 0.9), sn(72, 1, 0.25, 0.7), sn(74, 1.5, 1, 0.8), sn(76, 3, 0.5, 0.9), sn(74, 3.75, 0.25, 0.6), sn(72, 4.5, 1.5, 0.8), sn(67, 6.5, 0.75, 0.7)];
  const all = {
    distortion: { enabled: true, amount: 0.4, wet: 0.5 },
    eq: { enabled: true, low: 3, mid: -2, high: 4 },
    delay: { enabled: true, time: 0.3, feedback: 0.4, wet: 0.3 },
    compressor: { enabled: true, threshold: -20, ratio: 4, attack: 0.005, release: 0.2 },
    filter: { enabled: true, type: 'bandpass', cutoff: 1800, resonance: 1.5, lfoRate: 3, lfoDepth: 0.4, lfoWave: 'triangle' },
  };
  const mk = (id, fxs) => ({ id, name: id, bpm: 100, lengthBeats: 16, tracks: [synthTrack('lead', 1, lead, { volume: -10, pan: 0.2, effects: fx(fxs) })], loopMarkers: { startBeat: 0, endBeat: 8 } });
  const one = (id, notes, overrides) => ({ id, name: id, bpm: 100, lengthBeats: 16, tracks: [synthTrack('lead', 1, notes, { volume: -10, instrument: { type: 'synth', presetIndex: 1, ...(overrides ? { synthOverrides: overrides } : {}) } })], loopMarkers: { startBeat: 0, endBeat: 8 } });
  return [
    mk('dbg_lead_dry', {}),
    ...Object.keys(all).map((k) => mk(`dbg_lead_${k}`, { [k]: all[k] })),
    one('dbg_lead_one', [sn(69, 0.5, 0.5, 0.9)]),
    one('dbg_lead_novib', [sn(69, 0.5, 0.5, 0.9)], { lfo: { vibrato: 0 } }),
    one('dbg_lead_nofine', [sn(69, 0.5, 0.5, 0.9)], { oscB: { fine: 0 } }),
    // each oscillator alone: the sub (detune −1200 written at Tone's clock time) was the 4–8 kHz error
    one('dbg_lead_a', [sn(69, 0.5, 0.5, 0.9)], { oscB: { level: 0 }, sub: { level: 0 } }),
    one('dbg_lead_b', [sn(69, 0.5, 0.5, 0.9)], { oscA: { level: 0 }, sub: { level: 0 } }),
    one('dbg_lead_sub', [sn(69, 0.5, 0.5, 0.9)], { oscA: { level: 0 }, oscB: { level: 0 } }),
  ];
}

/** PARITY_EXPORT_DEBUG=1: small single-track probes (not part of the committed fixture). */
function debugProjects() {
  if (!process.env.PARITY_EXPORT_DEBUG) return [];
  const one = (id, preset, notes, bpm = 70) => ({ id, name: id, bpm, lengthBeats: 32, tracks: [synthTrack('s', preset, notes, { volume: 0 })], loopMarkers: { startBeat: 0, endBeat: 8 } });
  return [
    one('dbg_warm_two', 3, [sn(72, 18, 4, 0.315), sn(71, 22, 2, 0.276)]),
    one('dbg_warm_one', 3, [sn(71, 22, 2, 0.276)]),
    one('dbg_warm_gap', 3, [sn(72, 18, 2, 0.315), sn(71, 22, 2, 0.276)]),
    ...leadProbes(),
    one('dbg_harp_one', 29, [sn(55, 0, 0.4, 0.5)], 112),
    one('dbg_harp_late', 29, [sn(55, 1, 0.4, 0.5)], 112),
    one('dbg_harp_two', 29, [sn(55, 0, 0.4, 0.5), sn(59, 1, 0.4, 0.53)], 112),
  ];
}

function projects(factory) {
  // factory_basic: kick every beat, snare 2/4, closed hats every 8th, open hat (choke) on the &s of 4
  const fk = structuredClone(factory);
  const basic = [];
  for (let b = 0; b < 8; b++) {
    basic.push(n(0, b, b % 4 === 0 ? 1 : 0.85));
    if (b % 2 === 1) basic.push(n(2, b, 0.9));
    basic.push(n(5, b, 0.6), n(5, b + 0.5, 0.45));
  }
  basic.push(n(7, 3.5, 0.7), n(7, 7.5, 0.7), n(4, 7.75, 0.5));

  // classic_swing: TR-808, swing 0.5 on 1/16, gate clap, hats choke, pitch / reverse / pan
  const k808 = classicKit('k808', 'tr808', [
    [0, 'kick', { gainDb: -2 }],
    [1, 'snare', { pan: -0.3 }],
    [2, 'chh', { chokeGroup: 1, pan: 0.5, gainDb: -4 }],
    [3, 'ohh', { chokeGroup: 1, decay: 0.3 }],
    [4, 'clap', { mode: 'gate', decay: 0.08 }],
    [5, 'tom-lo', { pitch: -3, pan: 0.4 }],
    [6, 'rim', { reverse: true, gainDb: -6 }],
    [7, 'cowbell', { filterCutoff: 1800, startSec: 0.01 }],
  ]);
  const swing = [];
  for (let s = 0; s < 32; s++) swing.push(n(2, s * 0.25, s % 2 ? 0.55 : 0.8));
  swing.push(n(0, 0, 1), n(0, 2.75, 0.9), n(0, 4, 1), n(0, 6.5, 0.9));
  swing.push(n(1, 1, 0.9), n(1, 3, 0.9), n(1, 5, 0.9), n(1, 7.25, 0.6));
  swing.push(n(3, 3.5, 0.8), n(4, 2, 0.8, 0.5), n(4, 6, 0.8, 1.25), n(5, 5.75, 0.7), n(6, 4.5, 0.8), n(7, 1.5, 0.6), n(7, 6.25, 0.6));

  // two_tracks: swing 0.3 on 1/8 (fractional ticks), factory + 909 (soloed: ignored), a muted
  // third track (skipped), a long crash, a gate pad, track pans / volumes
  const fk2 = structuredClone(factory);
  fk2.pads[1] = { ...fk2.pads[1], mode: 'gate', decay: 0.12 };
  fk2.pads[11] = { ...fk2.pads[11], gainDb: -3 };
  const a = [n(0, 0, 1), n(0, 1.5, 0.8), n(0, 4, 1), n(0, 5.5, 0.8), n(1, 2, 0.9, 0.75), n(1, 6, 0.9, 0.3), n(11, 0, 0.7)];
  for (let e = 0; e < 16; e++) a.push(n(6, e * 0.5, e % 2 ? 0.5 : 0.75));
  const k909 = classicKit('k909', 'tr909', [
    [0, 'kick'],
    [1, 'snare', { gainDb: -3 }],
    [2, 'chh', { chokeGroup: 1 }],
    [3, 'ohh', { chokeGroup: 1 }],
    [4, 'clap', { pan: -0.6 }],
  ]);
  const b = [n(4, 1, 0.8), n(4, 3, 0.8), n(4, 5, 0.8), n(4, 7, 0.8), n(3, 1.5, 0.6), n(2, 2, 0.7), n(3, 3.5, 0.6), n(2, 4, 0.7), n(1, 6.5, 0.9)];
  const muted = [n(0, 0.25, 1), n(0, 2.25, 1)];
  return [
    project('factory_basic', 120, [track('drums', fk, basic)], {}),
    project('classic_swing', 100, [track('tr808', k808, swing, { volume: -3, pan: 0.25 })], { swing: 0.5, swingGrid: '1/16' }),
    project('two_tracks', 128, [
      track('factory', fk2, a, { volume: -4, pan: -0.5 }),
      track('tr909', k909, b, { volume: -8, solo: true }),
      track('muted', structuredClone(factory), muted, { muted: true }),
    ], { swing: 0.3, swingGrid: '1/8' }),
    ...synthProjects(factory),
  ].filter((p) => !only || only.includes(p.id));
}

/** 16-bit PCM WAV → per-channel Float32 (int16 / 32768) + header fields. */
function decodeWav(bytes) {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const channels = dv.getUint16(22, true);
  const sampleRate = dv.getUint32(24, true);
  const bits = dv.getUint16(34, true);
  const dataSize = dv.getUint32(40, true);
  const frames = dataSize / (channels * 2);
  const out = Array.from({ length: channels }, () => new Float32Array(frames));
  for (let i = 0; i < frames; i++) {
    for (let c = 0; c < channels; c++) out[c][i] = dv.getInt16(44 + (i * channels + c) * 2, true) / 32768;
  }
  return { channels, sampleRate, bits, frames, bytes: bytes.length, data: out };
}

const only = process.env.PARITY_EXPORT_ONLY?.split(',');

export default async function steps(page, { log, url }) {
  await page.context().addInitScript(initScript);
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}?paritySr=${SR}`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => typeof window.__paritySeed === 'function');
  const rate = await page.evaluate(`(async () => (${IMPORT}).contextSampleRate())()`);
  if (rate !== SR) throw new Error(`context rate ${rate} != ${SR}`);
  const calls = await page.evaluate(`(async () => (${IMPORT}).preloadFactory(${JSON.stringify(VOICES)}, ${NOISE_SEED}, ${OFFSET_SEED}))()`);
  if (calls !== 2 * 44100 * 5 + 1) throw new Error(`noise buffer not drawn from the seed (${calls} draws)`);
  const noiseCalls = await page.evaluate(`(async () => (${IMPORT}).prepareNoise(${JSON.stringify(SYNTH_NOISE_SEEDS)}, ${SR}))()`);
  log(`synth noise buffers drawn (random calls ${JSON.stringify(noiseCalls)})`);
  const factory = await page.evaluate(`(async () => (${IMPORT}).factoryKit())()`);
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-export.steps.mjs via scripts/ui-snap.mjs (ExportService.exportWav, real Tone.js in headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      sampleRate: SR,
      evidence: 'per project and channel: blockEvidence (stats.mjs) of the decoded 16-bit WAV (int16 / 32768), 10 significant digits',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
      factorySeeds: { noise: NOISE_SEED, offsetBase: OFFSET_SEED, order: VOICES },
      synthNoiseSeeds: SYNTH_NOISE_SEEDS,
      synthSeed: SYNTH_SEED,
    },
    projects: [],
  };
  for (const arrangement of projects(factory)) {
    const entry = { id: arrangement.id, arrangement };
    if (arrangement.knownDeviation) {
      entry.knownDeviation = arrangement.knownDeviation;
      delete arrangement.knownDeviation;
    }
    for (const dedupe of [false, true]) {
      const r = await page.evaluate(`(async () => (${IMPORT}).exportProject(${JSON.stringify(arrangement)}, ${dedupe}, ${SYNTH_SEED}))()`);
      if (r.sr !== SR) throw new Error(`export rate ${r.sr}`);
      const bytes = new Uint8Array(Buffer.from(r.wav, 'base64'));
      if (DEBUG_DIR) {
        mkdirSync(DEBUG_DIR, { recursive: true });
        writeFileSync(path.join(DEBUG_DIR, `${arrangement.id}${dedupe ? '.dedup' : ''}.wav`), bytes);
      }
      const w = decodeWav(bytes);
      const peak = Math.max(...w.data.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
      const evidence = w.data.map((x) => blockEvidence(x).map(f10));
      if (!dedupe) {
        entry.wav = { bytes: w.bytes, channels: w.channels, sampleRate: w.sampleRate, bits: w.bits, frames: w.frames };
        entry.peak = f10(peak);
        entry.triggers = r.triggers.reduce((a, t) => a + t.length, 0);
        entry.duplicates = r.duplicates;
        entry.randomCalls = r.randomCalls;
        entry.evidence = evidence;
      } else {
        entry.evidenceDeduped = evidence;
      }
      log(`${arrangement.id}${dedupe ? ' (deduped)' : ''}: ${w.frames} frames (${(w.frames / SR).toFixed(3)} s), peak ${peak.toFixed(4)}, random ${r.randomCalls}, ${r.duplicates.length} duplicate triggers ${JSON.stringify(r.duplicates)}`);
    }
    out.projects.push(entry);
  }
  mkdirSync(FIXTURES, { recursive: true });
  if (only) {
    // a partial run (PARITY_EXPORT_ONLY=id,id): replace those projects in the existing fixture
    const prev = JSON.parse(readFileSync(path.join(FIXTURES, 'export_reference.json'), 'utf8'));
    for (const p of out.projects) {
      const i = prev.projects.findIndex((x) => x.id === p.id);
      if (i >= 0) prev.projects[i] = p;
      else prev.projects.push(p);
    }
    prev.meta = out.meta;
    out.projects = prev.projects;
  }
  writeFileSync(path.join(FIXTURES, 'export_reference.json'), JSON.stringify(out) + '\n');
  log(`wrote export_reference.json (${out.projects.length} projects)`);
}
