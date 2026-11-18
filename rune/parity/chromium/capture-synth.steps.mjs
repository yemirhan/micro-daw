/**
 * Chromium reference capture for the synth instruments: the app's real `PolyInstrument` with its
 * real Subtractive / FM / Pluck voices (`createSynthFromPreset`) and real Tone.js in headless
 * Chromium, rendered offline at 48 kHz, tapped at the voice bus (before CharacterChain), for
 * EVERY non-sampler preset of `SYNTH_PRESETS` plus FALLBACK_PIANO.
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-synth.steps.mjs \
 *        --out /tmp/parity-synth.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (L then R),
 *              `<id>.f32`, for `PARITY_DEBUG_DIR=/some/dir cargo test -p mdaw-engine
 *              synth_parity_debug -- --ignored --nocapture`.
 *   (optional) CASES=p01,p24 → only those cases (fixtures are not written).
 *
 * Script per preset (times in seconds, ops applied at the render quantum containing their time):
 * poly presets: C4 vel .8 for 1 s; a 3-note chord at velocities .3 / 1 / .8 with a re-strike of a
 * held note; polyphony + 1 staggered notes (forces stealing); two scheduled one-shots
 * (triggerAttackRelease, one below the 24 ms minimum); a zero-length gate (MIN_GATE); a long
 * sustain + release; two notes cut by releaseAll. Mono presets: a plain note, a fast overlapping
 * legato run, falling back to a held key, releasing a non-sounding key, velocities .3 / 1,
 * overlapping one-shots, releaseAll. Four presets also get a live `set()` case (a note, `set(params)`
 * while it sounds, a second note on a voice created after the set).
 *
 * Page side: ./capture-synth-page.ts. Output: rune/parity/fixtures/synth_reference_<engine>.json
 * (subtractive / fm / pluck): the scripts (the Rust test replays them) and, per case, the
 * full-length block evidence of ./stats.mjs (`blockEvidence`, 9 significant digits) of the left
 * channel, plus the right channel when it differs (noise sources are stereo), and the first sample
 * of each channel above −200 dBFS (`onset`).
 * Rust: rune/crates/mdaw-engine/src/synth/parity_tests.rs.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-synth-page.ts')";
const SR = 48000;
const Q = 128;
const DIGITS = 9;
const fd = (x) => Number(x.toPrecision(DIGITS));

/** Seeds of Tone's three noise buffers (drawn once per page, in this order). */
export const NOISE_SEEDS = { white: 0x5eed_2001, pink: 0x5eed_2002, brown: 0x5eed_2003 };
/** Seed of the noise start offsets of case `i` (re-seeded before each render). */
const caseSeed = (i) => 0x5eed_3000 + i;

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

const on = (note, vel, time) => ({ op: 'on', note, vel, time });
const off = (note, time) => ({ op: 'off', note, time });
const ar = (note, dur, vel, time) => ({ op: 'ar', note, dur, vel, time });
const releaseAll = (time) => ({ op: 'releaseAll', time });
const set = (params, time) => ({ op: 'set', params, time });

/** Live `set()` cases (voice update(), e.g. updateOmni applying semi/fine): preset index → params. */
const LIVE_SET = {
  1: { oscB: { fine: 20 }, filter: { cutoff: 1200 }, lfo: { rate: 3 } },
  12: { oscB: { width: -0.5, semi: 7 }, amp: { release: 0.4 } },
  2: { ratio: 2, ping: { level: 0.4 }, carrier: { wave: 'triangle' } },
  23: { noise: { type: 'brown', level: 0.4 }, oscA: { unison: 5 } },
};

function liveSetOps(params) {
  return {
    ops: [on(60, 0.8, 0.05), set(params, 0.3), on(64, 0.8, 0.5), off(60, 1.0), off(64, 1.0)],
    end: 1.0,
    lastRelease: 1.0,
  };
}

function steps(ops) {
  const merged = new Map();
  for (const o of ops) {
    const f = Math.floor((o.time * SR) / Q) * Q;
    merged.set(f, [...(merged.get(f) ?? []), o]);
  }
  return [...merged.entries()].sort((a, b) => a[0] - b[0]).map(([frame, ops]) => ({ frame, ops }));
}

