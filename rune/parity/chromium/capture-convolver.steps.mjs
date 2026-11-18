/**
 * Chromium reference capture for the native `ConvolverNode` with the app's RoomReverb impulse
 * response (`createImpulseResponse` from src/audio/RoomReverb.ts, `normalize: true` as in
 * `RoomReverb`), rendered offline in headless Chromium.
 *
 *   node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/capture-convolver.steps.mjs \
 *        --out /tmp/parity-convolver.png
 *   (optional) CASES=a,b → only those cases, fixture not written.
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (L then R),
 *              `<id>.f32`, and each input as `<id>.in.f32` (for `convolver_parity_debug`).
 *
 * Graph per case: AudioBufferSourceNode (stereo, as long as the render) → ConvolverNode → destination.
 * The input is a list of events (impulses, mulberry32 noise bursts, sines) generated in f64 and
 * stored as f32: `inputSignal` below is mirrored by `case_input` in
 * rune/crates/mdaw-engine/src/fx/convolver_tests.rs. Each case has a silent gap longer than the
 * IR (the convolver's idle path) followed by more input.
 *
 * Output: rune/parity/fixtures/fx_convolver.json — the cases and, per case and channel, the
 * full-length block evidence of ./stats.mjs (`blockEvidence`, 10 significant digits).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const f10 = (x) => Number(x.toPrecision(10));

/** Events at `sr` (frames rounded): impulses on partition boundaries, noise, sines, a gap, more. */
function events(sr) {
  const fr = (t) => Math.round(t * sr);
  return [
    { type: 'impulse', ch: 0, frame: 0, amp: 1 },
    { type: 'impulse', ch: 1, frame: 127, amp: -0.8 },
    { type: 'impulse', ch: 0, frame: 1024, amp: 0.6 },
    { type: 'impulse', ch: 1, frame: 8191, amp: 0.7 },
    { type: 'noise', ch: 0, frame: fr(0.2), amp: 0.5, seed: 17, len: fr(0.3) },
    { type: 'noise', ch: 1, frame: fr(0.35), amp: 0.4, seed: 99, len: fr(0.25) },
    { type: 'sine', ch: 0, frame: fr(0.6), amp: 0.3, freq: 220, len: fr(0.4) },
    { type: 'sine', ch: 1, frame: fr(0.7), amp: 0.25, freq: 1750, len: fr(0.3) },
    // silence from 1.0 s to 4.0 s (longer than every IR here: the idle path), then more input
    { type: 'impulse', ch: 1, frame: fr(4.0), amp: 0.9 },
    { type: 'noise', ch: 0, frame: fr(4.05), amp: 0.3, seed: 5, len: fr(0.5) },
    { type: 'sine', ch: 1, frame: fr(4.2), amp: 0.2, freq: 97, len: fr(0.4) },
  ];
}

function cs(id, sr, ir = {}) {
  return { id, sr, frames: Math.round((6.2 * sr) / 128) * 128, ir, events: events(sr) };
}

function cases() {
  return [
    cs('room_48k', 48000),
    cs('room_44k', 44100),
    cs('room_96k', 96000),
    cs('short_nopre_48k', 48000, { decay: 0.5, preDelay: 0 }),
    cs('mono_ir_48k', 48000, { mono: true }),
  ];
}

/** Runs in the page: renders one case, returns base64 f32 channels. */
async function renderCase(c) {
  const { createImpulseResponse } = await import('/src/audio/RoomReverb.ts');
  const mulberry32 = (seed) => {
    let a = seed | 0;
    return () => {
      a = (a + 0x6d2b79f5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  };
  // inputSignal (mirrored by case_input in convolver_tests.rs).
  const inputSignal = () => {
    const x = [new Float64Array(c.frames), new Float64Array(c.frames)];
    for (const e of c.events) {
      const d = x[e.ch];
      if (e.type === 'impulse') d[e.frame] += e.amp;
      else if (e.type === 'noise') {
        const rng = mulberry32(e.seed);
        for (let i = 0; i < e.len; i++) d[e.frame + i] += e.amp * (rng() * 2 - 1);
      } else {
        for (let i = 0; i < e.len; i++) d[e.frame + i] += e.amp * Math.sin((2 * Math.PI * e.freq * i) / c.sr);
      }
    }
    return x.map((d) => Float32Array.from(d));
  };
  const b64 = (data) => {
    const bytes = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
    let s = '';
    for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
    return btoa(s);
  };
  const ctx = new OfflineAudioContext({ numberOfChannels: 2, length: c.frames, sampleRate: c.sr });
  let ir = createImpulseResponse(ctx, { ...c.ir });
  if (c.ir.mono) {
    const m = new AudioBuffer({ length: ir.length, numberOfChannels: 1, sampleRate: c.sr });
    m.copyToChannel(ir.getChannelData(0), 0);
    ir = m;
  }
  const buf = new AudioBuffer({ length: c.frames, numberOfChannels: 2, sampleRate: c.sr });
  const input = inputSignal();
  input.forEach((d, ch) => buf.copyToChannel(d, ch));
  const src = new AudioBufferSourceNode(ctx, { buffer: buf });
  const conv = new ConvolverNode(ctx, { disableNormalization: false });
  conv.buffer = ir;
  src.connect(conv).connect(ctx.destination);
  src.start(0);
  const out = await ctx.startRendering();
  return { channels: [0, 1].map((ch) => b64(out.getChannelData(ch))), inputs: input.map(b64), irFrames: ir.length, irChannels: ir.numberOfChannels };
}

const decode = (b) => {
  const buf = Buffer.from(b, 'base64');
  return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
};

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-convolver.steps.mjs via scripts/ui-snap.mjs (native ConvolverNode, RoomReverb IR, headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      evidence: 'per case and channel: blockEvidence (stats.mjs) of the full render, 10 significant digits',
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
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.in.f32`), Buffer.concat(r.inputs.map((b) => Buffer.from(b, 'base64'))));
    }
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    out.cases.push({ ...c, irFrames: r.irFrames, peak: f10(peak), evidence: chans.map((x) => blockEvidence(x).map(f10)) });
    log(`${c.id}: ${c.frames} frames @ ${c.sr}, IR ${r.irFrames}×${r.irChannels}, peak ${peak.toFixed(4)}`);
  }
  if (only) return log('CASES set: fixture not written');
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(path.join(FIXTURES, 'fx_convolver.json'), JSON.stringify(out) + '\n');
  log(`wrote fx_convolver.json (${out.cases.length} cases)`);
}
