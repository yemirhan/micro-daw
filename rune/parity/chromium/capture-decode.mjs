/**
 * Chromium decode reference for the Rust `mdaw_dsp::audio_io` decoder/resampler.
 *
 * A `scripts/ui-snap.mjs --steps` module: decodes every file of rune/crates/mdaw-dsp/testdata (and
 * the damaged / unusual files of testdata/edge, see testdata/edge/gen-edge.mjs) in the real
 * renderer (headless Chromium + Vite dev server) and writes rune/parity/fixtures/decode_reference.json.
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-decode.mjs --out /tmp/decode.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir  → also writes every decode as raw <file>.<app|native>.ch<c>.f32
 *
 * Two decodes per file:
 *  - `app`: the app's real path — `sampleManager.loadFromPath(path)` (SampleManager.ts:
 *    `electronAPI.sampleReadFile` → `Tone.getContext().rawContext.decodeAudioData`). Without
 *    Electron, `electronAPI.sampleReadFile` is stubbed with a fetch of `/@fs/<abs path>` (same
 *    bytes). decodeAudioData resamples to the context rate, recorded as `contextRate`.
 *  - `native`: `new OfflineAudioContext(1, 1, fileRate).decodeAudioData(bytes)` — the decoder
 *    alone, no resampling (isolates decoder priming from the resampler). Files whose rate is
 *    outside the OfflineAudioContext range use 44100.
 *
 * Every decode records, for EVERY channel (definitions in ./decode-stats.mjs, mirrored by
 * rune/crates/mdaw-dsp/src/audio_io/parity.rs):
 *  - full length: FNV-1a 64 digest of the little-endian f32 bytes, peak, RMS, non-finite count;
 *  - per 1024-frame block: RMS and 4 random ±1 projections (Σ x·s), from which the Rust test
 *    estimates the error energy of every block without storing the waveform;
 *  - the first HEAD frames as base64 f32 (app decode), 512 frames from onset−32 (native decode).
 */
import { readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { channelStats, onset } from './decode-stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TESTDATA = path.resolve(HERE, '../../crates/mdaw-dsp/testdata');
const EDGE = path.join(TESTDATA, 'edge');
const OUT = path.resolve(HERE, '../fixtures/decode_reference.json');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const EXTS = ['wav', 'aif', 'aiff', 'mp3', 'flac', 'ogg', 'm4a'];
const ONSET_THRESHOLD = 0.01;
const HEAD = 4096;
const WINDOW = 512;

/** File rate from the `_<rate>k_` name token (gen.sh naming); 44100 when absent or invalid for a context. */
function fileRate(name) {
  const m = /_(\d+)k_/.exec(name);
  if (!m) return 44100;
  const k = Number(m[1]);
  const r = k === 44 ? 44100 : k === 22 ? 22050 : k * 1000;
  return r >= 3000 && r <= 768000 ? r : 44100;
}

const list = (dir) =>
  readdirSync(dir)
    .filter((f) => EXTS.includes(f.split('.').pop().toLowerCase()))
    .sort()
    .map((name) => ({ name, abs: path.join(dir, name), rate: fileRate(name) }));

function fromB64(b64) {
  const buf = Buffer.from(b64, 'base64');
  return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
}

function toB64(f32) {
  return Buffer.from(f32.buffer, f32.byteOffset, f32.byteLength).toString('base64');
}

/** Raw page result → fixture entry. */
function summarize(raw, kind, debugName) {
  if (raw.error) return { error: raw.error };
  const chans = raw.data.map(fromB64);
  if (DEBUG_DIR) {
    mkdirSync(DEBUG_DIR, { recursive: true });
    chans.forEach((d, c) => writeFileSync(path.join(DEBUG_DIR, `${debugName}.${kind}.ch${c}.f32`), Buffer.from(d.buffer)));
  }
  const on = onset(chans, ONSET_THRESHOLD);
  const from = Math.max(0, on - 32);
  return {
    sampleRate: raw.sampleRate,
    length: raw.length,
    channels: chans.length,
    onset: on,
    windowStart: from,
    perChannel: chans.map((d) => ({
      ...channelStats(d),
      ...(kind === 'app' ? { head: toB64(d.slice(0, HEAD)) } : { window: toB64(d.slice(from, from + WINDOW)) }),
    })),
  };
}

export default async (page, { log }) => {
  const files = list(TESTDATA);
  const edge = list(EDGE);
  const decodeAll = (items) =>
    page.evaluate(
      `(async (files) => {
        if (!window.electronAPI) window.electronAPI = {};
        if (!window.electronAPI.sampleReadFile) {
          window.electronAPI.sampleReadFile = async (p) => (await fetch('/@fs' + p)).arrayBuffer();
        }
        const { sampleManager } = await import('/src/services/SampleManager.ts');
        const b64 = (f32) => {
          const u8 = new Uint8Array(f32.buffer, f32.byteOffset, f32.byteLength);
          let s = '';
          for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
          return btoa(s);
        };
        const raw = (buf) => {
          const data = [];
          for (let c = 0; c < buf.numberOfChannels; c++) data.push(b64(buf.getChannelData(c)));
          return { sampleRate: buf.sampleRate, length: buf.length, data };
        };
        const err = (e) => ({ error: String((e && e.message) || e) });
        const out = {};
        let contextRate = 0;
        for (const { name, abs, rate } of files) {
          const entry = {};
          try {
            const ref = await sampleManager.loadFromPath(abs);
            const buf = sampleManager.getBuffer(ref.id);
            contextRate = buf.sampleRate;
            entry.app = raw(buf);
          } catch (e) { entry.app = err(e); }
          try {
            const bytes = await window.electronAPI.sampleReadFile(abs);
            entry.native = raw(await new OfflineAudioContext(1, 1, rate).decodeAudioData(bytes));
          } catch (e) { entry.native = err(e); }
          out[name] = entry;
        }
        return { contextRate, userAgent: navigator.userAgent, files: out };
      })(${JSON.stringify(items)})`,
    );

  const main = await decodeAll(files);
  const edgeRaw = await decodeAll(edge);
  const results = {
    contextRate: main.contextRate,
    userAgent: main.userAgent,
    onsetThreshold: ONSET_THRESHOLD,
    head: HEAD,
    window: WINDOW,
    stats: 'rune/parity/chromium/decode-stats.mjs',
    files: {},
    edge: {},
  };
  for (const [key, raw] of [
    ['files', main],
    ['edge', edgeRaw],
  ]) {
    for (const [name, e] of Object.entries(raw.files)) {
      results[key][name] = { app: summarize(e.app, 'app', name), native: summarize(e.native, 'native', name) };
      const fmt = (d) => (d.error ? `ERR ${d.error}` : `${d.sampleRate}Hz len=${d.length} ch=${d.channels} onset=${d.onset}`);
      log(`${key}/${name}: app ${fmt(results[key][name].app)} | native ${fmt(results[key][name].native)}`);
    }
  }
  mkdirSync(path.dirname(OUT), { recursive: true });
  writeFileSync(OUT, JSON.stringify(results) + '\n');
  log(`contextRate=${results.contextRate} -> ${OUT}`);
};
