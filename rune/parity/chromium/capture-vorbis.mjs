/**
 * Per-channel Chromium reference for the Ogg Vorbis decoder (`mdaw_dsp::audio_io`, vendored and
 * patched symphonia-codec-vorbis). It covers every channel of every `.ogg` file in
 * rune/crates/mdaw-dsp/testdata.
 *
 * A `scripts/ui-snap.mjs --steps` module. Run it from the repo root:
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-vorbis.mjs --out /tmp/vorbis.png
 *
 * It writes rune/parity/fixtures/vorbis_channels.json.
 *
 * Each file is decoded with `new OfflineAudioContext(1, 1, 44100).decodeAudioData(bytes)`. The
 * test files are 44.1 kHz, so no resampling happens and this is FFmpeg's decoder alone. For each
 * channel the fixture stores:
 *  - `peak` and `rms` over the full length. The peak catches clamping: Chromium keeps overs.
 *  - `stride`: every STRIDE-th sample over the full length.
 *  - `window`: WINDOW consecutive samples from `windowStart` (the onset, so the region is dense).
 *
 * Optional env: CAPTURE_RAW_DIR=<dir> also writes raw little-endian f32 dumps
 * (`<file>.ch<N>.f32`, full length) for ad-hoc comparisons.
 */
import { readdirSync, writeFileSync, mkdirSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TESTDATA = path.resolve(HERE, '../../crates/mdaw-dsp/testdata');
const OUT = path.resolve(HERE, '../fixtures/vorbis_channels.json');
const STRIDE = 32;
const WINDOW = 512;
const ONSET_THRESHOLD = 0.01;

export default async (page, { log }) => {
  const files = readdirSync(TESTDATA)
    .filter((f) => f.toLowerCase().endsWith('.ogg'))
    .sort();
  const raw = !!process.env.CAPTURE_RAW_DIR;
  const results = await page.evaluate(
    `(async (files, stride, window, threshold, raw) => {
      const out = {};
      for (const { name, abs } of files) {
        const bytes = await (await fetch('/@fs' + abs)).arrayBuffer();
        const buf = await new OfflineAudioContext(1, 1, 44100).decodeAudioData(bytes);
        const chans = [];
        for (let c = 0; c < buf.numberOfChannels; c++) chans.push(buf.getChannelData(c));
        let onset = 0;
        search: for (let i = 0; i < buf.length; i++)
          for (const d of chans) if (Math.abs(d[i]) > threshold) { onset = i; break search; }
        out[name] = {
          sampleRate: buf.sampleRate,
          length: buf.length,
          windowStart: onset,
          channels: chans.map((d) => ({
            peak: d.reduce((m, x) => Math.max(m, Math.abs(x)), 0),
            rms: Math.sqrt(d.reduce((s, x) => s + x * x, 0) / Math.max(1, d.length)),
            stride: Array.from(d.filter((_, i) => i % stride === 0)),
            window: Array.from(d.subarray(onset, onset + window)),
            ...(raw ? { raw: Array.from(d) } : {}),
          })),
        };
      }
      return { userAgent: navigator.userAgent, files: out };
    })(${JSON.stringify(files.map((name) => ({ name, abs: path.join(TESTDATA, name) })))}, ${STRIDE}, ${WINDOW}, ${ONSET_THRESHOLD}, ${raw})`,
  );
  if (raw) {
    mkdirSync(process.env.CAPTURE_RAW_DIR, { recursive: true });
    for (const [name, e] of Object.entries(results.files)) {
      e.channels.forEach((c, i) => {
        writeFileSync(
          path.join(process.env.CAPTURE_RAW_DIR, `${name}.ch${i}.f32`),
          Buffer.from(new Float32Array(c.raw).buffer),
        );
        delete c.raw;
      });
    }
  }
  results.stride = STRIDE;
  results.onsetThreshold = ONSET_THRESHOLD;
  mkdirSync(path.dirname(OUT), { recursive: true });
  writeFileSync(OUT, JSON.stringify(results) + '\n');
  for (const [name, e] of Object.entries(results.files)) {
    const peaks = e.channels.map((c) => c.peak.toFixed(4)).join('/');
    log(`${name}: ${e.sampleRate}Hz len=${e.length} ch=${e.channels.length} peak=${peaks}`);
  }
  log(`-> ${OUT}`);
};
