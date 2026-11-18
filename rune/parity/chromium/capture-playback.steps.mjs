/**
 * Chromium reference capture for AudioBufferSourceNode playback (pad voices): rate conversion of
 * 44.1 kHz classic voices and other-rate buffers at 44.1k / 48k / 96k contexts, pitch via
 * playbackRate, sub-sample start times, offsets, grain durations, stop(), reversed buffers, and
 * the app's real SamplerKit.trigger path (Tone.Offline).
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-playback.steps.mjs \
 *        --out /tmp/parity-playback.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir  → also writes every render as raw planar f32
 *              (<case id>.f32, channel after channel) for
 *              `PARITY_DEBUG_DIR=/some/dir cargo test -p mdaw-dsp abs_node_debug -- --ignored --nocapture`
 *
 * Page side: ./capture-playback-page.ts, imported in ./blank.html (no app boot).
 * Output: rune/parity/fixtures/playback_reference.json. Per case: the parameters, the output
 * length/channels, an FNV-1a 64 digest of the full output (f32 little-endian bytes, channel after
 * channel), per-1024-frame RMS per channel, and the full waveform (base64 f32) for a few cases.
 * Rust: rune/crates/mdaw-dsp/src/abs_node/parity_tests.rs.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-playback-page.ts')";

export const CTX_RATES = [44100, 48000, 96000];
export const PITCHES = [0, 7, -5];
export const BUFFERS = {
  kick808: { kind: 'classic', machine: 'tr808', voice: 'kick' },
  chh808: { kind: 'classic', machine: 'tr808', voice: 'chh' },
  synth32: { kind: 'synth', sampleRate: 32000, frames: 16000, channels: 2, seed: 12345 },
  synth48: { kind: 'synth', sampleRate: 48000, frames: 24000, channels: 1, seed: 777 },
  kick808Rev: { kind: 'classic', machine: 'tr808', voice: 'kick', reverse: true },
  synth32Rev: { kind: 'synth', sampleRate: 32000, frames: 16000, channels: 2, seed: 12345, reverse: true },
};
const WHENS = [0.0123456, 0.02, 0.00731];
const OFFSETS = [0.0371, 0, 0.05];
const MODES = ['sampler', 'grain', 'toEnd'];
const BLOCK = 1024;
const TAIL = 0.01;

function decode(b64) {
  const buf = Buffer.from(b64, 'base64');
  return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
}

/** FNV-1a 64 over the f32 LE bytes of every channel in order. */
export function fnv64(channels) {
  let h = 0xcbf29ce484222325n;
  const P = 0x100000001b3n;
  const M = 0xffffffffffffffffn;
  for (const ch of channels) {
    const bytes = new Uint8Array(ch.buffer, ch.byteOffset, ch.byteLength);
    for (const b of bytes) h = ((h ^ BigInt(b)) * P) & M;
  }
  return h.toString(16).padStart(16, '0');
}

function blockRms(x) {
  const out = [];
  for (let i = 0; i < x.length; i += BLOCK) {
    let s = 0;
    const end = Math.min(x.length, i + BLOCK);
    for (let j = i; j < end; j++) s += x[j] * x[j];
    out.push(Number(Math.sqrt(s / (end - i)).toPrecision(7)));
  }
  return out;
}

function pitchRate(p) {
  return Math.pow(2, p / 12);
}

/** Build the raw cases (every buffer × pitch × context rate, modes rotating). */
function rawCases(info) {
  const cases = [];
  const names = Object.keys(BUFFERS);
  CTX_RATES.forEach((ctxRate, ri) => {
    names.forEach((name, bi) => {
      PITCHES.forEach((pitch, pi) => {
        const b = info[name];
        const bufDur = b.frames / b.sampleRate;
        const rate = pitchRate(pitch);
        const mode = MODES[(ri + bi + pi) % 3];
        const when = WHENS[(bi + pi) % 3];
        const offset = OFFSETS[(ri + pi) % 3];
        const k = { id: `${name}_${ctxRate}_p${pitch}_${mode}`, ctxRate, buffer: BUFFERS[name], playbackRate: rate, when, offset };
        let real;
        if (mode === 'sampler') {
          // What SamplerKit hands to Tone: rate via setValueAtTime, start(when, offset), stop(end).
          const length = Math.min(0.25, bufDur - offset);
          real = length / rate;
          Object.assign(k, { rateMode: 'param', stop: when + real });
        } else if (mode === 'grain') {
          Object.assign(k, { rateMode: 'value', duration: 0.2 });
          real = 0.2 / rate;
        } else {
          Object.assign(k, { rateMode: 'value' });
          real = (bufDur - offset) / rate;
        }
        k.frames = Math.ceil((when + real + TAIL) * ctxRate / 128) * 128;
        cases.push(k);
      });
    });
  });
  // play-from-0 control cases at rate 1 and the `param` vs `value` contrast with a pitched start.
  for (const ctxRate of CTX_RATES) {
    const b = info.chh808;
    cases.push({
      id: `chh808_${ctxRate}_p0_plain`, ctxRate, buffer: BUFFERS.chh808, rateMode: 'value', playbackRate: 1, when: 0, offset: 0,
      frames: Math.ceil((b.frames / b.sampleRate + TAIL) * ctxRate / 128) * 128,
    });
    for (const rateMode of ['param', 'value']) {
      cases.push({
        id: `chh808_${ctxRate}_p7_${rateMode}`, ctxRate, buffer: BUFFERS.chh808, rateMode, playbackRate: pitchRate(7),
        when: 0.0123456, offset: 0.0013337,
        frames: Math.ceil((0.0123456 + b.frames / b.sampleRate / pitchRate(7) + TAIL) * ctxRate / 128) * 128,
      });
    }
  }
  return cases;
}