function polyOps(polyphony) {
  const ops = [on(60, 0.8, 0.05), off(60, 1.05)];
  ops.push(on(60, 0.3, 1.2), on(64, 1, 1.2), on(67, 0.8, 1.2101), on(64, 0.6, 1.5), off(60, 1.8), off(64, 1.8), off(67, 1.8));
  const n = polyphony + 1;
  for (let i = 0; i < n; i++) ops.push(on(40 + 3 * i, 0.7, 2.0 + 0.0131 * i));
  for (let i = 0; i < n; i++) ops.push(off(40 + 3 * i, 2.6));
  ops.push(ar(72, 0.25, 0.6, 2.8), ar(76, 0.01, 1, 2.85), on(79, 0.8, 3.0), off(79, 3.0));
  ops.push(on(55, 0.9, 3.2), off(55, 4.0));
  ops.push(on(62, 0.5, 4.2), on(65, 0.5, 4.22), releaseAll(4.4));
  return { ops, end: 4.4, lastRelease: 4.0 };
}

function monoOps() {
  const ops = [on(36, 0.8, 0.05), off(36, 0.6)];
  ops.push(on(36, 0.8, 0.8), on(38, 0.7, 0.95), off(36, 1.0), on(40, 0.9, 1.1), off(38, 1.15), on(43, 0.8, 1.25), off(40, 1.3), on(41, 0.6, 1.4), off(43, 1.45), off(41, 1.7));
  ops.push(on(36, 0.8, 2.0), on(43, 0.8, 2.2), off(43, 2.5), off(36, 2.8));
  ops.push(on(48, 0.8, 3.0), on(50, 0.8, 3.1), off(48, 3.2), off(50, 3.5));
  ops.push(on(36, 0.3, 3.7), off(36, 3.95), on(36, 1, 4.1), off(36, 4.35));
  ops.push(ar(40, 0.2, 0.8, 4.5), ar(43, 0.2, 0.8, 4.6));
  ops.push(on(45, 0.8, 5.0), releaseAll(5.2));
  return { ops, end: 5.2, lastRelease: 4.8 };
}

/** The cases, from the preset table the page reports. */
function cases(presets) {
  const out = [];
  // Debugging: PROBE='[{"id":"x","preset":-1,"seconds":1,"ops":[["on",60,0.8,0.05],…]}]' renders
  // only these scripts (debug dumps only, no fixtures).
  if (process.env.PROBE) {
    for (const p of JSON.parse(process.env.PROBE)) {
      const ops = p.ops.map(([op, note, a, b, c]) =>
        op === 'on' ? on(note, a, b) : op === 'off' ? off(note, a) : op === 'ar' ? ar(note, a, b, c) : releaseAll(note));
      const preset = presets.find((x) => x.index === p.preset);
      out.push({ id: p.id, engine: preset.engine, preset: p.preset, sr: SR, frames: Math.ceil((p.seconds * SR) / Q) * Q, seed: p.seed ?? 1, overrides: p.overrides, steps: steps(ops) });
    }
    return out;
  }
  presets.forEach((p, idx) => {
    if (p.engine === 'sampler') return;
    const mono = p.engine === 'subtractive' && p.mono;
    const { ops, end, lastRelease } = mono ? monoOps() : polyOps(Math.max(1, Math.round(p.polyphony)));
    const tail = p.engine === 'pluck' ? p.release : p.amp.release;
    const frames = Math.ceil(((Math.max(end, lastRelease + tail) + 0.15) * SR) / Q) * Q;
    const i = p.index;
    out.push({ id: i < 0 ? 'fallback' : `p${String(i).padStart(2, '0')}`, engine: p.engine, preset: i, sr: SR, frames, seed: caseSeed(idx), steps: steps(ops) });
    if (LIVE_SET[i]) {
      const live = liveSetOps(LIVE_SET[i]);
      const f = Math.ceil(((live.lastRelease + Math.max(tail, 0.4)) + 0.15) * SR / Q) * Q;
      out.push({ id: `set${String(i).padStart(2, '0')}`, engine: p.engine, preset: i, sr: SR, frames: f, seed: caseSeed(idx) + 100, steps: steps(live.ops) });
    }
  });
  return out;
}

