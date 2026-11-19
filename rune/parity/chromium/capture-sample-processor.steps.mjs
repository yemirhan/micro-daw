/**
 * Chromium reference capture for the Samples view's DSP: the app's real `processAudio` and
 * `generateSignal` (src/services/SampleProcessor.ts, real Tone.js offline renders in headless
 * Chromium). Page side: ./capture-sample-processor-page.ts.
 *
 *   node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/capture-sample-processor.steps.mjs \
 *        --out /tmp/parity-sample-processor.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (channel after
 *              channel), `<id>.f32` (for `sample_processor_parity_debug`).
 *
 * Inputs (`input` below, mirrored by `case_input` in
 * rune/crates/mdaw-engine/src/sample_processor/parity_tests.rs): decaying sine plucks plus a
 * mulberry32 noise burst, generated in f64, stored as f32. `Math.random` is a mulberry32 seeded
 * per case while the case runs (the reverb's noise offsets, the noise generator); Tone's shared
 * white-noise buffer is overwritten once with mulberry32(0x5eed) (`seedNoise`). The reverb cases run
 * at the context rate (the noise buffer's rate), as every Samples render in the app does.
 *
 * Output: rune/parity/fixtures/sample_processor.json — the cases (params, seeds, input recipe)
 * and, per case and channel, the output length, peak and the full-length block evidence of
 * ./stats.mjs (`blockEvidence`, 10 significant digits).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-sample-processor-page.ts')";
const f10 = (x) => Number(x.toPrecision(10));

const DEFAULT = {
  filter: { enabled: false, type: 'lowpass', cutoff: 1000, resonance: 1 },
  pitchShift: { enabled: false, semitones: 0 },
  reverse: { enabled: false },
  normalize: { enabled: false },
  reverb: { enabled: false, decay: 2.5, wet: 0.3 },
  delay: { enabled: false, time: 0.25, feedback: 0.3, wet: 0.3 },
};
const P = (over) => {
  const p = structuredClone(DEFAULT);
  for (const [k, v] of Object.entries(over)) p[k] = { ...p[k], enabled: true, ...v };
  return p;
};

/** The input recipe (mirrored in Rust): plucks + a noise burst, `len` seconds. */
const recipe = (len) => ({
  len,
  plucks: [
    { at: 0.0, freq: 220, decay: 0.25, amp: [0.6, 0.45] },
    { at: 0.18, freq: 659.26, decay: 0.12, amp: [0.3, 0.4] },
    { at: 0.41, freq: 98, decay: 0.3, amp: [0.5, 0.5] },
  ],
  noise: { at: 0.3, len: 0.05, amp: [0.25, 0.2], seed: 4242 },
});

function input(r, sr, channels) {
  const frames = Math.round(r.len * sr);
  const mulberry32 = (seed) => {
    let a = seed | 0;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  };
  const out = [];
  for (let ch = 0; ch < channels; ch++) {
    const x = new Float64Array(frames);
    for (const p of r.plucks) {
      const s = Math.round(p.at * sr);
      for (let i = s; i < frames; i++) {
        const t = (i - s) / sr;
        x[i] += p.amp[ch] * Math.exp(-t / p.decay) * Math.sin(2 * Math.PI * p.freq * t + ch * 0.7);
      }
    }
    const rnd = mulberry32(r.noise.seed + ch);
    const s = Math.round(r.noise.at * sr);
    const e = Math.min(frames, s + Math.round(r.noise.len * sr));
    for (let i = s; i < e; i++) x[i] += r.noise.amp[ch] * (rnd() * 2 - 1);
    out.push(Float32Array.from(x));
  }
  return out;
}

const proc = (id, sr, channels, len, params, seed = 1) => ({ id, kind: 'process', sr, channels, recipe: recipe(len), params, seed });
const gen = (id, params, seed = 1) => ({ id, kind: 'generate', params, seed });

/**
 * The 48 kHz run (`UI_SNAP_CONTEXT_RATE=48000`): the reverb (noise buffer built at the context
 * rate) and generator cases at 48 kHz, written to sample_processor_48k.json.
 */
function cases48() {
  return [
    proc('reverb_48k_stereo', 48000, 2, 0.5, P({ reverb: {} }), 7),
    proc('reverb_short_48k_mono', 48000, 1, 0.5, P({ reverb: { decay: 0.8, wet: 0.7 } }), 8),
    proc('all_48k_stereo', 48000, 2, 0.5, P({ reverse: {}, filter: { cutoff: 3000 }, pitchShift: { semitones: -5 }, delay: { time: 0.1, feedback: 0.5 }, reverb: { decay: 1.2, wet: 0.4 }, normalize: {} }), 9),
    proc('normalize_only_48k_stereo', 48000, 2, 0.4, P({ normalize: {} })),
    gen('gen_sine', { type: 'sine', frequency: 440, duration: 1, amplitude: 0.8 }),
    gen('gen_square', { type: 'square', frequency: 220, duration: 0.5, amplitude: 0.5 }),
    gen('gen_saw', { type: 'sawtooth', frequency: 1000, duration: 0.3, amplitude: 1 }),
    gen('gen_triangle', { type: 'triangle', frequency: 97, duration: 0.7, amplitude: 0.25 }),
    gen('gen_noise', { type: 'white-noise', frequency: 440, duration: 0.25, amplitude: 0.6 }, 11),
  ];
}

