import { it } from 'vitest';
import {
  PAD_KEY_HINTS,
  STEP_GRIDS,
  adjustVelocity,
  buildStepMatrix,
  cellAt,
  describeLoopMismatch,
  dragHasSample,
  fileSample,
  formatHz,
  formatSeconds,
  fromLog01,
  hardwareNoteLabel,
  isTypingTarget,
  needsToggle,
  normVelocity,
  padBank,
  padColor,
  padForKeyEvent,
  padLabel,
  padName,
  padRows,
  paintModeFor,
  patternStepAt,
  planChop,
  playheadStep,
  readDroppedSample,
  rowForPad,
  stepBeatsFor,
  stepCount,
  stepsPerBeat,
  toLog01,
  velocityFromPointer,
} from '@/components/beat/beatHelpers';
import { describeStep, nudgeVelocity, stepKeyAction, VELOCITY_KEY_STEP } from '@/components/beat/stepKeys';
import { isScopedKeyTarget } from '@/components/beat/keyScope';
import { chopRanges } from '@/components/beat/chop';
import { planMissingPadFix } from '@/components/beat/missingPackActions';
import { computePeaks, drawPeaks, prepareCanvas } from '@/components/beat/waveform';
import { swingGridLabel } from '@/components/beat/BeatTransport';
import { pendingLoadText } from '@/components/beat/PendingLoadChip';
import { SAMPLE_DRAG_MIME } from '@/components/browser/browserHelpers';
import { CLASSIC_MACHINES } from '@/audio/sampler/classic/machines';
import { FACTORY_VOICE_IDS } from '@/audio/sampler/factoryKit';
import type { PadIndex, PadSettings } from '@/types/sampler';
import { BUFFER_SPECS, N, genBuffer, mulberry32 } from './bufgen';
import { writeFixture } from './fixture';

const SPECIAL = [NaN, Infinity, -Infinity, -0];

