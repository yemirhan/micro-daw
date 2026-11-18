/**
 * Chromium reference capture for the instrument output chain: the app's real
 * `src/audio/CharacterChain.ts` with real Tone.js in headless Chromium, rendered offline at
 * 48 kHz, fed by the deterministic stereo test signal of capture-piano-page.ts (`testSignal`:
 * saw sweep + triangle chord, mirrored in Rust). Cases: every distinct preset character
 * (presets.ts), each stage alone (drive, presence ±, tone, tremolo, chorus), a mono input, and
 * live `update()`s (stage insert / remove, ramps of every parameter, gain).
 *
 *   node scripts/ui-snap.mjs --steps rune/parity/chromium/capture-character.steps.mjs --out /tmp/c.png
 *   (optional) PARITY_DEBUG_DIR=/dir → also writes each render as raw f32 (L then R), char_<id>.f32.
 *
 * Output: rune/parity/fixtures/character_reference.json. Rust: rune/crates/mdaw-engine/src/character/parity_tests.rs.
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
const SECONDS = 1.2;
const f10 = (x) => Number(x.toPrecision(10));
const q = (t) => Math.round((t * SR) / 128) * 128;

const DEFAULTS = { drive: 0, chorus: 0, tremolo: 0, tremoloRate: 4.5, tone: 20000, presence: 0 };

function cs(id, character, gain, extra = {}) {
  return { id, sr: SR, frames: q(extra.seconds ?? SECONDS), character, gain, mono: !!extra.mono, steps: extra.steps ?? [] };
}

function cases(presets) {
  const out = [
    cs('drive', { drive: 0.35 }, 0),
    cs('drive_full', { drive: 1 }, -6),
    cs('presence_up', { presence: 5 }, 0),
    cs('presence_down', { presence: -2 }, 0),
    cs('tone', { tone: 2000 }, 0),
    cs('tone_clamped', { tone: 150 }, 6),
    cs('tremolo', { tremolo: 0.35, tremoloRate: 4.5 }, 0),
    cs('chorus', { chorus: 0.3 }, 0),
    cs('chorus_full', { chorus: 1 }, 0),
    cs('gain_only', {}, -4.5),
    cs('mono_all', { drive: 0.2, presence: 3, tone: 9000, tremolo: 0.4, tremoloRate: 7, chorus: 0.5 }, 1, { mono: true }),
    cs('live', { chorus: 0.2 }, 0, {
      seconds: 2.4,
      steps: [
        { frame: q(0.3), ops: [{ op: 'update', character: { chorus: 0.8, drive: 0.3 }, gain: -3 }] },
        { frame: q(0.6), ops: [{ op: 'update', character: { chorus: 0.8, drive: 0.6, tone: 4000, presence: -3 }, gain: -3 }] },
        { frame: q(0.9), ops: [{ op: 'update', character: { chorus: 0.8, drive: 0.6, tone: 1000, presence: 6, tremolo: 0.5, tremoloRate: 3 }, gain: 2 }] },
        { frame: q(1.3), ops: [{ op: 'update', character: { chorus: 0, drive: 0.6, tone: 18000, presence: 6, tremolo: 0.9, tremoloRate: 9 }, gain: 2 }] },
        { frame: q(1.7), ops: [{ op: 'update', character: { tremolo: 0.9, tremoloRate: 9, tone: 25000 }, gain: 0 }] },
        { frame: q(2.0), ops: [{ op: 'update', character: {}, gain: 0 }] },
      ],
    }),
  ];
  const seen = new Set();
  for (const p of presets) {
    const c = { ...DEFAULTS, ...p.character };
    const key = JSON.stringify(c);
    if (key === JSON.stringify(DEFAULTS) || seen.has(key)) continue;
    seen.add(key);
    out.push({ ...cs(`preset_${p.id}`, c, p.gain), presets: presets.filter((x) => JSON.stringify({ ...DEFAULTS, ...x.character }) === key).map((x) => x.id) });
  }
  return out;
}

export default async function steps(page, { log, url }) {
  await page.goto(`${new URL('rune/parity/chromium/blank.html', url)}`, { waitUntil: 'domcontentloaded' });
  const presets = await page.evaluate(`(async () => (${IMPORT}).presetCharacters())()`);
  const out = {
    meta: {
      source: 'rune/parity/chromium/capture-character.steps.mjs via scripts/ui-snap.mjs (real CharacterChain + Tone.js in headless Chromium)',
      userAgent: await page.evaluate('navigator.userAgent'),
      evidence: 'per case and channel: blockEvidence (stats.mjs) of the full render, 10 significant digits',
      block: BLOCK,
      proj: PROJ,
      projSeed: PROJ_SEED,
    },
    cases: [],
  };
  const only = process.env.CASES?.split(',');
  for (const c of cases(presets).filter((k) => !only || only.includes(k.id))) {
    const r = await page.evaluate(`(async () => (${IMPORT}).renderCharacter(${JSON.stringify(c)}))()`);
    const chans = r.channels.map((b) => {
      const buf = Buffer.from(b, 'base64');
      return new Float32Array(buf.buffer.slice(buf.byteOffset, buf.byteOffset + buf.byteLength));
    });
    if (chans[0].length !== c.frames) throw new Error(`${c.id}: ${chans[0].length} frames`);
    if (DEBUG_DIR) {
      mkdirSync(DEBUG_DIR, { recursive: true });
      writeFileSync(path.join(DEBUG_DIR, `char_${c.id}.f32`), Buffer.concat(chans.map((x) => Buffer.from(x.buffer))));
    }
    const peak = Math.max(...chans.map((x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0)));
    out.cases.push({ ...c, peak: f10(peak), evidence: chans.map((x) => blockEvidence(x).map(f10)) });
    log(`${c.id}: ${c.frames} frames, peak ${peak.toFixed(4)}`);
  }
  mkdirSync(FIXTURES, { recursive: true });
  if (only) return log('CASES set: fixture not written');
  writeFileSync(path.join(FIXTURES, 'character_reference.json'), JSON.stringify(out) + '\n');
  log(`wrote character_reference.json (${out.cases.length} cases)`);
}
