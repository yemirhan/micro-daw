/**
 * Chromium decode reference for the Rust `mdaw_dsp::audio_io` decoder/resampler.
 *
 * A `scripts/ui-snap.mjs --steps` module: decodes every file of rune/crates/mdaw-dsp/testdata in
 * the real renderer (headless Chromium + Vite dev server) and writes
 * rune/parity/fixtures/decode_reference.json.
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-decode.mjs --out /tmp/decode.png
 *
 * Two decodes per file:
 *  - `app`: the app's real path — `sampleManager.loadFromPath(path)` (SampleManager.ts:
 *    `electronAPI.sampleReadFile` → `Tone.getContext().rawContext.decodeAudioData`). Without
 *    Electron, `electronAPI.sampleReadFile` is stubbed with a fetch of `/@fs/<abs path>` (same
 *    bytes). decodeAudioData resamples to the context rate, recorded as `contextRate`.
 *  - `native`: `new OfflineAudioContext(1, 1, fileRate).decodeAudioData(bytes)` — the decoder
 *    alone, no resampling (isolates decoder priming from the resampler).
 * Per decode: sampleRate, length, channels, per-channel peak / RMS, first onset index
 * (|x| > ONSET_THRESHOLD on any channel), the first 512 samples of channel 0, 512 samples of
 * channel 0 from onset-32, and (app decode only) channel 0 (first 16384 frames) as base64 little-endian f32.
 */
import { readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TESTDATA = path.resolve(HERE, '../../crates/mdaw-dsp/testdata');
const OUT = path.resolve(HERE, '../fixtures/decode_reference.json');
const EXTS = ['wav', 'aif', 'aiff', 'mp3', 'flac', 'ogg', 'm4a'];
const ONSET_THRESHOLD = 0.01;

/** File rate from the `_<rate>k_` name token (gen.sh naming). */
function fileRate(name) {
  const m = /_(\d+)k_/.exec(name);
  const k = Number(m[1]);
  return k === 44 ? 44100 : k === 22 ? 22050 : k * 1000;
}

export default async (page, { log }) => {
  const files = readdirSync(TESTDATA)
    .filter((f) => EXTS.includes(f.split('.').pop().toLowerCase()))
    .sort();
  const results = await page.evaluate(
    `(async (files, threshold) => {
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
      const stats = (buf, full) => {
        const chans = [];
        for (let c = 0; c < buf.numberOfChannels; c++) chans.push(buf.getChannelData(c));
        let onset = -1;
        for (let i = 0; i < buf.length && onset < 0; i++)
          for (const d of chans) if (Math.abs(d[i]) > threshold) { onset = i; break; }
        const ch0 = chans[0];
        const from = Math.max(0, onset - 32);
        return {
          sampleRate: buf.sampleRate,
          length: buf.length,
          channels: buf.numberOfChannels,
          peak: chans.map((d) => d.reduce((m, x) => Math.max(m, Math.abs(x)), 0)),
          rms: chans.map((d) => Math.sqrt(d.reduce((s, x) => s + x * x, 0) / Math.max(1, d.length))),
          onset,
          head: Array.from(ch0.subarray(0, 512)),
          windowStart: from,
          window: Array.from(ch0.subarray(from, from + 512)),
          ...(full ? { ch0: b64(ch0.slice(0, 16384)) } : {}),
        };
      };
      const out = {};
      let contextRate = 0;
      for (const { name, abs, rate } of files) {
        const entry = {};
        try {
          const ref = await sampleManager.loadFromPath(abs);
          const buf = sampleManager.getBuffer(ref.id);
          contextRate = buf.sampleRate;
          entry.app = stats(buf, true);
        } catch (e) { entry.app = { error: String(e && e.message || e) }; }
        try {
          const bytes = await window.electronAPI.sampleReadFile(abs);
          const buf = await new OfflineAudioContext(1, 1, rate).decodeAudioData(bytes);
          entry.native = stats(buf, false);
        } catch (e) { entry.native = { error: String(e && e.message || e) }; }
        out[name] = entry;
      }
      return { contextRate, userAgent: navigator.userAgent, files: out };
    })(${JSON.stringify(files.map((name) => ({ name, abs: path.join(TESTDATA, name), rate: fileRate(name) })))}, ${ONSET_THRESHOLD})`,
  );
  results.onsetThreshold = ONSET_THRESHOLD;
  mkdirSync(path.dirname(OUT), { recursive: true });
  writeFileSync(OUT, JSON.stringify(results) + '\n');
  for (const [name, e] of Object.entries(results.files)) {
    const a = e.app.error ? `ERR ${e.app.error}` : `${e.app.sampleRate}Hz len=${e.app.length} ch=${e.app.channels} onset=${e.app.onset}`;
    const n = e.native.error ? `ERR ${e.native.error}` : `${e.native.sampleRate}Hz len=${e.native.length} onset=${e.native.onset}`;
    log(`${name}: app ${a} | native ${n}`);
  }
  log(`contextRate=${results.contextRate} -> ${OUT}`);
};
