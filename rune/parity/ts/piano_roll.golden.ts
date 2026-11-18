import { it } from 'vitest';
import type { RegionNote } from '@/types/arrangement';
import type { RowLayout } from '@/types/pianoRollEditor';
import * as P from '@/utils/pianoRollHelpers';
import { writeFixture } from './fixture';

/** JSON can't hold NaN/±Infinity: encode them as strings (the Rust side decodes them). */
function enc(v: unknown): unknown {
  if (typeof v === 'number') return Number.isFinite(v) ? v : String(v);
  if (Array.isArray(v)) return v.map(enc);
  if (v && typeof v === 'object') return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, enc(x)]));
  return v;
}

const N = (note: number, startBeat: number, durationBeats = 1, velocity = 0.8, isDrum = false): RegionNote => ({
  note,
  startBeat,
  durationBeats,
  velocity,
  isDrum,
});
const D = (note: number, startBeat: number, durationBeats = 0.25, velocity = 0.9) => N(note, startBeat, durationBeats, velocity, true);
const one = (n: RegionNote) => [n.note, n.velocity, n.startBeat, n.durationBeats, n.isDrum ? 1 : 0];
const flat = (notes: readonly RegionNote[]) => notes.map(one);
/** Same-length edit: only the notes that changed, as [index, ...note]. Rust re-applies onto the input. */
const diff = (before: readonly RegionNote[], after: readonly RegionNote[]) => {
  if (after.length !== before.length) throw new Error('length changed');
  const out: unknown[] = [];
  after.forEach((n, i) => {
    const a = one(n);
    const b = one(before[i]);
    if (a.some((x, k) => !Object.is(x, b[k]))) out.push([i, ...a]);
  });
  return out;
};
/** Appending edit: the untouched prefix is checked here, only the tail is stored. */
const tail = (before: readonly RegionNote[], after: readonly RegionNote[]) => {
  if (diff(before, after.slice(0, before.length)).length) throw new Error('prefix changed');
  return flat(after.slice(before.length));
};

// Deterministic LCG
let seed = 12345;
const rnd = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648;
  return seed / 2147483648;
};

const NOTE_SETS: RegionNote[][] = [
  [N(60, 0, 1), N(64, 1, 1), N(67, 2, 0.5)],
  [N(60, 0, 2), N(60, 0, 2), N(60, 1, 0.5, 0.3), N(61, 0.1, 0.01), N(60, 1.9, 0.3, 1)],
  [D(36, 0), D(38, 0.5), D(42, 0), D(42, 0.25, 0.25, 0.4), D(46, 1), D(37, 1.5), D(49, 3.75), D(51, 2, 0.5)],
  [N(127, 0.5, 1), N(0, 3.3, 2, 0.01), N(126, 7.99, 0.333), N(64, 0.0001, 0.1), N(62, -0.5, 1), N(60.5, 1, 1), N(64, 0.1 + 0.2, 1 / 3)],
  Array.from({ length: 24 }, () =>
    N(48 + Math.floor(rnd() * 36), Math.round(rnd() * 64) * 0.0625 + (rnd() < 0.3 ? 0.013 : 0), 0.05 + Math.round(rnd() * 16) / 8, Math.round(rnd() * 100) / 100),
  ),
  [],
];
const ALL = (k: number) => NOTE_SETS[k].map((_, i) => i);
const INDEX_SETS: number[][] = [[], [0], [1], [0, 1], [1, 2], [2, 0, 2], [0, 1, 2, 3], [3, 1], [99], [0, 99], ALL(4), [5, 3, 7, 1, 11, 23, 2]];

const LAYOUT_SPECS: (['piano', number, number] | ['drum', number[]])[] = [
  ['piano', 0, 127],
  ['piano', 36, 72],
  ['drum', []],
  ['drum', [36, 37, 38, 100, 20, 37, 60.5]],
];
const LAYOUTS: RowLayout[] = LAYOUT_SPECS.map((s) => (s[0] === 'piano' ? P.createPianoLayout(s[1], s[2]) : P.createDrumLayout(s[1])));
/** (noteSet, layout) pairs used by the layout-dependent edits. */
const PAIRS: [number, number][] = [
  [0, 0], [0, 1], [0, 2], [1, 0], [1, 1], [2, 2], [2, 3], [2, 0], [3, 0], [3, 1], [3, 3], [4, 0], [4, 1], [5, 0], [5, 2],
];

