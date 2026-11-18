/**
 * Chromium reference capture for the SamplerKit voice engine: the app's real
 * `src/audio/sampler/SamplerKit.ts` with real Tone.js in headless Chromium, rendered offline at
 * 48 kHz (plus two cases at 44.1 and 96 kHz) through a scripted battery: pad gain, pan (mono and
 * stereo samples), pitch, trim, reverse, decay, filter cutoff, gate vs one-shot releases, choke
 * groups, voice stealing, stopAll, setKit during playback (param ramps, filter insert / ramp /
 * bypass, sample change), velocities and cancelIfPending.
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-sampler.steps.mjs \
 *        --out /tmp/parity-sampler.png
 *   (optional) PARITY_DEBUG_DIR=/some/dir → also writes each render as raw f32 (L then R),
 *              `<id>.f32`, for `PARITY_DEBUG_DIR=/some/dir cargo test -p mdaw-engine
 *              sampler_parity_debug -- --ignored --nocapture`.
 *
 * Page side: ./capture-sampler-page.ts. Output: rune/parity/fixtures/sampler_reference.json —
 * the scripts themselves (the Rust test replays them) and, per case and channel, the full-length
 * block evidence of ./stats.mjs (`blockEvidence`, 10 significant digits).
 * Rust: rune/crates/mdaw-engine/src/sampler/parity_tests.rs.
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { BLOCK, PROJ, PROJ_SEED, blockEvidence } from './stats.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIXTURES = path.join(HERE, '..', 'fixtures');
const DEBUG_DIR = process.env.PARITY_DEBUG_DIR;
const IMPORT = "await import('/rune/parity/chromium/capture-sampler-page.ts')";
const f10 = (x) => Number(x.toPrecision(10));

const BUFFERS = {
  kick: { kind: 'classic', machine: 'tr808', voice: 'kick' },
  snare: { kind: 'classic', machine: 'tr808', voice: 'snare' },
  chh: { kind: 'classic', machine: 'tr808', voice: 'chh' },
  ohh: { kind: 'classic', machine: 'tr808', voice: 'ohh' },
  clap: { kind: 'classic', machine: 'tr909', voice: 'clap' },
  synthS: { kind: 'synth', sampleRate: 44100, frames: 44100, channels: 2, seed: 4242 },
  synthM: { kind: 'synth', sampleRate: 48000, frames: 38400, channels: 1, seed: 777 },
  synthL: { kind: 'synth', sampleRate: 32000, frames: 64000, channels: 1, seed: 99 },
};

const DEF = { sample: null, gainDb: 0, pan: 0, pitch: 0, startSec: 0, endSec: null, decay: 0.05, filterCutoff: 20000, reverse: false, mode: 'oneshot', chokeGroup: 0 };

/** KitState JSON from `{ padIndex: { buf, ...settings } }`. */
function kit(pads) {
  const arr = Array.from({ length: 16 }, () => ({ ...DEF }));
  for (const [i, p] of Object.entries(pads)) {
    const { buf, ...rest } = p;
    arr[i] = { ...DEF, ...rest, sample: buf ? { source: { kind: 'file', path: `/parity/${buf}` }, name: buf } : null };
  }
  return { id: 'parity', name: 'parity', pads: arr };
}

const trig = (pad, time, vel = 1) => ({ op: 'trigger', pad, vel, time });
const rel = (pad, time) => ({ op: 'release', pad, time });

/** Build a case: `steps` maps seconds (rounded to a quantum) to ops; 0 = before rendering. */
function cs(id, seconds, pads, steps, sr = 48000) {
  const q = (t) => Math.round((t * sr) / 128) * 128;
  const merged = new Map();
  for (const [t, ops] of steps) {
    const f = q(t);
    merged.set(f, [...(merged.get(f) ?? []), ...ops]);
  }
  const s = [...merged.entries()].sort((a, b) => a[0] - b[0]).map(([frame, ops]) => ({ frame, ops }));
  return { id, sr, frames: q(seconds), buffers: BUFFERS, kit: kit(pads), steps: s };
}