function cases() {
  return [
    proc('filter_lp_48k_stereo', 48000, 2, 0.6, P({ filter: {} })),
    proc('filter_hp_44k_mono', 44100, 1, 0.6, P({ filter: { type: 'highpass', cutoff: 2500, resonance: 4 } })),
    proc('filter_bp_48k_stereo', 48000, 2, 0.6, P({ filter: { type: 'bandpass', cutoff: 800, resonance: 10 } })),
    proc('pitch_up7_48k_stereo', 48000, 2, 0.6, P({ pitchShift: { semitones: 7 } })),
    proc('pitch_down12_48k_mono', 48000, 1, 0.6, P({ pitchShift: { semitones: -12 } })),
    proc('pitch_0_44k_mono', 44100, 1, 0.6, P({ pitchShift: { semitones: 0 } })),
    proc('pitch_up24_44k_stereo', 44100, 2, 0.5, P({ pitchShift: { semitones: 24 } })),
    proc('delay_48k_stereo', 48000, 2, 0.6, P({ delay: {} })),
    proc('delay_cap_44k_mono', 44100, 1, 0.4, P({ delay: { time: 0.6, feedback: 0.8, wet: 0.5 } })),
    proc('reverb_44k_stereo', 44100, 2, 0.5, P({ reverb: {} }), 7),
    proc('reverb_short_44k_mono', 44100, 1, 0.5, P({ reverb: { decay: 0.8, wet: 0.7 } }), 8),
    proc('all_44k_stereo', 44100, 2, 0.5, P({ reverse: {}, filter: { cutoff: 3000 }, pitchShift: { semitones: -5 }, delay: { time: 0.1, feedback: 0.5 }, reverb: { decay: 1.2, wet: 0.4 }, normalize: {} }), 9),
    proc('reverse_normalize_44k_stereo', 44100, 2, 0.4, P({ reverse: {}, normalize: {} })),
    // Normalize only: the render is a scaled copy; the page also reports whether the source
    // buffer changed (it did before the review-9 fix).
    proc('normalize_only_48k_stereo', 48000, 2, 0.4, P({ normalize: {} })),
    gen('gen_sine', { type: 'sine', frequency: 440, duration: 1, amplitude: 0.8 }),
    gen('gen_square', { type: 'square', frequency: 220, duration: 0.5, amplitude: 0.5 }),
    gen('gen_saw', { type: 'sawtooth', frequency: 1000, duration: 0.3, amplitude: 1 }),
    gen('gen_triangle', { type: 'triangle', frequency: 97, duration: 0.7, amplitude: 0.25 }),
    gen('gen_noise', { type: 'white-noise', frequency: 440, duration: 0.25, amplitude: 0.6 }, 11),
  ];
}

const b64 = (f32) => Buffer.from(f32.buffer, f32.byteOffset, f32.byteLength).toString('base64');
const unb64 = (s) => {
  const b = Buffer.from(s, 'base64');
  return new Float32Array(b.buffer, b.byteOffset, b.byteLength / 4);
};

export default async (page, { log }) => {
  const contextRate = await page.evaluate(`(async () => (${IMPORT}).contextRate())()`);
  const all = contextRate === 48000 ? cases48() : cases();
  const outName = contextRate === 48000 ? 'sample_processor_48k.json' : 'sample_processor.json';
  const NOISE_SEED = 0x5eed;
  const [noiseRate, noiseFrames] = await page.evaluate(`(async () => (${IMPORT}).seedNoise(${NOISE_SEED}))()`);
  log(`context rate ${contextRate}, noise buffer ${noiseFrames} frames at ${noiseRate}`);
  const out = { block: BLOCK, proj: PROJ, projSeed: PROJ_SEED, contextRate, noise: { seed: NOISE_SEED, rate: noiseRate, frames: noiseFrames }, cases: [] };
  if (DEBUG_DIR) mkdirSync(DEBUG_DIR, { recursive: true });
  for (const c of all) {
    const pageCase = { id: c.id, kind: c.kind, seed: c.seed, params: c.params };
    if (c.kind === 'process') {
      const x = input(c.recipe, c.sr, c.channels);
      Object.assign(pageCase, { sr: c.sr, channels: c.channels, frames: x[0].length, input: x.map(b64) });
    }
    const res = await page.evaluate(`(async () => (${IMPORT}).runCase(${JSON.stringify(pageCase)}))()`);
    const chans = res.channels.map(unb64);
    log(`${c.id}: sr ${res.sr}, ${chans.length} ch × ${chans[0].length}`);
    if (DEBUG_DIR) {
      const all = new Float32Array(chans.reduce((n, ch) => n + ch.length, 0));
      let o = 0;
      for (const ch of chans) {
        all.set(ch, o);
        o += ch.length;
      }
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.f32`), Buffer.from(all.buffer));
    }
    out.cases.push({
      ...c,
      sr: res.sr,
      ...(res.sourceUnchanged === undefined ? {} : { sourceUnchanged: res.sourceUnchanged }),
      out: chans.map((x) => ({
        length: x.length,
        peak: f10(x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)),
        blocks: blockEvidence(x).map(f10),
      })),
    });
  }
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(path.join(FIXTURES, outName), JSON.stringify(out));
  log(`wrote ${path.join(FIXTURES, outName)}`);
};
