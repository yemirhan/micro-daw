/**
 * Chromium reference capture for the native `DynamicsCompressorNode` (raw Web Audio, no Tone.js),
 * rendered offline in headless Chromium at 48 kHz (plus one case at 44.1 kHz).
 *
 *   node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/capture-compressor.steps.mjs \
 *        --out /tmp/parity-compressor.png
 *   (optional) CASES=a,b → only those cases, fixture not written.
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (L then R),
 *              `<id>.f32`, and each input as `<id>.in.f32`.
 *
 * Graph per case: AudioBufferSourceNode (a stereo buffer as long as the render, so the source is
 * active throughout) → DynamicsCompressorNode → destination. The input signal is described by a
 * list of parts (decaying sine "kicks", mulberry32 noise bursts with exponential envelopes,
 * sustained / gated sines, a step from silence to full scale) generated in f64 and stored as f32:
 * `genSignal` below is mirrored exactly by `gen_signal` in
 * rune/crates/mdaw-engine/src/fx/compressor_tests.rs. k-rate parameter changes are applied with
 * `.value =` inside `suspend()` callbacks at render-quantum boundaries; at every step the node's
 * `reduction` is read first (the meter after the quantum that ended there).
 *
 * Output: rune/parity/fixtures/fx_compressor.json — the case scripts and, per case and channel,
 * the full-length block evidence of ./stats.mjs (`blockEvidence`, 10 significant digits).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const f10 = (x) => Number(x.toPrecision(10));

const DEFAULTS = { threshold: -24, knee: 30, ratio: 12, attack: 0.003, release: 0.25 };

/** Drum-like loop: kicks, snare-ish noise bursts, hats, a bass sine; different L/R. */
const DRUMS = [
  { k: 'kick', at: 0.01, period: 0.5, count: 8, freq: 55, decay: 0.12, amp: [0.9, 0.8] },
  { k: 'noise', at: 0.26, period: 0.5, count: 8, decay: 0.06, amp: [0.5, 0.6], seed: 11 },
  { k: 'noise', at: 0.01, period: 0.125, count: 32, decay: 0.015, amp: [0.15, 0.25], seed: 77 },
  { k: 'sine', at: 0.0, end: 4.0, freq: 110, amp: [0.2, 0.15], phase: [0, 1.3] },
];
const LOUD_SINE = [{ k: 'sine', at: 0.05, end: 2.0, freq: 440, amp: [0.95, 0.7], phase: [0, 0.5] }];
const QUIET = [
  { k: 'sine', at: 0.0, end: 2.0, freq: 330, amp: [0.003, 0.002], phase: [0, 2] },
  { k: 'noise', at: 0.1, period: 0.3, count: 6, decay: 0.05, amp: [0.002, 0.003], seed: 5 },
];
const STEP = [
  { k: 'sine', at: 0.5, end: 1.4, freq: 220, amp: [1.0, 0.9], phase: [Math.PI / 2, Math.PI / 2] },
  { k: 'sine', at: 1.6, end: 2.0, freq: 1000, amp: [0.5, 0.5], phase: [Math.PI / 2, 0] },
];

const q = (t, sr) => Math.round((t * sr) / 128) * 128;

/**
 * A case: `params` at frame 0; `changes` = [[seconds, {param: value}], …]; every `meter` seconds a
 * reduction read without a change. Frames are rounded to render quanta.
 */
function cs(id, seconds, signal, params, changes = [], { sr = 48000, meter = 0.25 } = {}) {
  const steps = new Map();
  for (let t = meter; t < seconds - 1e-9; t += meter) steps.set(q(t, sr), {});
  for (const [t, set] of changes) steps.set(q(t, sr), { ...(steps.get(q(t, sr)) ?? {}), ...set });
  return {
    id,
    sr,
    frames: q(seconds, sr),
    signal,
    params: { ...DEFAULTS, ...params },
    steps: [...steps.entries()].sort((a, b) => a[0] - b[0]).map(([frame, set]) => ({ frame, set })),
  };
}