/** setKit op: the case's pads with `edit` applied. */
function setKitOp(pads, edit) {
  const next = structuredClone(pads);
  for (const [i, p] of Object.entries(edit)) next[i] = { ...(next[i] ?? {}), ...p };
  return [{ op: 'setKit', kit: kit(next) }, next];
}

function liveSetKit() {
  let pads = { 0: { buf: 'synthL', decay: 0.3, endSec: 1.5 }, 1: { buf: 'snare' } };
  const steps = [[0, [trig(0, 0.01), trig(1, 0.3)]]];
  const at = (t, edit, extra = []) => {
    const [op, next] = setKitOp(pads, edit);
    pads = next;
    steps.push([t, [op, ...extra]]);
  };
  at(0.2, { 0: { gainDb: -6 } });
  at(0.35, { 0: { pan: 0.8 } });
  at(0.5, { 0: { filterCutoff: 1000 } });
  at(0.6, { 1: { pitch: -5 } }, [trig(1, 0.7)]);
  at(0.65, { 0: { filterCutoff: 4000 } });
  at(0.8, { 0: { filterCutoff: 20000 } });
  at(0.95, { 0: { filterCutoff: 500, pan: -0.3 } });
  at(1.1, { 0: { buf: 'synthM', endSec: null, decay: 0.1 } });
  steps.push([1.2, [trig(0, 1.2 + 0.0101)]]);
  return cs('setkit_live', 1.6, { 0: { buf: 'synthL', decay: 0.3, endSec: 1.5 }, 1: { buf: 'snare' } }, steps);
}

function stereoRamps() {
  let pads = { 0: { buf: 'synthS', pan: -0.5 } };
  const steps = [[0, [trig(0, 0.01)]]];
  for (const [t, edit] of [
    [0.2, { 0: { pan: 0.9 } }],
    [0.4, { 0: { pan: 0 } }],
    [0.6, { 0: { gainDb: 6 } }],
    [0.61, { 0: { gainDb: -3, pan: -1 } }],
  ]) {
    const [op, next] = setKitOp(pads, edit);
    pads = next;
    steps.push([t, [op]]);
  }
  return cs('stereo_ramps', 1.1, { 0: { buf: 'synthS', pan: -0.5 } }, steps);
}

