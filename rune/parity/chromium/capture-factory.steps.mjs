/**
 * Chromium reference capture for the factory drum voices (+ the kick/snare shaper curves).
 *
 *   node scripts/ui-snap.mjs --mode daw --steps rune/parity/chromium/capture-factory.steps.mjs \
 *        --out /tmp/parity-factory.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir  → also writes every seeded render as raw <voice>_<sr>.f32
 *                                            (full waveforms for `cargo test -p mdaw-dsp
 *                                            debug_full_waveforms -- --ignored --nocapture`)
 *   (optional) PARITY_TRIALS=24            → random (unseeded) renders per voice and rate (default 24)
 *
 * ui-snap provides the Vite dev server and headless Chromium (real Web Audio, real Tone.js). For
 * each sample rate (44100, 48000, 96000) the steps navigate to ./blank.html (same origin, same Vite
 * module graph, but WITHOUT booting the app: the app draws Tone's noise buffer on its own before
 * we could seed it) with an init script that forces the AudioContext sample rate (Tone.Offline
 * renders at the current context rate, like the app) and installs a switchable seeded
 * Math.random (mulberry32). ./capture-page.ts is then imported in the page and renders through
 * the app's own path (bufferCache.loadPadBuffer({ kind: 'factory' }) → Tone.Offline → trimTail):
 *
 *   seeded: noise buffer drawn from mulberry32(NOISE_SEED) (checked: exactly 2·220500 + 1 draws),
 *           every voice's noise start offset from mulberry32(OFFSET_SEED + voiceIndex) (checked:
 *           one Math.random call and one noise-buffer start per render; the offset is recorded).
 *           Deterministic → compared sample-wise in Rust.
 *   random: PARITY_TRIALS renders with the real Math.random (as in the app); per-trial
 *           statistics only (the app itself is not deterministic for noise voices).
 *
 * Output: rune/parity/fixtures/factory_reference.json (statistics + first 2048 samples),
 *         rune/parity/fixtures/factory_blocks.json (full-length waveform evidence of every seeded
 *         render: per 1024-sample block the energy + 16 random ±1 projections, see stats.mjs
 *         `blockEvidence`) and rune/parity/fixtures/shaping_chromium.json.
 * Statistic definitions: ./stats.mjs (mirrored by rune/crates/mdaw-dsp/src/factory/parity_tests.rs).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, HEAD, PROJ, PROJ_SEED, blockEvidence, envelope, stats, toDb } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');

export const VOICES = [
  'kick', 'kick-sub', 'snare', 'rim', 'clap', 'chh', 'phh', 'ohh',
  'tom-lo', 'tom-mid', 'tom-hi', 'crash', 'ride', 'shaker', 'perc', 'snap',
];
export const RATES = [44100, 48000, 96000];
export const NOISE_SEED = 0x5eed0001;
export const OFFSET_SEED = 0x5eed1000;
const TRIALS = Number(process.env.PARITY_TRIALS ?? 24);
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;

const r2 = (x) => Number(x.toFixed(2));
const f8 = (x) => Number(x.toPrecision(8));
const f10 = (x) => Number(x.toPrecision(10));

function decode(b64) {
  const buf = Buffer.from(b64, 'base64');
  return new Float32Array(buf.buffer, buf.byteOffset, buf.byteLength / 4);
}

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
  const nativeRandom = Math.random;
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
  // Record every start of Tone's noise buffer (220500 frames): offset + first samples.
  window.__parityNoiseStarts = [];
  const nativeStart = AudioBufferSourceNode.prototype.start;
  AudioBufferSourceNode.prototype.start = function (when, offset, duration) {
    const b = this.buffer;
    if (b && b.length === 44100 * 5) {
      window.__parityNoiseStarts.push({ when, offset, first: [b.getChannelData(0)[0], b.getChannelData(1)[0]] });
    }
    return nativeStart.call(this, when, offset, duration);
  };
  window.__parityUnseed = () => {
    Math.random = nativeRandom;
  };
}

const IMPORT = "await import('/rune/parity/chromium/capture-page.ts')";

export default async function steps(page, { log, url }) {
  await page.context().addInitScript(initScript);
  const out = { meta: {}, voices: {} };
  const blocks = { voices: {} };
  const shapers = {};
  for (const id of VOICES) {
    out.voices[id] = {};
    blocks.voices[id] = {};
  }

  for (const sr of RATES) {
    // A page without the app (blank.html): nothing draws Tone's noise buffer before we seed it.
    await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}?paritySr=${sr}`, { waitUntil: 'domcontentloaded' });
    await page.waitForFunction(() => typeof window.__paritySeed === 'function');
    const ctxRate = await page.evaluate(`(async () => (${IMPORT}).contextSampleRate())()`);
    if (ctxRate !== sr) throw new Error(`context rate ${ctxRate} != ${sr}`);
    out.meta.userAgent = await page.evaluate('navigator.userAgent');

    // --- seeded ---------------------------------------------------------------------------
    const noiseCalls = await page.evaluate(`(async () => (${IMPORT}).prepareSeededNoise(${NOISE_SEED}))()`);
    if (noiseCalls !== 2 * 44100 * 5 + 1) throw new Error(`noise buffer not drawn from the seed (${noiseCalls} draws)`);
    out.meta[`noiseFirst_${sr}`] = (await page.evaluate('window.__parityNoiseStarts'))[0].first;
    log(`sr ${sr}: seeded noise buffer generated (${noiseCalls} Math.random calls)`);
    for (let vi = 0; vi < VOICES.length; vi++) {
      const id = VOICES[vi];
      await page.evaluate('window.__parityNoiseStarts = []');
      const r = await page.evaluate(
        `(async () => (${IMPORT}).renderVoice(${JSON.stringify(id)}, ${OFFSET_SEED + vi}))()`,
      );
      const starts = await page.evaluate('window.__parityNoiseStarts');
      if (starts.length !== 1) throw new Error(`${id}: expected one noise start, got ${starts.length}`);
      if (r.sr !== sr) throw new Error(`render rate ${r.sr} != ${sr}`);
      const x = decode(r.data);
      if (DEBUG_DIR) {
        mkdirSync(DEBUG_DIR, { recursive: true });
        writeFileSync(path.join(DEBUG_DIR, `${id}_${sr}.f32`), Buffer.from(x.buffer, x.byteOffset, x.byteLength));
      }
      const s = stats(x, sr);
      blocks.voices[id][sr] = blockEvidence(x).map(f10);
      out.voices[id][sr] = {
        seeded: {
          randomCalls: r.randomCalls,
          noiseOffsetSec: starts[0].offset,
          length: s.length,
          peak: f8(s.peak),
          rms: f8(s.rms),
          onset: s.onset,
          zcr: r2(s.zcr),
          centroid: s.centroid.map(r2),
          env_db: s.env.map((e) => r2(toDb(e))),
          head: Array.from(x.subarray(0, HEAD), f8),
        },
      };
      log(`sr ${sr} seeded ${id}: len ${s.length} peak ${toDb(s.peak).toFixed(2)} dB, random calls ${r.randomCalls}`);
    }

    // --- random (the app as-is) ----------------------------------------------------------
    await page.evaluate('window.__parityUnseed()');
    for (const id of VOICES) {
      const lengths = [];
      const peaks = [];
      const rmss = [];
      const zcrs = [];
      const cents = [];
      let power = [];
      for (let k = 0; k < TRIALS; k++) {
        const r = await page.evaluate(`(async () => (${IMPORT}).renderVoice(${JSON.stringify(id)}))()`);
        const x = decode(r.data);
        const s = stats(x, sr);
        lengths.push(s.length);
        peaks.push(f8(s.peak));
        rmss.push(f8(s.rms));
        zcrs.push(s.zcr);
        cents.push(s.centroid.map(r2));
        const env = envelope(x, sr);
        if (env.length > power.length) power = power.concat(new Array(env.length - power.length).fill(0));
        env.forEach((e, i) => (power[i] += (e * e) / TRIALS));
      }
      out.voices[id][sr].random = {
        trials: TRIALS,
        length: lengths,
        peak: peaks,
        rms: rmss,
        zcr: r2(zcrs.reduce((a, b) => a + b, 0) / TRIALS),
        centroid: cents, // per trial: 4 windows
        env_db: power.map((p) => r2(toDb(Math.sqrt(p)))),
      };
      log(`sr ${sr} random ${id}: lengths ${Math.min(...lengths)}..${Math.max(...lengths)}`);
    }

    if (sr === RATES[0]) {
      const c = await page.evaluate(`(async () => (${IMPORT}).shaperCurves())()`);
      shapers.kick = Array.from(decode(c.kick));
      shapers.snare = Array.from(decode(c.snare));
    }
  }

  out.meta = {
    ...out.meta,
    source: 'rune/parity/chromium/capture-factory.steps.mjs via scripts/ui-snap.mjs (real Tone.js in Chromium)',
    renderPath: "bufferCache.loadPadBuffer({ kind: 'factory', voice }) with clearPadBufferCache() before each render",
    noiseSeed: NOISE_SEED,
    offsetSeedBase: OFFSET_SEED,
    prng: 'mulberry32 (Math.random replacement), see initScript',
    stats: 'rune/parity/chromium/stats.mjs',
  };
  mkdirSync(FIXTURES, { recursive: true });
  writeFileSync(path.join(FIXTURES, 'factory_reference.json'), JSON.stringify(out) + '\n');
  const blocksOut = {
    meta: {
      source: out.meta.source,
      evidence: 'seeded renders; per block [energy, PROJ projections] flattened, 10 significant digits',
      definition: 'rune/parity/chromium/stats.mjs blockEvidence',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
    },
    voices: blocks.voices,
  };
  writeFileSync(path.join(FIXTURES, 'factory_blocks.json'), JSON.stringify(blocksOut) + '\n');
  writeFileSync(
    path.join(FIXTURES, 'shaping_chromium.json'),
    JSON.stringify({ source: 'DrumKit kickShaper/snareShaper .curve read in Chromium', ...shapers }) + '\n',
  );
  log('wrote factory_reference.json, factory_blocks.json and shaping_chromium.json');
}