const BEATS: number[] = [];
for (let i = 0; i <= 50; i++) BEATS.push(i * 0.1371 - 0.6);
BEATS.push(0, 0.25, 0.5, 1, 1 / 3, 2 / 3, 0.2499, 0.2501, 0.125, 1.9999999, 3.99, 4, 7.5, 8, 15.99, -2, -0.000001, 0.1 + 0.2, 100.37);
const SNAPS = [...new Set([...P.SNAP_OPTIONS, ...P.NOTE_LENGTH_OPTIONS].map((o) => o.beats)), 0.3, -1];
const OFFSETS = [0, 3.5, 1 / 3];
const PXS = [0.5, 1, 6, 8, 12, 30, 64, 120, 640];

it('dumps piano roll helper vectors', () => {
  const consts = {
    snapOptions: P.SNAP_OPTIONS.map((o) => [o.id, o.label, o.beats]),
    noteLengthOptions: P.NOTE_LENGTH_OPTIONS.map((o) => [o.id, o.label, o.beats]),
    drumRows: P.DRUM_ROWS.map((d) => [d.midiNote, d.name, d.fullName, d.color]),
    numbers: {
      BEATS_PER_BAR: P.BEATS_PER_BAR, PR_NOTE_MIN: P.PR_NOTE_MIN, PR_NOTE_MAX: P.PR_NOTE_MAX, PR_NOTE_COUNT: P.PR_NOTE_COUNT,
      PR_DRUM_COUNT: P.PR_DRUM_COUNT, DEFAULT_NOTE_VELOCITY: P.DEFAULT_NOTE_VELOCITY, DEFAULT_NOTE_DURATION: P.DEFAULT_NOTE_DURATION,
      DEFAULT_DRUM_DURATION: P.DEFAULT_DRUM_DURATION, MIN_NOTE_LENGTH: P.MIN_NOTE_LENGTH, TICKS_PER_BEAT: P.TICKS_PER_BEAT,
      MIN_VELOCITY: P.MIN_VELOCITY, PR_MIN_PX_PER_BEAT: P.PR_MIN_PX_PER_BEAT, PR_MAX_PX_PER_BEAT: P.PR_MAX_PX_PER_BEAT,
      PR_DEFAULT_PX_PER_BEAT: P.PR_DEFAULT_PX_PER_BEAT, PR_MIN_ROW_HEIGHT: P.PR_MIN_ROW_HEIGHT, PR_MAX_ROW_HEIGHT: P.PR_MAX_ROW_HEIGHT,
      PR_DEFAULT_ROW_HEIGHT: P.PR_DEFAULT_ROW_HEIGHT, PR_DEFAULT_DRUM_ROW_HEIGHT: P.PR_DEFAULT_DRUM_ROW_HEIGHT,
    },
  };

  // ---- scalar maths ----
  const XS = [...BEATS, -1, 0.5 / 960, 1.5 / 960, -0.5 / 960, 1e-7, 0.49999999999999994, 2.5, -2.5, 1e6 + 0.3, NaN, Infinity, -Infinity, 1 / 127, 0.004, 1.2];
  const roundBeat = XS.map((x) => [x, P.roundBeat(x), P.clampVelocity(x), P.velocityToMidi(x), P.nudgeStep(x)]);
  const isMultipleOf: unknown[] = [];
  for (const v of [0, 1, 4, 8, 12, 0.5, 1 / 3, 2 / 3, 4.00001, 4.001, -4, 7.9999, NaN]) for (const s of [0, -1, 4, 1, 0.25, 1 / 3, 1 / 12, NaN]) isMultipleOf.push([v, s, P.isMultipleOf(v, s)]);
  const clamp: unknown[] = [];
  for (const v of [-1, 0, 0.5, 1, 2, NaN]) for (const [lo, hi] of [[0, 1], [1, 0], [0.5, 0.5], [-Infinity, 0]]) clamp.push([v, lo, hi, P.clamp(v, lo, hi)]);
  const FB = [...XS, 0.0625, 0.1875, 0.375, 0.625, 1.25, 2.5, 3, 5, 6, 12, 16, 64, 4.5, 1 / 6, 1 / 12, 1 / 24, 5 / 12, 0.37, 0.3, 0.1, 7 / 48, 1 / 48, 1 / 96, 1 / 64, 3 / 64, 1 / 128, 1e-5, 4.000001, 3.99999999, 1e21, 123456.789];
  const formatBeats = FB.map((b) => [b, P.formatBeats(b), P.formatPosition(b)]);
  const findSnapOption = FB.map((b) => [b, P.findSnapOption(b)?.id ?? null, P.findSnapOption(b, P.NOTE_LENGTH_OPTIONS)?.id ?? null]);
  const coords: unknown[] = [];
  for (const v of [-13, 0, 7.5, 100, 333.3]) for (const s of [1, 7, 16, 64, 0.5]) coords.push([v, s, P.beatToX(v, s), P.xToBeat(v, s), P.rowToY(v, s), P.yToRow(v, s), P.beatToGridX(v, s), P.gridXToBeat(v, s)]);
  const zoomScroll: unknown[] = [];
  for (const s of [0, 100, 2500]) for (const c of [0, 50, 400]) for (const o of [0, -1, 6, 64, 640]) for (const n of [6, 64, 640]) zoomScroll.push([s, c, o, n, P.zoomScroll(s, c, o, n)]);
  const zoomFromWheel: unknown[] = [];
  for (const cur of [6, 64, 640]) for (const dy of [-1000, -100, -3, 0, 3, 100, 10000]) for (const sens of [0.0025, 0.01]) zoomFromWheel.push([cur, dy, 6, 640, sens, P.zoomFromWheel(cur, dy, 6, 640, sens)]);
  const slider: unknown[] = [];
  for (const t of [-0.5, 0, 0.1, 0.25, 0.5, 0.77, 1, 1.5]) for (const [mn, mx] of [[6, 640], [7, 40], [1, 2]]) slider.push([t, mn, mx, P.sliderToZoom(t, mn, mx), P.zoomToSlider(P.sliderToZoom(t, mn, mx), mn, mx)]);
  for (const v of [1, 6, 64, 100, 640, 1000]) slider.push([v, 6, 640, P.sliderToZoom(v / 640, 6, 640), P.zoomToSlider(v, 6, 640)]);
  const snap: unknown[] = [];
  for (const b of BEATS) for (const s of SNAPS) for (const o of OFFSETS) snap.push([b, s, o, P.snapBeat(b, s, o), P.snapFloor(b, s, o), P.snapCeil(b, s, o)]);
  const snapLegacy = BEATS.map((b) => [b, P.snapBeatPR(b, 0.25), P.snapBeatFloor(b, 1 / 3)]);
  const snappedMoveDelta: unknown[] = [];
  const drawSpan: unknown[] = [];
  for (const a of [0, 0.1, 1, 2.75, 1 / 3]) for (const d of [-3, -0.6, -0.1, 0, 0.05, 0.5, 1.37, 4.2]) for (const s of SNAPS) for (const o of [0, 2, 1 / 3]) {
    snappedMoveDelta.push([a, d, s, o, P.snappedMoveDelta(a, d, s, o)]);
    const ds = P.drawSpan(a, a + d, s, o);
    drawSpan.push([a, a + d, s, o, ds.startBeat, ds.durationBeats]);
  }
  const gridSubdivision: unknown[] = [];
  for (const s of SNAPS) for (const px of [...PXS, 0, 0.001]) for (const ms of [6, 3, 20]) gridSubdivision.push([s, px, ms, P.gridSubdivision(s, px, ms)]);
  const gridLines: unknown[] = [];
  const ruler: unknown[] = [];
  for (const [s, e] of [[0, 4], [0, 1], [-1.3, 9.7], [2, 2], [5, 1], [0.37, 33.1], [0, 64]]) for (const px of [...PXS, 0, 44, 45]) for (const o of [0, 2, 4, 1 / 3]) {
    if ((e - s) * px > 1000) continue; // keep the fixture small: wide ranges only at low zoom
    for (const sn of [0, 0.25, 1 / 3, 0.125, 4, 1 / 12]) for (const ms of sn === 0.25 ? [6, 10] : [6]) {
      gridLines.push([s, e, px, sn, o, ms, P.computeGridLines(s, e, px, sn, o, ms).flatMap((l) => [l.beat, l.level])]);
    }
    for (const mp of [44, 20]) ruler.push([s, e, px, o, mp, P.computeRulerLabels(s, e, px, o, mp).map((l) => [l.beat, l.label, l.major])]);
  }
  const velocityBucket: unknown[] = [];
  for (const v of [-1, 0, 0.004, 0.1, 0.5, 0.55, 0.999, 1, 2, NaN]) for (const b of [0, 1, 2, 8, 16, 127]) velocityBucket.push([v, b, P.velocityBucket(v, b)]);
  const followScroll: unknown[] = [];
  for (const p of [-10, 0, 10, 399, 400, 500, 900, 950, 990, 1350, 1401, 5000]) for (const s of [0, 400]) for (const w of [0, -5, 1000]) for (const m of [0.1, 0, 0.5]) followScroll.push([p, s, w, m, P.followScroll(p, s, w, m)]);

  // ---- layouts ----
  const layouts = LAYOUTS.map((L, li) => {
    const pitches = [-1, 0, 20, 35, 36, 37, 38, 39, 42, 45, 46, 49, 51, 59, 60, 60.5, 61, 71.5, 72, 73, 100, 127, 128];
    const rows = [-5, -1, -0.5, 0, 0.4, 0.5, 1, 2, 7, 7.5, 8, 9, 10, 11, 12, 36, 36.6, 37, 100, 127, 128, 200];
    const intRows = [-2, -1, 0, 1, 2, 3, 7, 8, 9, 10, 11, 12, 36, 37, 67, 127, 128];
    return {
      spec: LAYOUT_SPECS[li],
      kind: L.kind,
      rowCount: L.rowCount,
      pitches: (L as RowLayout & { pitches?: number[] }).pitches ?? null,
      p2r: pitches.map((p) => [p, L.pitchToRow(p)]),
      r2p: rows.map((r) => [r, L.rowToPitch(r), P.isShadedRow(L, r)]),
      labels: intRows.map((r) => [r, L.rowLabel(r), L.rowColor?.(r) ?? null]),
    };
  });

  // ---- hit-testing & selection ----
  const HIT_OPTS: (P.HitTestOptions & { selectedArr?: number[] })[] = [
    {},
    { resizable: false },
    { edgePx: 12, minWidthPx: 10 },
    { selectedArr: [0] },
    { selectedArr: [1, 3], edgePx: 2 },
  ];
  const hitOpts = HIT_OPTS.map((o) => ({ edgePx: o.edgePx ?? 6, minWidthPx: o.minWidthPx ?? 4, resizable: o.resizable ?? true, selected: o.selectedArr ?? null }));
  const hit: unknown[] = [];
  const noteVisualWidth: unknown[] = [];
  for (const [si, li] of PAIRS) {
    const notes = NOTE_SETS[si];
    const L = LAYOUTS[li];
    const rowsToTry = new Set<number>([-1, 0, L.rowCount - 1, L.rowCount]);
    for (const n of notes) rowsToTry.add(L.pitchToRow(n.note));
    for (const [px, rh] of [[40, 10], [200, 7]]) for (const row of rowsToTry) {
      if (!Number.isInteger(row)) continue;
      for (const y of [row * rh + 1, row * rh + rh / 2]) {
        for (let xi = -2; xi <= 50; xi += 2) {
          const x = (xi * px) / 8 + 0.37;
          HIT_OPTS.forEach((o, oi) => {
            const opts: P.HitTestOptions = { ...o, selected: o.selectedArr ? new Set(o.selectedArr) : undefined };
            const h = P.hitTestNote(notes, x, y, L, px, rh, opts);
            if (h || (xi % 14 === 0 && oi === 0)) hit.push([si, li, oi, x, y, px, rh, h ? h.index : -1, h ? h.edge : '']);
          });
        }
      }
    }
    if (li === 0 || li === 2) notes.forEach((n, i) => { for (const px of [0.5, 40, 640]) for (const mw of [4, 0, 30]) noteVisualWidth.push([si, i, px, mw, P.noteVisualWidth(n, px, mw)]); });
  }
  const notesInRect: unknown[] = [];
  for (const [si, li] of PAIRS) {
    const L = LAYOUTS[li];
    for (const [bA, bB] of [[0.5, 1.5], [2.5, 0.5], [0, 0], [-1, 100], [1, 1.000001], [3, 2.9999995], [1.95, 2.05]])
      for (const [rA, rB] of [[0, L.rowCount], [67, 67], [63, 60], [0, 8], [8, 0], [-3, -1], [66, 67], [66.5, 66.5]])
        notesInRect.push([si, li, bA, bB, rA, rB, P.notesInRect(NOTE_SETS[si], bA, bB, rA, rB, L)]);
  }
  const click: unknown[] = [];
  for (const cur of [[], [1, 2], [2, 1], [3, 1, 2], [5]]) for (const idx of [1, 2, 3, 5]) for (const t of [false, true]) click.push([cur, idx, t, [...P.applyClickSelection(new Set(cur), idx, t)]]);

  // ---- edits ----
  const clampMove: unknown[] = [];
  const move: unknown[] = [];
  const transpose: unknown[] = [];
  const appendCopies: unknown[] = [];
  const DELTAS = [[1, -2], [-3, -5], [0, 0], [0.3, 1], [-0.5, 13], [2.25, -1], [0.0004, 0.4], [-100, 200]];
  for (const [si, li] of PAIRS) {
    const notes = NOTE_SETS[si];
    const L = LAYOUTS[li];
    INDEX_SETS.forEach((ix, xi) => {
      for (const [dB, dR] of DELTAS) {
        const c = P.clampMoveDelta(notes, ix, dB, dR, L);
        clampMove.push([si, li, xi, dB, dR, c.dBeat, c.dRow]);
        move.push([si, li, xi, dB, dR, diff(notes, P.moveNotes(notes, ix, dB, dR, L))]);
      }
      for (const st of [12, -1, 1, 0, -200, 0.5]) transpose.push([si, li, xi, st, diff(notes, P.transposeNotes(notes, ix, st, L))]);
      for (const [dB, dR] of [[0, 0], [1, -12], [-5, 3], [0.37, 1]]) {
        const e = P.appendCopies(notes, ix, dB, dR, L);
        appendCopies.push([si, li, xi, dB, dR, tail(notes, e.notes), e.selection]);
      }
    });
  }
  const resize: unknown[] = [];
  const changeLengths: unknown[] = [];
  const del: unknown[] = [];
  const selectionSpan: unknown[] = [];
  const duplicate: unknown[] = [];
  const copy: unknown[] = [];
  const appendNoLayout: unknown[] = [];
  const setVel: unknown[] = [];
  const offsetVel: unknown[] = [];
  NOTE_SETS.forEach((notes, si) => {
    INDEX_SETS.forEach((ix, xi) => {
      if ([1, 3, 5, 6, 8, 10].includes(xi)) for (const edge of ['start', 'end'] as const) for (const anchor of [0, 3, 99]) for (const raw of [-5, -0.4, 0.07, 0.6, 3.3]) for (const [s, o] of [[0, 0], [0.25, 0], [0.25, 0.5], [1 / 3, 0.5], [4, 0]])
        resize.push([si, xi, edge, anchor, raw, s, o, diff(notes, P.resizeNotes(notes, ix, edge, anchor, raw, s, o))]);
      for (const d of [-2, -0.25, 0, 0.25, 1 / 3]) for (const ml of [P.MIN_NOTE_LENGTH, 0.5]) changeLengths.push([si, xi, d, ml, diff(notes, P.changeLengths(notes, ix, d, ml))]);
      del.push([si, xi, flat(P.deleteNotes(notes, ix))]);
      const sp = P.selectionSpan(notes, ix);
      selectionSpan.push([si, xi, sp ? [sp.start, sp.end] : null]);
      for (const s of [0, 0.25, 1 / 3, 4, 0.3]) {
        const e = P.duplicateNotes(notes, ix, s);
        duplicate.push([si, xi, s, tail(notes, e.notes), e.selection]);
      }
      copy.push([si, xi, flat(P.copyNotes(notes, ix))]);
      const e = P.appendCopies(notes, ix, -0.7, 5);
      appendNoLayout.push([si, xi, tail(notes, e.notes), e.selection]);
      for (const v of [2, 0.5, 0, -1]) setVel.push([si, xi, v, diff(notes, P.setVelocities(notes, ix, v))]);
      for (const d of [-1, 0.25, -0.05]) offsetVel.push([si, xi, d, diff(notes, P.offsetVelocities(notes, ix, d))]);
    });
  });
  const paste: unknown[] = [];
  for (const si of [0, 2, 5]) NOTE_SETS.forEach((clip, ci) => {
    for (const at of [8, 0, -3, 1 / 3]) for (const dr of [undefined, true, false]) {
      const e = P.pasteNotes(NOTE_SETS[si], clip, at, dr);
      paste.push([si, ci, at, dr ?? null, tail(NOTE_SETS[si], e.notes), e.selection]);
    }
  });
  const Q_OPTS: { indices: number[] | null; ends: boolean; offset: number; strength: number }[] = [];
  for (const indices of [null, [1], [], [0, 2, 5]]) for (const ends of [false, true]) for (const offset of [0, 0.5]) for (const strength of [1, 0.5, 0]) Q_OPTS.push({ indices, ends, offset, strength });
  const quantize: unknown[] = [];
  NOTE_SETS.forEach((notes, si) => {
    for (const s of [0, -1, 0.25, 1 / 3, 1, 4, 0.125]) Q_OPTS.forEach((q, qi) => {
      quantize.push([si, s, qi, diff(notes, P.quantizeNotes(notes, s, { ...q, indices: q.indices ?? undefined }))]);
    });
  });
  const ramp: unknown[] = [];
  const notesAtX: unknown[] = [];
  NOTE_SETS.forEach((notes, si) => {
    for (const cand of [null, [1], [0, 2, 3], []]) {
      for (const [bA, vA, bB, vB] of [[0, 0.2, 3, 0.8], [3, 0.8, 0, 0.2], [1, 0.5, 1, 0.9], [0.5, -1, 2.5, 2], [-1, 1, 10, 0]])
        ramp.push([si, cand, bA, vA, bB, vB, diff(notes, P.rampVelocities(notes, cand, bA, vA, bB, vB))]);
      for (const x of [-5, 0, 3, 41, 80, 100, 160]) for (const [px, tol] of [[40, 5], [40, 0], [8, 2], [64, 20]])
        notesAtX.push([si, x, px, tol, cand, P.notesAtX(notes, x, px, tol, cand ? new Set(cand) : null)]);
    }
  });

  // ---- view ----
  const view: unknown[] = [];
  for (const [si, li] of PAIRS) {
    const notes = NOTE_SETS[si];
    const L = LAYOUTS[li];
    const pr = P.pitchRange(notes);
    const centers: unknown[] = [];
    for (const rh of [7, 10, 16, 40]) for (const vh of [0, 200, 340, 5000]) centers.push([rh, vh, P.centerScrollTop(notes, L, rh, vh)]);
    const fits: unknown[] = [];
    for (const len of [0, 4, 16, 100]) for (const [vw, vh] of [[816, 340], [100, 100], [10, 2000], [100000, 50]]) {
      const f = P.fitView(notes, L, len, vw, vh);
      fits.push([len, vw, vh, [f.pxPerBeat, f.rowHeight, f.scrollLeft, f.scrollTop]]);
    }
    const reqs = [0, 1, 3, 4, 8, 7.9999999, 100].map((cur) => [cur, P.requiredRegionLength(notes, cur)]);
    view.push([si, li, pr ? [pr.lo, pr.hi] : null, reqs, centers, fits]);
  }

  writeFixture('piano_roll', enc({
    consts,
    noteSets: NOTE_SETS.map(flat),
    indexSets: INDEX_SETS,
    layouts,
    pairs: PAIRS,
    roundBeat, isMultipleOf, clamp, formatBeats, findSnapOption, coords, zoomScroll, zoomFromWheel, slider,
    snap, snapLegacy, snappedMoveDelta, drawSpan, gridSubdivision, gridLines, ruler, velocityBucket, followScroll,
    hitOpts, hit, noteVisualWidth, notesInRect, click,
    clampMove, move, transpose, appendCopies, resize, changeLengths, del, selectionSpan, duplicate, copy, appendNoLayout,
    setVel, offsetVel, paste, qOpts: Q_OPTS, quantize, ramp, notesAtX, view,
  }));
});
