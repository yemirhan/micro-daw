/**
 * Chromium reference capture for the sampled piano: the app's real `src/audio/SampledPiano.ts`
 * (Tone.Sampler over the bundled Salamander set, decoded by the app's `loadPianoSamples` at the
 * context rate) with real Tone.js in headless Chromium, rendered offline at 48 kHz through a
 * scripted battery: every sample root, notes between / outside the samples, velocities, sustain
 * and release (incl. live `set` of the release), re-strike, triggerAttackRelease, polyphony
 * overflow (steal), releaseAll, and the grand / bright piano presets through their
 * CharacterChain.
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-piano.steps.mjs --out /tmp/p.png
 *   (optional) PARITY_DEBUG_DIR=/dir → also writes each render as raw f32 (L then R), piano_<id>.f32.
 *
 * Page side: ./capture-piano-page.ts. Output: rune/parity/fixtures/piano_reference.json (the
 * scripts, the decoded buffers' lengths + digests, and per case and channel the block evidence of
 * ./stats.mjs). Rust: rune/crates/mdaw-engine/src/piano/parity_tests.rs.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-piano-page.ts')";
const SR = 48000;
const f10 = (x) => Number(x.toPrecision(10));

const on = (midi, time, vel = 0.6) => ({ op: 'on', midi, vel, time });
const off = (midi, time) => ({ op: 'off', midi, time });
const ar = (midi, dur, time, vel = 0.6) => ({ op: 'ar', midi, dur, time, vel });
const probe = (time) => ({ op: 'probe', time });
const set = (params) => ({ op: 'set', params });

/** A plain piano: no character stages, 0 dB. */
const PLAIN = { gain: 0, character: { tone: 20000 } };

/** `steps` maps seconds (rounded to a quantum) to ops; 0 = before rendering. */
function cs(id, seconds, preset, overrides, steps) {
  const q = (t) => Math.round((t * SR) / 128) * 128;
  const merged = new Map();
  for (const [t, ops] of steps) {
    const f = q(t);
    merged.set(f, [...(merged.get(f) ?? []), ...ops]);
  }
  const s = [...merged.entries()].sort((a, b) => a[0] - b[0]).map(([frame, ops]) => ({ frame, ops }));
  return { id, sr: SR, frames: q(seconds), preset, overrides, steps: s };
}

const ROOTS = [];
for (let m = 21; m <= 108; m += 3) ROOTS.push(m);