function cases() {
  return [
    cs('drums_default', 2.5, DRUMS, {}),
    cs('drums_live_synth', 2.5, DRUMS, { knee: 6 }),
    cs('drums_44k', 2.0, DRUMS, { threshold: -30, ratio: 6, knee: 10 }, [], { sr: 44100 }),
    cs('t60_r20_k0_a0', 2.0, DRUMS, { threshold: -60, ratio: 20, knee: 0, attack: 0, release: 0.01 }),
    cs('t40_r1p5_k40', 2.0, DRUMS, { threshold: -40, ratio: 1.5, knee: 40, attack: 0.05, release: 1 }),
    cs('t12_r4_k6_slow', 2.0, DRUMS, { threshold: -12, ratio: 4, knee: 6, attack: 0.5, release: 0.25 }),
    cs('t3_r12_sine', 2.0, LOUD_SINE, { threshold: -3, ratio: 12, knee: 0, attack: 0.003, release: 0.25 }),
    cs('sine_t24_r20_k30', 2.0, LOUD_SINE, { ratio: 20, attack: 0.05, release: 1 }),
    cs('ratio1_bypass', 1.5, DRUMS, { threshold: 0, ratio: 1, knee: 0 }),
    cs('ratio1_t40', 1.5, DRUMS, { threshold: -40, ratio: 1, knee: 30 }),
    cs('quiet_under', 2.0, QUIET, {}),
    cs('step', 2.0, STEP, { threshold: -40, ratio: 20, knee: 0, attack: 0, release: 0.01 }),
    cs('step_slow', 2.0, STEP, { threshold: -24, ratio: 4, knee: 40, attack: 0.5, release: 1 }),
    cs('release0_t100', 1.5, DRUMS, { threshold: -100, knee: 0, ratio: 20, attack: 1, release: 0 }),
    cs('t0_k0_r20', 1.5, LOUD_SINE, { threshold: 0, knee: 0, ratio: 20, attack: 0, release: 0.01 }),
    cs('changes', 3.0, DRUMS, {}, [
      [0.3, { threshold: -40 }],
      [0.55, { ratio: 2 }],
      [0.8, { attack: 0.2, release: 0.05 }],
      [1.1, { threshold: -6, ratio: 20, knee: 0 }],
      [1.4, { attack: 0, release: 1 }],
      [1.7, { knee: 40, threshold: -50 }],
      [2.0, { ratio: 1 }],
      [2.3, { threshold: -20, ratio: 8, attack: 0.003, release: 0.25, knee: 6 }],
    ], { meter: 0.1 }),
    cs('changes_clamped', 2.0, LOUD_SINE, { threshold: -30 }, [
      [0.4, { knee: 50, ratio: 30 }],
      [0.8, { threshold: -150, attack: 2, release: 3 }],
      [1.2, { threshold: 10, knee: -5, ratio: 0.5, attack: -1, release: -1 }],
      [1.6, { threshold: -24, knee: 30, ratio: 12, attack: 0.003, release: 0.25 }],
    ], { meter: 0.1 }),
    cs('changes_step', 2.0, STEP, { threshold: -30, ratio: 6 }, [
      [0.5, { attack: 0.01 }],
      [0.9, { release: 0.5 }],
      [1.5, { threshold: -10 }],
    ], { meter: 0.05 }),
  ];
}

/** Runs in the page: renders one case, returns base64 f32 channels + the reduction reads. */
async function renderCase(c) {
  const mulberry32 = (seed) => {
    let a = seed | 0;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  };
  // genSignal (mirrored by gen_signal in compressor_tests.rs).
  const genSignal = (parts, frames, sr) => {
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
  const b64 = (data) => {
    const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
  const ctx = new OfflineAudioContext({ numberOfChannels: 2, length: c.frames, sampleRate: c.sr });
  const input = genSignal(c.signal, c.frames, c.sr);
  const buf = new AudioBuffer({ length: c.frames, numberOfChannels: 2, sampleRate: c.sr });
  input.forEach((d, ch) => buf.copyToChannel(d, ch));
  const src = new AudioBufferSourceNode(ctx, { buffer: buf });
  const comp = new DynamicsCompressorNode(ctx);
  for (const [k, v] of Object.entries(c.params)) comp[k].value = v;
  src.connect(comp).connect(ctx.destination);
  src.start(0);
  const reduction = [];
  const warn = console.warn;
  console.warn = () => {};
  for (const step of c.steps) {
    ctx.suspend(step.frame / c.sr).then(() => {
      reduction.push({ frame: step.frame, ctxFrame: Math.round(ctx.currentTime * c.sr), reduction: comp.reduction });
      for (const [k, v] of Object.entries(step.set)) comp[k].value = v;
      ctx.resume();
    });
  }
  const out = await ctx.startRendering();
  console.warn = warn;
  return {
    channels: [0, 1].map((ch) => b64(out.getChannelData(ch))),
    inputs: input.map(b64),
    reduction,
  };
}

const decode = (b) => {
  const buf = Buffer.from(b, 'base64');
  return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
};

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-compressor.steps.mjs via scripts/ui-snap.mjs (raw DynamicsCompressorNode in headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      evidence: 'per case and channel: blockEvidence (stats.mjs) of the full render, 10 significant digits; reduction = node.reduction read at each step before its changes',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
    },
    cases: [],
  };
  const only = process.env.CASES?.split(',');
  for (const c of cases().filter((k) => !only || only.includes(k.id))) {
    const r = await page.evaluate(renderCase, c);
    const chans = r.channels.map(decode);
    if (chans[0].length !== c.frames) throw new Error(`${c.id}: ${chans[0].length} frames`);
    for (const s of r.reduction) if (s.frame !== s.ctxFrame) throw new Error(`${c.id}: suspend at ${s.ctxFrame}, wanted ${s.frame}`);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.in.f32`), Buffer.concat(r.inputs.map((b) => Buffer.from(b, 'base64'))));
    }
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    out.cases.push({
      ...c,
      reduction: r.reduction.map((s) => f10(s.reduction)),
      peak: f10(peak),
      evidence: chans.map((x) => blockEvidence(x).map(f10)),
    });
    log(`${c.id}: ${c.frames} frames @ ${c.sr}, peak ${peak.toFixed(4)}, reduction ${r.reduction.slice(0, 6).map((s) => s.reduction.toFixed(3)).join(' ')}…`);
  }
  if (only) return log('CASES set: fixture not written');
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(path.join(FIXTURES, 'fx_compressor.json'), JSON.stringify(out) + '\n');
  log(`wrote fx_compressor.json (${out.cases.length} cases)`);
}