function cases() {
  const synthLHits = Array.from({ length: 10 }, (_, i) => trig(0, 0.01 + i * 0.03));
  const pending8 = Array.from({ length: 8 }, (_, i) => trig(1, 0.6 + i * 0.01));
  const live9 = Array.from({ length: 9 }, (_, i) => trig(2, 0.9 + i * 0.004, 0.7));
  const mix = {
    0: { buf: 'kick', pitch: -3, filterCutoff: 3000, pan: -0.4 },
    1: { buf: 'synthS', reverse: true, startSec: 0.1, endSec: 0.5, gainDb: -4 },
    2: { buf: 'chh', chokeGroup: 1, pan: 0.5 },
    3: { buf: 'ohh', chokeGroup: 1, decay: 0.3 },
    4: { buf: 'synthM', mode: 'gate', decay: 0.1, startSec: 0.02, pitch: 2 },
  };
  const mixSteps = [
    [0, [trig(0, 0.0123456), trig(3, 0.05), trig(2, 0.3), trig(1, 0.35, 90), trig(4, 0.5)]],
    [0.6, [rel(4, 0.6123)]],
    [0.7, [trig(3, 0.71), trig(0, 0.72, 0.6)]],
    [0.9, [{ op: 'stopAll' }]],
  ];
  return [
    cs('gain', 1.4, { 0: { buf: 'kick' }, 1: { buf: 'kick', gainDb: -12 }, 2: { buf: 'kick', gainDb: 6 }, 3: { buf: 'snare', gainDb: -60 } }, [
      [0, [trig(0, 0.01), trig(1, 0.3001), trig(2, 0.62), trig(3, 0.9)]],
    ]),
    cs('pan', 1.5, {
      0: { buf: 'snare', pan: -1 }, 1: { buf: 'snare', pan: 1 }, 2: { buf: 'snare', pan: 0.37 },
      3: { buf: 'synthS', pan: -1, endSec: 0.25 }, 4: { buf: 'synthS', pan: 0.6, endSec: 0.25 }, 5: { buf: 'synthS', endSec: 0.25 },
    }, [[0, [trig(0, 0.01), trig(1, 0.2), trig(2, 0.4), trig(3, 0.6), trig(4, 0.9), trig(5, 1.2)]]]),
    cs('pitch', 1.6, {
      0: { buf: 'kick', pitch: 7 }, 1: { buf: 'kick', pitch: -12, endSec: 0.5, decay: 0.2 }, 2: { buf: 'synthM', pitch: 24 },
      3: { buf: 'synthL', pitch: -24, endSec: 0.1 }, 4: { buf: 'chh', pitch: 3.5 },
    }, [[0, [trig(0, 0.0123456), trig(1, 0.3), trig(2, 0.75), trig(3, 0.9), trig(4, 1.3)]]]),
    cs('trim', 1.3, {
      0: { buf: 'synthM', startSec: 0.1, endSec: 0.3 }, 1: { buf: 'synthM', endSec: 0.2 }, 2: { buf: 'synthL', startSec: 0.25, decay: 0.3 },
      3: { buf: 'kick', startSec: 0.05, endSec: 0.0504 }, 4: { buf: 'kick', endSec: 2 },
    }, [[0, [trig(0, 0.00731), trig(1, 0.25), trig(3, 0.4), trig(2, 0.5), trig(4, 0.9)]]]),
    cs('reverse', 1.5, {
      0: { buf: 'kick', reverse: true }, 1: { buf: 'synthS', reverse: true, startSec: 0.2, endSec: 0.6 }, 2: { buf: 'synthM', reverse: true, pitch: 5, startSec: 0.05 },
    }, [[0, [trig(0, 0.01), trig(1, 0.6), trig(2, 1.05)]]]),
    cs('decay', 1.6, {
      0: { buf: 'synthM', decay: 0.005 }, 1: { buf: 'synthM', decay: 0.5 }, 2: { buf: 'synthL', decay: 4, endSec: 0.6 }, 3: { buf: 'kick', decay: 0.7 },
    }, [[0, [trig(0, 0.01), trig(1, 0.2), trig(2, 0.6), trig(3, 0.9)]]]),
    cs('filter', 1.5, {
      0: { buf: 'snare', filterCutoff: 200 }, 1: { buf: 'snare', filterCutoff: 2000 }, 2: { buf: 'snare', filterCutoff: 12000 },
      3: { buf: 'snare', filterCutoff: 20000 }, 4: { buf: 'synthS', filterCutoff: 800, endSec: 0.3 }, 5: { buf: 'kick', filterCutoff: 20, gainDb: 12 },
    }, [[0, [trig(0, 0.01), trig(1, 0.2), trig(2, 0.4), trig(3, 0.6), trig(4, 0.8), trig(5, 1.1)]]]),
    cs('gate', 1.4, {
      0: { buf: 'synthL', mode: 'gate', decay: 0.05, endSec: 0.3 }, 1: { buf: 'synthM', endSec: 0.3 },
      2: { buf: 'synthM', mode: 'gate', startSec: 0.05, decay: 0.2 }, 3: { buf: 'synthM', mode: 'gate', endSec: 0.1, decay: 0.02 },
    }, [
      [0, [trig(0, 0.01), rel(0, 0.2), trig(0, 0.4), rel(0, 0.68), trig(0, 0.8), rel(0, 0.75), trig(1, 0.05), rel(1, 0.1), trig(3, 1.2), rel(3, 1.35)]],
      [0.5, [trig(2, 0.5 + 0.0021)]],
      [0.6, [rel(2, 0.6)]],
      [1.0, [trig(2, 1.0123), rel(2, 1.0131)]],
    ]),
    cs('choke', 1.5, {
      0: { buf: 'ohh', chokeGroup: 1 }, 1: { buf: 'chh', chokeGroup: 1 }, 2: { buf: 'snare', chokeGroup: 2 }, 3: { buf: 'clap', chokeGroup: 2 },
    }, [
      [0, [trig(0, 0.01), trig(1, 0.2), trig(0, 0.4), trig(0, 0.45), trig(1, 0.5001), trig(0, 0.9), trig(1, 0.6), trig(2, 1.0), trig(3, 1.05)]],
      [1.1, [trig(2, 1.1), trig(2, 1.1)]],
    ]),
    cs('choke_fading', 0.6, {
      0: { buf: 'ohh', chokeGroup: 1, mode: 'gate', decay: 0.2 }, 1: { buf: 'chh', chokeGroup: 1 },
    }, [[0, [trig(0, 0.01), rel(0, 0.1), trig(1, 0.15), trig(1, 0.152)]]]),
    cs('steal', 1.4, { 0: { buf: 'synthL' }, 1: { buf: 'synthM', endSec: 0.3 }, 2: { buf: 'snare' } }, [
      [0, [...synthLHits, ...pending8, trig(1, 0.5)]],
      [0.9, live9],
    ]),
    cs('stop_all', 1.2, { 0: { buf: 'kick' }, 1: { buf: 'synthL' }, 2: { buf: 'synthS' } }, [
      [0, [trig(0, 0.01), trig(1, 0.05), trig(2, 0.1), trig(0, 0.9)]],
      [0.3, [{ op: 'stopAll' }]],
      [0.5, [trig(0, 0.5), trig(1, 0.52), { op: 'stopAll', time: 0.6123 }]],
    ]),
    liveSetKit(),
    stereoRamps(),
    cs('velocity', 1.6, { 0: { buf: 'kick', endSec: 0.2 } }, [
      [0, [trig(0, 0.01, 127), trig(0, 0.25, 64), trig(0, 0.5, 1), trig(0, 0.75, 0.5), trig(0, 0.9, 0), trig(0, 1.0, 200), trig(0, 1.25, null)]],
    ]),
    cs('cancel_pending', 1.0, { 0: { buf: 'synthM' }, 1: { buf: 'snare', mode: 'gate' } }, [
      [0, [trig(0, 0.01), trig(0, 0.5), trig(1, 0.6)]],
      [0.2, [{ op: 'cancel', handle: 0 }, { op: 'cancel', handle: 1 }]],
      [0.62, [{ op: 'cancel', handle: 2 }]],
    ]),
    cs('gate_release_past_end', 0.4, { 0: { buf: 'synthM', mode: 'gate', endSec: 0.2, decay: 0.005 } }, [
      [0, [trig(0, 0.0), rel(0, 0.5), { op: 'stopAll', time: 0.1 }]],
    ]),
    cs('mix_44k', 1.2, mix, mixSteps, 44100),
    cs('mix_96k', 1.2, mix, mixSteps, 96000),
  ];
}

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-sampler.steps.mjs via scripts/ui-snap.mjs (real SamplerKit + Tone.js in headless Chromium)',
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
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    const { buffers: _b, ...script } = c;
    out.cases.push({ ...script, log: r.log, peak: f10(peak), evidence: chans.map((x) => blockEvidence(x).map(f10)) });
    log(`${c.id}: ${c.frames} frames, peak ${peak.toFixed(4)}, log ${JSON.stringify(r.log)}`);
  }
  out.buffers = BUFFERS;
  mkdirSync(FIXTURES, { recursive: true });
  if (only) return log('CASES set: fixture not written');
  writeFileSync(path.join(FIXTURES, 'sampler_reference.json'), JSON.stringify(out) + '\n');
  log(`wrote sampler_reference.json (${out.cases.length} cases)`);
}