function cases() {
  const roots = ROOTS.flatMap((m, i) => [on(m, 0.01 + i * 0.25, 0.7), off(m, 0.2 + i * 0.25)]);
  const between = [0, 5, 20, 22, 23, 47, 59, 61, 62, 64, 65, 100, 101, 107, 109, 115, 127];
  const betweenOps = between.flatMap((m, i) => [on(m, 0.0137 + i * 0.25, 0.8), off(m, 0.2 + i * 0.25)]);
  return [
    cs('roots', 0.6 + ROOTS.length * 0.25, 0, PLAIN, [[0, roots]]),
    cs('between', 0.6 + between.length * 0.25, 0, PLAIN, [[0, betweenOps]]),
    cs('velocity', 2.4, 0, PLAIN, [
      [0, [on(60, 0.01, 0.2), off(60, 0.3), on(60, 0.5, 0.6), off(60, 0.8), on(60, 1.0, 1), off(60, 1.3), on(64, 1.5, 100), on(67, 1.5, 0), off(64, 1.8), off(67, 1.8)]],
    ]),
    cs('velocity_curve', 1.6, 0, { ...PLAIN, velocityCurve: 3 }, [
      [0, [on(60, 0.01, 0.2), off(60, 0.3), on(60, 0.5, 0.6), off(60, 0.8)]],
      [0.9, [set({ velocityCurve: 0.2 }), on(62, 0.9, 0.3), off(62, 1.2)]],
    ]),
    cs('sustain_release', 5.2, 0, PLAIN, [
      [0, [on(48, 0.01, 0.9), on(55, 0.02, 0.5), off(48, 1.0), off(55, 1.5)]],
      [2.0, [set({ release: 0.01 }), on(60, 2.0, 0.7), off(60, 2.3)]],
      [2.5, [set({ release: 3 }), on(64, 2.5, 0.7), off(64, 2.8)]],
      [4.0, [set({ release: 9 }), on(67, 4.0, 0.7), off(67, 4.2)]],
    ]),
    cs('restrike', 2.2, 0, PLAIN, [
      [0, [on(60, 0.01), on(60, 0.3), on(60, 0.3101), off(60, 0.8), off(61, 0.81)]],
      [1.0, [ar(64, 0.2, 1.0, 0.9), ar(64, 0.001, 1.05, 0.5)]],
      [1.1, [on(64, 1.1, 0.4), off(64, 1.5), ar(67, 0.3, 1.6), ar(67, 0.3, 1.7)]],
    ]),
    cs('overflow', 2.6, 0, { ...PLAIN, polyphony: 4 }, [
      [0, [on(36, 0.01), on(48, 0.05), on(52, 0.1), on(55, 0.15), probe(0.2)]],
      [0.3, [on(60, 0.3), probe(0.31)]],
      [0.5, [off(52, 0.5), on(64, 0.6), on(67, 0.7), probe(0.75)]],
      [0.9, [{ op: 'releaseAll', time: 1.2 }, probe(1.25)]],
      [1.4, [on(40, 1.4, 0.9), on(43, 1.45, 0.9), on(47, 1.5), on(50, 1.55), on(53, 1.6), on(57, 1.65), on(60, 1.7), on(64, 1.75), on(67, 1.8), probe(1.85)]],
      [2.0, [{ op: 'releaseAll', time: 2.0 }, probe(2.6)]],
    ]),
    cs('grand_piano', 3.0, 0, {}, [
      [0, [on(48, 0.01, 0.8), on(55, 0.01, 0.7), on(64, 0.02, 0.6), on(72, 0.5, 0.9), off(72, 0.9), on(76, 1.0, 0.5), off(76, 1.3)]],
      [1.5, [off(48, 1.5), off(55, 1.5), off(64, 1.5), ar(84, 0.4, 1.6, 1)]],
    ]),
    cs('bright_piano', 2.4, 15, {}, [
      [0, [on(45, 0.01, 1), on(57, 0.01, 0.8), on(69, 0.3, 0.7), on(81, 0.6, 0.9), off(45, 1.2), off(57, 1.2), off(69, 1.2), off(81, 1.2)]],
    ]),
    cs('live_set', 2.6, 0, PLAIN, [
      [0, [on(60, 0.01, 0.8), on(67, 0.01, 0.6)]],
      [0.4, [set({ gain: -6 })]],
      [0.8, [set({ character: { tone: 3000 } })]],
      [1.2, [set({ character: { tone: 20000, presence: 4 }, gain: 2 })]],
      [1.6, [set({ polyphony: 2 }), on(72, 1.6), on(76, 1.65), on(79, 1.7), off(60, 1.7), off(67, 1.7), probe(1.75)]],
      [2.0, [off(72, 2.0), off(76, 2.0), off(79, 2.0)]],
    ]),
  ];
}

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-piano.steps.mjs via scripts/ui-snap.mjs (real SampledPiano + Tone.js in headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      evidence: 'per case and channel: blockEvidence (stats.mjs) of the full render, 10 significant digits',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
    },
    buffers: await page.evaluate(`(async () => (${IMPORT}).pianoBuffers(${SR}))()`),
    cases: [],
  };
  log(`decoded ${out.buffers.length} piano buffers`);
  const only = process.env.CASES?.split(',');
  for (const c of cases().filter((k) => !only || only.includes(k.id))) {
    const r = await page.evaluate(`(async () => (${IMPORT}).renderPiano(${JSON.stringify(c)}))()`);
    const chans = r.channels.map((b) => {
      const buf = Buffer.from(b, 'base64');
      return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
    });
    if (chans[0].length !== c.frames) throw new Error(`${c.id}: ${chans[0].length} frames`);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `piano_${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
    }
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    out.cases.push({ ...c, log: r.log, peak: f10(peak), evidence: chans.map((x) => blockEvidence(x).map(f10)) });
    log(`${c.id}: ${c.frames} frames, peak ${peak.toFixed(4)}`);
  }
  mkdirSync(FIXTURES, { recursive: true });
  if (only) return log('CASES set: fixture not written');
  writeFileSync(path.join(FIXTURES, 'piano_reference.json'), JSON.stringify(out) + '\n');
  log(`wrote piano_reference.json (${out.cases.length} cases)`);
}