const SAMPLER_CASES = [
  { name: 'kick808', pad: { pitch: 7, startSec: 0.0371, endSec: null, reverse: false } },
  { name: 'kick808', pad: { pitch: -5, startSec: 0.01, endSec: 0.6, reverse: true } },
  { name: 'synth32', pad: { pitch: 0, startSec: 0.1, endSec: 0.4, reverse: false } },
  { name: 'chh808', pad: { pitch: 3, startSec: 0, endSec: null, reverse: false } },
];

function samplerCases(info) {
  const out = [];
  for (const ctxRate of [48000, 96000]) {
    for (const s of SAMPLER_CASES) {
      const b = info[s.name];
      const dur = b.frames / b.sampleRate;
      const end = s.pad.endSec ?? dur;
      const real = (end - s.pad.startSec) / pitchRate(s.pad.pitch);
      const when = 0.0123456;
      out.push({
        id: `kit_${s.name}_${ctxRate}_p${s.pad.pitch}${s.pad.reverse ? '_rev' : ''}`,
        ctxRate, buffer: BUFFERS[s.name], pad: s.pad, when,
        // Tone stops the native node from its offline clock (128-frame ticks) after `end`: render
        // well past it so the stop is inside the capture.
        frames: Math.ceil((when + real + 0.05) * ctxRate / 128) * 128,
      });
    }
  }
  return out;
}

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const ev = (expr) => page.evaluate(`(async () => (${IMPORT}).${expr})()`);
  const info = {};
  for (const [name, spec] of Object.entries(BUFFERS)) info[name] = await ev(`bufferInfo(${JSON.stringify(spec)})`);
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-playback.steps.mjs via scripts/ui-snap.mjs (headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      digest: 'FNV-1a 64 over f32 LE bytes, channel after channel, full render length',
      blockRms: BLOCK,
      buffers: info,
    },
    raw: [],
    sampler: [],
  };
  const store = (k, r, keepWave) => {
    const chans = r.channels.map(decode);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `${k.id}.f32`), Buffer.concat(chans.map((c) => Buffer.from(c.buffer))));
    }
    const o = { ...k, outFrames: chans[0].length, outChannels: chans.length, fnv64: fnv64(chans), rms: chans.map(blockRms) };
    if (keepWave) o.wave = r.channels;
    return o;
  };
  for (const k of rawCases(info)) {
    const r = await ev(`renderRaw(${JSON.stringify(k)})`);
    if (r.sr !== k.ctxRate) throw new Error(`${k.id}: rate ${r.sr}`);
    const keep = k.id.startsWith('chh808_48000') || k.id.startsWith('synth48_96000_p7');
    out.raw.push(store(k, r, keep));
    log(`${k.id}: ${out.raw.at(-1).fnv64}`);
  }
  for (const k of samplerCases(info)) {
    const r = await ev(`renderSampler(${JSON.stringify(k)})`);
    if (r.native.length !== 1) throw new Error(`${k.id}: ${r.native.length} native sources`);
    const o = store(k, r, false);
    o.native = r.native[0];
    out.sampler.push(o);
    log(`${k.id}: start ${JSON.stringify(o.native.start)} stop ${JSON.stringify(o.native.stop)} rate ${JSON.stringify(o.native.rateEvents)}`);
  }
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(path.join(FIXTURES, 'playback_reference.json'), JSON.stringify(out) + '\n');
  log(`wrote playback_reference.json (${out.raw.length} raw + ${out.sampler.length} SamplerKit cases)`);
}
