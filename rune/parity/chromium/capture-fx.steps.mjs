/**
 * Chromium reference capture for the effect chains: the app's real track strip
 * (`createTrackAudioNodes` + `applyTrackEffect` / `resetTrackEffects` / `scheduleAutomation`) and
 * the real live synth bus (`AudioEngine` + its setters, master bus included), with real Tone.js
 * in headless Chromium, rendered offline. Page side: ./capture-fx-page.ts.
 *
 *   node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/capture-fx.steps.mjs \
 *        --out /tmp/parity-fx.png
 *   (optional) CASES=a,b → only those cases, fixture not written.
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (L then R),
 *              `<id>.f32` (for `fx_parity_debug`).
 *   (optional) FX_FIXTURE=/some/file.json → write the fixture there instead (the Rust tests read
 *              the same variable); FX_PROBE=1 → one 44.1 kHz strip case per effect instead of the
 *              battery (bisecting a mismatch).
 *
 * The input of every case is `genSignal` (the same generator as capture-compressor.steps.mjs,
 * mirrored by `gen_signal` in rune/crates/mdaw-engine/src/fx/parity_tests.rs), generated here in
 * f64 and handed to the page as f32. Step frames are multiples of 128 (render quanta); the
 * automation case runs at 112.5 BPM so every 0.25 beat is a whole number of quanta at 48 kHz.
 *
 * Output: rune/parity/fixtures/fx_chains.json — the case scripts (the Rust test replays them on
 * the engine core) and, per case and channel, the full-length block evidence of ./stats.mjs
 * (`blockEvidence`, 10 significant digits), plus each case's per-step RMS (dBFS) for the doc.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-fx-page.ts')";
const f10 = (x) => Number(x.toPrecision(10));

const DRUMS = [
  { k: 'kick', at: 0.01, period: 0.5, count: 16, freq: 55, decay: 0.12, amp: [0.7, 0.6] },
  { k: 'noise', at: 0.26, period: 0.5, count: 16, decay: 0.06, amp: [0.35, 0.45], seed: 11 },
  { k: 'noise', at: 0.01, period: 0.125, count: 64, decay: 0.015, amp: [0.1, 0.18], seed: 77 },
  { k: 'sine', at: 0.0, end: 8.0, freq: 220, amp: [0.12, 0.1], phase: [0, 1.3] },
  { k: 'sine', at: 0.0, end: 8.0, freq: 277.18, amp: [0.08, 0.1], phase: [0.4, 0] },
  { k: 'sine', at: 0.0, end: 8.0, freq: 1318.5, amp: [0.04, 0.05], phase: [0, 2] },
];

const genSignal = (parts, frames, sr) => {
  const mulberry32 = (seed) => {
    let a = seed | 0;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  };
  const out = [new Float64Array(frames), new Float64Array(frames)];
  for (const p of parts) {
    for (let ch = 0; ch < 2; ch++) {
      const x = out[ch];
      const g = p.amp[ch];
      const start = Math.round(p.at * sr);
      if (p.k === 'sine') {
        const end = Math.min(frames, Math.round(p.end * sr));
        for (let i = start; i < end; i++) x[i] += g * Math.sin(2 * Math.PI * p.freq * ((i - start) / sr) + p.phase[ch]);
      } else {
        const period = Math.round(p.period * sr);
        const end = Math.min(frames, start + p.count * period);
        const rng = p.k === 'noise' ? mulberry32((p.seed + ch) >>> 0) : null;
        for (let i = start; i < end; i++) {
          const tt = ((i - start) % period) / sr;
          const env = Math.exp(-tt / p.decay);
          x[i] += rng ? g * (rng() * 2 - 1) * env : g * env * Math.sin(2 * Math.PI * p.freq * tt);
        }
      }
    }
  }
  return out.map((x) => Float32Array.from(x));
};

const b64 = (data) => Buffer.from(data.buffer, data.byteOffset, data.byteLength).toString('base64');
const decode = (b) => {
  const buf = Buffer.from(b, 'base64');
  return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
};

const DEFAULT_FX = () => ({
  reverb: { enabled: false, wet: 0.25 },
  delay: { enabled: false, time: 0.25, feedback: 0.3, wet: 0.3 },
  chorus: { enabled: false, depth: 0.5 },
  distortion: { enabled: false, amount: 0.3, wet: 0.5 },
  eq: { enabled: false, low: 0, mid: 0, high: 0 },
  compressor: { enabled: false, threshold: -24, ratio: 4, attack: 0.003, release: 0.25 },
  filter: { enabled: false, type: 'lowpass', cutoff: 18000, resonance: 1, lfoRate: 1, lfoDepth: 0, lfoWave: 'sine' },
});

const ALL_ON = () => ({
  reverb: { enabled: true, wet: 0.35 },
  delay: { enabled: true, time: 0.18, feedback: 0.45, wet: 0.4 },
  chorus: { enabled: true, depth: 0.7 },
  distortion: { enabled: true, amount: 0.6, wet: 0.5 },
  eq: { enabled: true, low: 4, mid: -3, high: 5 },
  compressor: { enabled: true, threshold: -30, ratio: 6, attack: 0.01, release: 0.2 },
  filter: { enabled: true, type: 'lowpass', cutoff: 1200, resonance: 4, lfoRate: 2, lfoDepth: 0.5, lfoWave: 'triangle' },
});

const q = (t, sr) => Math.round((t * sr) / 128) * 128;

function strip(id, seconds, track, steps = [], { sr = 48000, bpm = 120 } = {}) {
  return {
    id,
    kind: 'strip',
    sr,
    bpm,
    frames: q(seconds, sr),
    signal: DRUMS,
    track,
    steps: steps.map(([t, ops]) => ({ frame: q(t, sr), ops })),
  };
}

function live(id, seconds, init, steps = [], { sr = 48000 } = {}) {
  return {
    id,
    kind: 'live',
    sr,
    frames: q(seconds, sr),
    signal: DRUMS,
    init: init.map(([op, v]) => ({ op, v })),
    steps: steps.map(([t, ops]) => ({ frame: q(t, sr), ops: ops.map(([op, v]) => ({ op, v })) })),
  };
}

const lane = (parameter, points, visible = true) => ({
  parameter,
  visible,
  points: points.map(([beat, value]) => ({ beat, value })),
});

function cases() {
  const apply = (name, params) => ({ op: 'apply', name, params });
  if (process.env.FX_PROBE) {
    const one = (k, v) => ({ ...DEFAULT_FX(), [k]: v });
    const on = ALL_ON();
    return Object.keys(on).map((k) => strip(`p44_${k}`, 2.0, { volume: 0, pan: 0, effects: one(k, on[k]) }, [], { sr: 44100 }));
  }
  return [
    strip('strip_default', 2.0, { volume: -6, pan: 0, effects: DEFAULT_FX() }),
    strip('strip_all_on', 3.0, { volume: -3, pan: 0.4, effects: ALL_ON() }),
    strip('strip_apply', 4.0, { volume: 0, pan: 0, effects: DEFAULT_FX() }, [
      [0.3, [apply('filter', { enabled: true, cutoff: 1500, resonance: 3 })]],
      [0.6, [apply('distortion', { enabled: true, amount: 0.5, wet: 0.6 })]],
      [0.9, [apply('eq', { enabled: true, low: 4, mid: -3, high: 6 })]],
      [1.2, [apply('compressor', { enabled: true, threshold: -30, ratio: 6 })]],
      [1.5, [apply('chorus', { enabled: true, depth: 0.8 })]],
      [1.8, [apply('delay', { enabled: true, time: 0.2, feedback: 0.5, wet: 0.4 })]],
      [2.1, [apply('reverb', { enabled: true, wet: 0.4 })]],
      [2.4, [apply('filter', { lfoDepth: 0.6, lfoRate: 3, lfoWave: 'triangle' })]],
      [2.7, [apply('filter', { lfoRate: 5, lfoWave: 'square', cutoff: 2500 })]],
      [2.9, [{ op: 'pan', v: -0.5 }, { op: 'volume', v: -8 }]],
      [3.1, [apply('compressor', { enabled: false })]],
      // finding: the LFO is removed, `filter.frequency.value = cutoff` writes 0 Hz (overridden)
      [3.3, [apply('filter', { lfoDepth: 0 })]],
    ]),
    strip(
      'strip_automation',
      4.8,
      {
        volume: -2,
        pan: 0,
        effects: { ...DEFAULT_FX(), reverb: { enabled: true, wet: 0.2 } },
        automation: [
          lane('volume', [[0, 0.9], [2, 0.5], [4, 1]]),
          lane('pan', [[0, 0.5], [1, 0], [3, 1]]),
          lane('reverbWet', [[0.5, 0], [3, 1]]),
          lane('delayWet', [[0, 0], [2, 0.7]]),
          lane('chorusDepth', [[1, 0.8], [3, 0]]),
          lane('distortionWet', [[0, 0.2], [2, 0.9]]),
          lane('filterCutoff', [[0, 1], [1.5, 0.1], [3.5, 0.8]]),
          lane('eqLow', [[0, 0.5], [2, 1]]),
          lane('eqMid', [[1, 0]], false),
          lane('eqHigh', [[0, 0.5], [3, 0.9]]),
        ],
      },
      [
        [0, [{ op: 'play' }]],
        // removeAutomationLane's reset in the middle (the callbacks keep firing)
        [6 * (60 / 112.5), [{ op: 'reset' }]],
      ],
      { bpm: 112.5 },
    ),
    strip('strip_all_on_44k', 2.5, { volume: 0, pan: -0.3, effects: ALL_ON() }, [
      [1.0, [apply('filter', { type: 'highpass', cutoff: 500, lfoDepth: 0 })]],
      [1.6, [apply('delay', { enabled: false }), apply('reverb', { wet: 0.8 })]],
    ], { sr: 44100 }),
    // a volume write and a play-start reset while muted keep the track muted (the Electron
    // bug where `Tone.Volume.mute`'s −∞ was replaced is fixed in both apps, 2026-11-18)
    strip('strip_mute_volume', 2.0, { volume: 0, pan: 0, effects: DEFAULT_FX() }, [
      [0.5, [{ op: 'mute', m: true }]],
      [1.0, [{ op: 'volume', v: -6 }]],
      [1.2, [{ op: 'track', patch: { volume: -6 } }, { op: 'reset' }]],
      [1.5, [{ op: 'mute', m: false }]],
    ]),
    live('live_default', 2.0, []),
    // the live synth: setVolume while muted keeps it muted (same fix)
    live('live_mute_volume', 2.0, [], [
      [0.5, [['mute', 0]]],
      [1.0, [['setVolume', -3]]],
      [1.5, [['unmute', 0]]],
    ]),
    live(
      'live_all',
      4.0,
      [
        ['setDelayWet', 0.3], ['setChorusDepth', 0.5], ['setDistortionAmount', 0.4], ['setDistortionWet', 0.5],
        ['setEqLow', 3], ['setEqHigh', -4], ['setCompRatio', 4], ['setCompThreshold', -30],
        ['setFilterCutoff', 3000], ['setFilterResonance', 2], ['setFilterLfoDepth', 0.5], ['setFilterLfoRate', 2],
        ['setReverbWet', 0.3], ['setDelayTime', 0.15], ['setDelayFeedback', 0.4],
      ],
      [
        [0.5, [['setReverbWet', 0], ['setFilterLfoWave', 'sawtooth']]],
        [0.8, [['setDelayWet', 0]]],
        [1.2, [['setDelayWet', 0.4]]],
        [1.5, [['setFilterCutoff', 1200], ['setCompRatio', 1]]],
        [1.8, [['setChorusDepth', 0], ['setEqLow', 0], ['setEqHigh', 0]]],
        [2.1, [['setDistortionAmount', 0]]],
        [2.4, [['setVolume', -3], ['setDelayTime', 0.3], ['setDelayFeedback', 0.6]]],
        [2.7, [['setCompRatio', 8], ['setCompAttack', 0.02], ['setCompRelease', 0.1], ['setReverbWet', 0.5]]],
        // LFO off → back to the static cutoff (the Electron bug that wrote 0 Hz is fixed in both
        // apps, 2026-11-18)
        [3.0, [['setFilterLfoDepth', 0]]],
        [3.3, [['setFilterType', 'highpass']]],
      ],
    ),
    // stages taken out and put back one at a time (frozen state resumes)
    live(
      'live_rewire',
      3.2,
      [['setReverbWet', 0.3], ['setCompRatio', 4], ['setCompThreshold', -30]],
      [
        [0.5, [['setReverbWet', 0]]],
        [1.0, [['setCompRatio', 1]]],
        [1.5, [['setCompRatio', 6]]],
        [2.0, [['setReverbWet', 0.4]]],
        [2.5, [['setCompAttack', 0.02], ['setCompRelease', 0.1]]],
      ],
    ),
    ...[48000, 44100].map((sr) =>
      live(
        sr === 44100 ? 'live_filter_44k' : 'live_filter_48k',
        2.5,
        [['setFilterCutoff', 800], ['setFilterResonance', 6]],
        [
          [1.0, [['setFilterCutoff', 4000], ['setChorusDepth', 0.9]]],
          [1.5, [['setFilterType', 'bandpass'], ['setDelayWet', 0.5], ['setEqMid', 6]]],
          [2.0, [['setFilterResonance', 0.5], ['setVolume', -20]]],
        ],
        { sr },
      ),
    ),
  ];
}

/** Per-step RMS (dBFS, both channels) between consecutive step frames (and the end). */
function stepRms(c, chans) {
  const edges = [...new Set([0, ...c.steps.map((s) => s.frame), c.frames])].sort((a, b) => a - b);
  const out = [];
  for (let i = 0; i + 1 < edges.length; i++) {
    let acc = 0;
    for (const x of chans) for (let k = edges[i]; k < edges[i + 1]; k++) acc += x[k] * x[k];
    const rms = Math.sqrt(acc / (2 * (edges[i + 1] - edges[i])));
    out.push([edges[i], rms > 1e-10 ? Number((20 * Math.log10(rms)).toFixed(2)) : -200]);
  }
  return out;
}

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-fx.steps.mjs via scripts/ui-snap.mjs (the app\'s createTrackAudioNodes / ArrangementEngine FX paths / AudioEngine, real Tone.js in headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      evidence: 'per case and channel: blockEvidence (stats.mjs) of the full render, 10 significant digits; stepRms = [frame, dBFS] from each step to the next',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
    },
    cases: [],
  };
  const only = process.env.CASES?.split(',');
  for (const c of cases().filter((k) => !only || only.includes(k.id))) {
    // A fresh page per case: Tone caches the PeriodicWave of a phased LFO (the chorus' right
    // LFO) per page, and a wave made by an earlier context at another rate would run at the
    // wrong frequency (`rate_scale` is the creating context's). The app has one context.
    await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
    const input = genSignal(c.signal, c.frames, c.sr).map(b64);
    const r = await page.evaluate(`(async () => (${IMPORT}).renderCase(${JSON.stringify({ ...c, input })}))()`);
    const chans = r.channels.map(decode);
    if (chans[0].length !== c.frames) throw new Error(`${c.id}: ${chans[0].length} frames`);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
    }
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    const rms = stepRms(c, chans);
    out.cases.push({ ...c, log: r.log, peak: f10(peak), stepRms: rms, evidence: chans.map((x) => blockEvidence(x).map(f10)) });
    log(`${c.id}: ${c.frames} frames @ ${c.sr}, peak ${peak.toFixed(4)}, log ${JSON.stringify(r.log).slice(0, 300)}`);
    log(`  step RMS ${JSON.stringify(rms)}`);
  }
  if (only) return log('CASES set: fixture not written');
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(process.env.FX_FIXTURE ?? path.join(FIXTURES, 'fx_chains.json'), JSON.stringify(out) + '\n');
  log(`wrote fx_chains.json (${out.cases.length} cases)`);
}