export default async function run(page, { log, url }) {
  await page.context().addInitScript(initScript);
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}?paritySr=${SR}`, { waitUntil: 'domcontentloaded' });
  await page.waitForFunction(() => typeof window.__paritySeed === 'function');
  const ctxRate = await page.evaluate(`(async () => (${IMPORT}).contextSampleRate())()`);
  if (ctxRate !== SR) throw new Error(`context rate ${ctxRate} != ${SR}`);
  const noiseCalls = await page.evaluate(`(async () => (${IMPORT}).prepareNoise(${JSON.stringify(NOISE_SEEDS)}, ${SR}))()`);
  log(`noise buffers drawn (random calls ${JSON.stringify(noiseCalls)})`);
  const presets = await page.evaluate(`(async () => {
    const m = await import('/src/audio/presets.ts');
    return [...m.SYNTH_PRESETS.map((p, index) => ({ ...p, index })), { ...m.FALLBACK_PIANO, index: -1 }];
  })()`);
  const meta = {
    source: 'rune/parity/chromium/capture-synth.steps.mjs via scripts/ui-snap.mjs (real PolyInstrument + voices + Tone.js in headless Chromium, voice bus before CharacterChain)',
    userAgent: await page.evaluate('navigator.userAgent'),
    evidence: `per case: blockEvidence (stats.mjs) of the left channel (and the right one when it differs), ${DIGITS} significant digits`,
    block: BLOCK,
    proj: PROJ,
    projSeed: PROJ_SEED,
    noiseSeeds: NOISE_SEEDS,
    noiseRandomCalls: noiseCalls,
  };
  const files = { subtractive: [], fm: [], pluck: [] };
  const only = process.env.CASES?.split(',') ?? (process.env.PROBE ? [] : undefined);
  const probe = !!process.env.PROBE;
  for (const c of cases(presets).filter((k) => probe || !only || only.includes(k.id))) {
    const t0 = Date.now();
    const r = await page.evaluate(`(async () => (${IMPORT}).renderScript(${JSON.stringify(c)}))()`);
    const chans = r.channels.map((b) => {
      const buf = Buffer.from(b, 'base64');
      return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
    });
    if (chans[0].length !== c.frames) throw new Error(`${c.id}: ${chans[0].length} frames`);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
    }
    const stereo = chans[0].some((v, i) => v !== chans[1][i]);
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    const evidence = (stereo ? chans : [chans[0]]).map((x) => blockEvidence(x).map(fd));
    const { engine, ...script } = c;
    // First sample above −200 dBFS (a 1e-28 FFT-rounding residue is not an onset).
    const onset = chans.map((x) => x.findIndex((v) => Math.abs(v) > 1e-10));
    files[engine].push({ ...script, randomCalls: r.randomCalls, pool: r.pool, stereo, peak: fd(peak), onset, evidence });
    log(`${c.id} (${engine}): ${c.frames} frames, peak ${peak.toFixed(4)}, stereo ${stereo}, pool ${r.pool}, random ${r.randomCalls}, ${Date.now() - t0} ms`);
  }
  if (probe) {
    if (DEBUG_DIR) writeFileSync(path.join(DEBUG_DIR, 'probe.json'), JSON.stringify(Object.values(files).flat()));
    return log('PROBE set: fixtures not written');
  }
  if (only) return log('CASES set: fixtures not written');
  mkdirSync(FIXTURES, { recursive: true });
  for (const [engine, list] of Object.entries(files)) {
    const file = path.join(FIXTURES, `synth_reference_${engine}.json`);
    writeFileSync(file, JSON.stringify({ meta, cases: list }) + '\n');
    log(`wrote ${path.basename(file)} (${list.length} cases)`);
  }
}