it('dumps beat helper vectors', () => {
  const out: Record<string, unknown> = {};

  // --- pad layout -----------------------------------------------------------
  out.padRows = { '4x4': padRows('4x4'), '2x8': padRows('2x8') };
  out.pads = Array.from({ length: 16 }, (_, p) => [p, padBank(p), padLabel(p), padColor(null, p), padColor({ color: 'red' }, p), padColor({ color: '' } as PadSettings, p), padColor({} as PadSettings, p), rowForPad(p), PAD_KEY_HINTS[p]]);
  out.keyHints = PAD_KEY_HINTS;

  const sampleCases: unknown[] = [null, { sample: null }, { sample: { name: 'My Kick', source: { kind: 'factory', voice: 'snare' } } }];
  for (const v of FACTORY_VOICE_IDS) sampleCases.push({ sample: { name: '', source: { kind: 'factory', voice: v } } });
  for (const m of CLASSIC_MACHINES) {
    for (const v of m.voices) sampleCases.push({ sample: { name: '', source: { kind: 'classic', machine: m.id, voice: v.id } } });
    sampleCases.push({ sample: { name: '', source: { kind: 'classic', machine: m.id, voice: 'unknown-voice' } } });
  }
  for (const f of ['a.wav', '.hidden', 'noext', 'x.y.z.WAV', 'kits/Kick 01.wav', 'trailing.', 'Ünïcødé.aif', '']) {
    sampleCases.push({ sample: { name: '', source: { kind: 'pack', packId: 'p1', file: f } } });
    sampleCases.push({ sample: { name: '', source: { kind: 'file', path: `/Users/me/${f}` } } });
    sampleCases.push({ sample: { name: '', source: { kind: 'file', path: f } } });
  }
  out.padName = sampleCases.map((s) => [s, padName(s as PadSettings | null)]);

  const notes = [-25, -13, -12.5, -12, -1, -0.5, 0, 0.4, 0.5, 0.6, 1.5, 2.5, 11, 12, 35.5, 36, 37, 48, 59.49, 60, 61, 69, 100, 127, 128, 1000, 1e6, ...SPECIAL];
  out.hardwareNoteLabel = [[null, hardwareNoteLabel(null)], [undefined, hardwareNoteLabel(undefined)], ...notes.map((n) => [N(n), hardwareNoteLabel(n)])];

  const vel = [];
  for (const y of [-50, 0, 1, 10, 33.3, 50, 66.6, 99, 100, 150, NaN]) for (const top of [0, 10, -5]) for (const h of [100, 37, 1, 0, -10, NaN])
    vel.push([N(y), top, N(h), N(velocityFromPointer(y, top, h))]);
  out.velocityFromPointer = vel;

  // --- computer keyboard pads -------------------------------------------------
  const keys = ['z', 'Z', 'x', 'c', 'v', 'a', 's', 'd', 'f', 'q', 'w', 'e', 'r', 't', 'T', 'y', '1', '2', '3', '4', '5', 'g', '', ' ', 'Enter', 'ArrowUp', 'ß', 'İ', 'Q'];
  const codes = ['KeyZ', 'KeyX', 'KeyT', 'KeyR', 'KeyY', 'Digit1', 'Digit4', 'Digit5', 'Numpad1', 'KeyQ', 'Space', ''];
  const padKeys = [];
  for (const key of keys) for (const code of codes) for (let m = 0; m < 8; m++) {
    const e = { key, code, metaKey: !!(m & 1), ctrlKey: !!(m & 2), altKey: !!(m & 4) };
    padKeys.push([key, code, m, padForKeyEvent(e)]);
  }
  out.padForKeyEvent = padKeys;

  // --- typing targets / key scopes (jsdom) ------------------------------------
  const root = document.createElement('div');
  root.innerHTML = `
    <aside data-keys-scope="browser"><div role="listbox" tabindex="0"><span>x</span></div><button>Add</button><input type="search"></aside>
    <section data-keys-scope=""><p>t</p></section>
    <input id="bpm" /><textarea></textarea><select><option>a</option></select>
    <div role="dialog"><button>ok</button><div><span>deep</span></div></div>
    <div role="alertdialog"><b>x</b></div>
    <div role="menu"><div role="menuitem"></div></div>
    <div role="combobox"><i>c</i></div><div role="textbox"></div><div role="button"></div><div role="Dialog"></div>
    <div contenteditable="true"><span><em>e</em></span><span contenteditable="false"><b>n</b></span></div>
    <div contenteditable=""><span>e2</span></div>
    <div contenteditable="false"><span>ne</span></div>
    <div contenteditable="yes"><span>bad</span></div>
    <button>pad</button><canvas tabindex="0"></canvas><svg><g><rect></rect></g></svg>
    <label><input type="checkbox"></label><form><fieldset><legend>l</legend></fieldset></form>
  `;
  document.body.appendChild(root);
  const chainOf = (el: Element | null) => {
    const chain = [];
    for (let e = el; e; e = e.parentElement) chain.push([e.tagName.toLowerCase(), e.getAttribute('role'), e.getAttribute('contenteditable'), e.getAttribute('data-keys-scope')]);
    return chain;
  };
  const targets: [unknown, boolean, boolean][] = [[null, isTypingTarget(null), isScopedKeyTarget(null)], ['window', isTypingTarget(window), isScopedKeyTarget(window)]];
  for (const el of [document.documentElement, document.body, ...root.querySelectorAll('*')]) targets.push([chainOf(el), isTypingTarget(el), isScopedKeyTarget(el)]);
  out.targets = targets;
  root.remove();

  // --- step grid ------------------------------------------------------------------
  out.stepGrids = STEP_GRIDS.map((g) => [g, stepBeatsFor(g)]);
  const sbs = [0.5, 0.25, 0.125, 1 / 3, 0.1, 1 / 6, 0.75, 2];
  const counts = [];
  for (const bars of [0, 0.01, 0.5, 1, 1.3, 2, 3, 4, 8, 16, 0.125]) for (const sb of sbs) counts.push([bars, sb, stepCount(bars, sb)]);
  out.stepCount = counts;
  out.stepsPerBeat = [...sbs, 0.3, 0.4, 0.6, 3, 10].map((sb) => [sb, stepsPerBeat(sb)]);
  const psa = [];
  for (const b of [-1, -1e-10, 0, 0.1, 0.124999, 0.125, 0.2499999999, 0.25, 0.37, 0.375, 1, 3.8, 3.87, 3.95, 4, 7.99, 100]) for (const sb of [0.125, 0.25, 0.5]) for (const n of [0, 1, 16, 32])
    psa.push([b, sb, n, patternStepAt(b, sb, n)]);
  out.patternStepAt = psa;
  out.normVelocity = [-1, 0, 0.02, 0.5, 1, 1.0001, 2, 63.5, 64, 127, 200, ...SPECIAL].map((v) => [N(v), N(normVelocity(v))]);

  const rnd = mulberry32(42);
  const matrices = [];
  for (let c = 0; c < 48; c++) {
    const n = Math.floor(rnd() * 40);
    const regionStart = [0, 4, 8, 3.75, 16][c % 5];
    const ns = Array.from({ length: n }, () => {
      const r = rnd();
      return {
        note: r < 0.1 ? Math.floor(rnd() * 128) : r < 0.12 ? 36 + rnd() * 16 : 36 + Math.floor(rnd() * 16),
        velocity: [rnd(), Math.floor(rnd() * 128), 0, 1, 127, 0.5][Math.floor(rnd() * 6)],
        startBeat: rnd() < 0.5 ? Math.floor(rnd() * 64) * 0.125 : rnd() * 34 - 1,
        durationBeats: 0.25,
        isDrum: true,
      };
    });
    const patternStart = [0, 4, 8, 3.9999, 8.00005][c % 5] + (c % 7 === 0 ? regionStart : 0);
    const sb = [0.25, 0.5, 0.125][c % 3];
    const steps = [16, 32, 8, 64, 256, 1, 0][c % 7];
    const region = c % 11 === 10 ? null : { startBeat: regionStart, notes: ns };
    const m = buildStepMatrix(region, patternStart, sb, steps);
    const cells: [number, number][] = [];
    m.forEach((v, i) => { if (v !== 0) cells.push([i, N(v) as number]); });
    matrices.push({ region, patternStart, sb, steps, len: m.length, cells });
  }
  out.buildStepMatrix = matrices;

  const ph = [];
  for (const pos of [-1, 0, 3.99, 4, 7.9, 8, 9, 9.1, 11.999, 12, 13]) for (const start of [0, 8]) for (const pb of [4, 8, 0, -1, NaN]) for (const sb of [0.25, 0.5, 1 / 3])
    ph.push([pos, start, N(pb), sb, playheadStep(pos, start, pb, sb)]);
  out.playheadStep = ph;

  const cells = [];
  for (const x of [-1, -0.0001, 0, 5, 19.99, 20, 39, 300, 319.9, 320, 1000]) for (const y of [-1, 0, 5, 9.99, 10, 155, 159.9, 160, 300]) for (const [w, h] of [[20, 10], [0, 10], [20, -1], [12.5, 7.5]]) for (const steps of [16, 32, 1])
    cells.push([x, y, w, h, steps, cellAt(x, y, w, h, steps)]);
  out.cellAt = cells;

  out.paint = [-1, 0, 0.0001, 0.5, 1, NaN].map((v) => [N(v), paintModeFor(v), needsToggle('draw', v), needsToggle('erase', v)]);

  const adj = [];
  for (const s of [0, 0.05, 0.3, 0.5, 0.8, 0.95, 1]) for (const d of [-1000, -60, -12, -1, 0, 0.5, 1, 6, 12, 60, 120, 1000]) {
    adj.push([s, d, null, adjustVelocity(s, d)]);
    adj.push([s, d, 40, adjustVelocity(s, d, 40)]);
  }
  out.adjustVelocity = adj;

  // --- sample drops -----------------------------------------------------------------
  const drops: unknown[] = [];
  const payloads = [
    '', '{broken', 'null', '5', '"str"', '[1,2]', '{}', '{"path":""}', '{"path":5}',
    JSON.stringify({ path: '/s/Kick 01.wav', name: 'Kick 01' }),
    JSON.stringify({ path: '/s/Kick 01.wav' }),
    JSON.stringify({ path: '/s/Kick 01.wav', name: '' }),
    JSON.stringify({ path: '/s/Kick 01.wav', name: 7 }),
    JSON.stringify({ path: 'C:\\Samples\\Snare.aif' }),
    JSON.stringify({ path: '/dir/' }),
    JSON.stringify({ path: 'plain' }),
    JSON.stringify({ path: '/a/.hidden' }),
    ' {"path":"/ws.wav"} ',
  ];
  const fileSets = [[], ['notes.txt', 'Snare.WAV'], ['a.wav'], ['x.mp3', 'y.wav'], ['.wav', 'b.'], ['c.flac', 'd.ogg', 'e.m4a', 'f.aiff']];
  for (const payload of payloads) for (const files of fileSets) for (const resolver of ['none', 'empty', 'prefix', 'onlyWav']) {
    const types = [...(payload ? [SAMPLE_DRAG_MIME] : []), ...(files.length ? ['Files'] : [])];
    const dt = { types, getData: (t: string) => (t === SAMPLE_DRAG_MIME ? payload : ''), files: files.map((f) => new File([''], f)) };
    const get = resolver === 'none' ? undefined : resolver === 'empty' ? () => '' : resolver === 'prefix' ? (f: File) => `/drop/${f.name}` : (f: File) => (f.name.endsWith('.wav') ? `/w/${f.name}` : '');
    const r = readDroppedSample(dt, get);
    drops.push([payload, files, resolver, dragHasSample(dt), r, r ? fileSample(r) : null]);
  }
  out.drops = drops;
  out.dragHasSample = [[null, dragHasSample(null)], [['text/plain'], dragHasSample({ types: ['text/plain'] })], [[], dragHasSample({ types: [] })], [['Files'], dragHasSample({ types: ['Files'] })]];

  // --- chop ---------------------------------------------------------------------------
  const ranges = Array.from({ length: 20 }, (_, i) => ({ startSec: i * 0.5, endSec: i * 0.5 + 0.5 }));
  const plans = [];
  for (let from = 0; from <= 16; from++) for (const n of [0, 1, 3, 8, 16, 20]) plans.push([from, n, planChop(from as PadIndex, ranges.slice(0, n))]);
  out.planChop = plans;
  out.chopRangesInput = ranges;

  out.bufferSpecs = BUFFER_SPECS;
  const chops = [];
  const trims = [
    { startSec: 0, endSec: null }, { startSec: 0.5, endSec: null }, { startSec: 1, endSec: 3 }, { startSec: 0.2, endSec: 0.9 },
    { startSec: -1, endSec: 100 }, { startSec: 0.5, endSec: 0.5 }, { startSec: 0.7, endSec: 0.3 }, { startSec: NaN, endSec: null }, { startSec: 0.123, endSec: 1.777 },
  ];
  const modes = [
    { kind: 'equal', count: 4 }, { kind: 'equal', count: 8 }, { kind: 'equal', count: 16 },
    { kind: 'transients', sensitivity: 0 }, { kind: 'transients', sensitivity: 0.25 }, { kind: 'transients', sensitivity: 0.5 }, { kind: 'transients', sensitivity: 0.8 }, { kind: 'transients', sensitivity: 1 }, { kind: 'transients', sensitivity: 2 },
  ] as const;
  BUFFER_SPECS.forEach((spec, b) => {
    const buf = genBuffer(spec);
    for (const trim of trims) for (const mode of modes) for (const from of [0, 4, 12, 15, 16, 20]) {
      chops.push([b, { startSec: N(trim.startSec), endSec: trim.endSec }, mode, from, chopRanges(buf, trim, mode, from as PadIndex)]);
    }
  });
  out.chopRanges = chops;

  out.missingFix = [[0, 5], [0, 15], [3, 7, 8, 12], [16, 20], []].flatMap((pads) =>
    (['clear', 'builtin'] as const).map((fix) => [pads, fix, planMissingPadFix(pads as PadIndex[], fix)]));

  // --- log knobs / formatting -----------------------------------------------------------
  const logs = [];
  for (const [min, max] of [[20, 20000], [0.005, 4], [1, 2], [0.01, 100]]) {
    for (const t of [-0.5, 0, 0.001, 0.1, 0.25, 0.37, 0.5, 0.75, 0.9, 0.999, 1, 1.5, NaN]) logs.push(['from', min, max, N(t), N(fromLog01(t, min, max))]);
    for (const v of [-1, 0, min / 2, min, min * 1.5, Math.sqrt(min * max), max * 0.9, max, max * 2, NaN]) logs.push(['to', min, max, N(v), N(toLog01(v, min, max))]);
  }
  out.log = logs;

  const secs = [0, -0, 0.0004, 0.0005, 0.0015, 0.0025, 0.001, 0.005, 0.0123, 0.05, 0.0995, 0.1, 0.2345, 0.5, 0.9994, 0.9995, 0.99951, 0.9999, 1, 1.005, 1.015, 1.125, 1.235, 1.2345, 2, 2.675, 3.14159, 4, 9.995, 10, 59.999, 100.125, 1234.5678, 1e6, 1e20, 1e21, 1.5e22, -0.0004, -0.0005, -0.5, -1.5, ...SPECIAL];
  for (let i = 0; i < 200; i++) secs.push(Math.pow(10, -4 + i * 0.04) * (1 + (i % 7) / 13));
  out.formatSeconds = secs.map((s) => [N(s), formatSeconds(s)]);
  const hzs = [0, 0.4, 0.5, 1.5, 2.5, 19.5, 20, 99.5, 440, 999.4, 999.5, 999.9, 1000, 1049, 1050, 1149.99, 1250, 1350, 1450, 2550, 9949, 9950, 9999, 10000, 10499, 10500, 11500, 12500, 19999, 20000, 1e7, 1e24, -5, -1000, ...SPECIAL];
  for (let i = 0; i < 200; i++) hzs.push(Math.pow(10, 0.5 + i * 0.025) * (1 + (i % 5) / 17));
  for (let i = 0; i <= 100; i++) hzs.push(fromLog01(i / 100, 20, 20000));
  out.formatHz = hzs.map((h) => [N(h), formatHz(h)]);

  // --- loop mismatch -----------------------------------------------------------------------
  const lm = [];
  const loops = [null, undefined, { startBeat: 0, endBeat: 8 }, { startBeat: 0, endBeat: 16 }, { startBeat: 0, endBeat: 6 }, { startBeat: 4, endBeat: 12 }, { startBeat: 16, endBeat: 24 },
    { startBeat: 0.00005, endBeat: 8.00005 }, { startBeat: 0, endBeat: 4 }, { startBeat: 0, endBeat: 4.00009 }, { startBeat: 0, endBeat: 4.0002 }, { startBeat: 0, endBeat: 3.333333 },
    { startBeat: 2, endBeat: 2 }, { startBeat: 8, endBeat: 0 }, { startBeat: 0, endBeat: 0.125 }, { startBeat: 0, endBeat: 1e22 }, { startBeat: 0, endBeat: 6.005 }, { startBeat: 0, endBeat: 6.015 }, { startBeat: 1, endBeat: 3.5 }];
  for (const loop of loops) for (const loopEnabled of [true, false]) for (const playing of [true, false]) for (const patternStartBeat of [0, 16, 4]) for (const bars of [1, 2, 4, 0.5, 1.5])
    lm.push([loop ?? null, loopEnabled, playing, patternStartBeat, bars, describeLoopMismatch({ loop, loopEnabled, playing, patternStartBeat, bars })]);
  out.describeLoopMismatch = lm;

  // --- step keys -------------------------------------------------------------------------------
  const sk = [];
  const skKeys = ['ArrowRight', 'ArrowLeft', 'ArrowUp', 'ArrowDown', 'Home', 'End', ' ', 'Enter', '+', '=', ']', '-', '_', '[', 'z', 'Tab', 'Escape', 'Delete'];
  const skCells = [{ pad: 0, step: 0 }, { pad: 5, step: 6 }, { pad: 15, step: 31 }, { pad: 13, step: 8 }, { pad: 2, step: 3 }];
  for (const key of skKeys) for (let m = 0; m < 16; m++) for (const cell of skCells) for (const [steps, pb] of [[16, 4], [32, 4], [8, 2], [24, 3], [1, 4], [0, 0], [64, 8]]) {
    const e = { key, shiftKey: !!(m & 1), altKey: !!(m & 2), metaKey: !!(m & 4), ctrlKey: !!(m & 8) };
    sk.push([key, m, cell, steps, pb, stepKeyAction(e, cell, steps, pb)]);
  }
  out.stepKeyAction = sk;
  out.velocityKeyStep = VELOCITY_KEY_STEP;
  const nv = [];
  for (const v of [0, 0.05, 0.1, 0.15, 0.5, 0.8, 0.85, 0.9, 0.95, 1, 1.2]) for (const d of [VELOCITY_KEY_STEP, -VELOCITY_KEY_STEP, 0, 0.005, -0.005]) nv.push([v, d, nudgeVelocity(v, d)]);
  out.nudgeVelocity = nv;
  const ds = [];
  for (const name of ['Kick', 'Pad A1', '', 'Snare, 2']) for (const step of [0, 4, 15, 255]) for (const v of [0, -0.1, 0.004, 0.005, 0.015, 0.5, 0.8, 0.805, 0.995, 1, NaN]) ds.push([name, step, N(v), describeStep(name, step, v)]);
  out.describeStep = ds;

  // --- waveform ----------------------------------------------------------------------------------
  const peaks = [];
  BUFFER_SPECS.forEach((spec, b) => {
    const buf = genBuffer(spec);
    for (const cols of [1, 2, 3, 7.9, 64, 100, 257, 0, -3]) {
      const p = computePeaks(buf, cols);
      const calls: number[][] = [];
      const ctx = { fillStyle: '', fillRect: (...a: number[]) => void calls.push(a) } as unknown as CanvasRenderingContext2D;
      drawPeaks(ctx, p, { x: 3, y: 2, w: 120, h: 40 }, 'red');
      peaks.push([b, cols, Array.from(p.min), Array.from(p.max), calls]);
    }
  });
  out.peaks = peaks;
  const flat = { min: Float32Array.from([0, -0.001, -1, -0.5, 0]), max: Float32Array.from([0, 0.001, 1, 0.25, -0.1]) };
  const rects = [];
  for (const box of [{ x: 0, y: 0, w: 100, h: 50 }, { x: 5, y: 7, w: 3, h: 1 }, { x: 0, y: 0, w: 2, h: 0.5 }]) {
    const calls: number[][] = [];
    drawPeaks({ fillStyle: '', fillRect: (...a: number[]) => void calls.push(a) } as unknown as CanvasRenderingContext2D, flat, box, 'x');
    rects.push([box, calls]);
  }
  out.peakRectsFlat = { peaks: { min: Array.from(flat.min), max: Array.from(flat.max) }, rects };

  const canv = [];
  for (const dpr of [1, 2, 1.5, 0.5, 0, 3]) for (const [w, h] of [[100, 50], [0, 0], [10.25, 3.75], [0.2, 0.2], [333.3, 77.7]]) {
    (window as unknown as { devicePixelRatio: number }).devicePixelRatio = dpr;
    let transform: number[] = [];
    const ctx = { setTransform: (...a: number[]) => void (transform = a), clearRect: () => undefined };
    const canvas = { width: 300, height: 150, getContext: () => ctx } as unknown as HTMLCanvasElement;
    prepareCanvas(canvas, w, h);
    canv.push([dpr, w, h, canvas.width, canvas.height, transform[0]]);
  }
  (window as unknown as { devicePixelRatio: number }).devicePixelRatio = 1;
  out.prepareCanvas = canv;

  // --- tsx helpers ----------------------------------------------------------------------------
  out.swingGridLabel = [['1/8', swingGridLabel('1/8')], ['1/16', swingGridLabel('1/16')]];
  out.pendingLoadText = ['TR-808 Recordings', '', 'pack-id'].flatMap((packName) => [true, false].map((w) => [packName, w, pendingLoadText({ key: 'k', packId: 'p', kitId: 'k', packName, waitingForRecording: w })]));

  writeFixture('beat_helpers', out);
});
