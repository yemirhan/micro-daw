import type { BeatState, RegionNote, Track } from '@/types/arrangement';
import {
  PAD_COUNT,
  noteToPad,
  padToNote,
  type KitState,
  type LoopRecordSettings,
  type QuantizeGrid,
  type SwingGrid,
} from '@/types/sampler';
import {
  FACTORY_VOICE_GM_NOTE,
  createFactoryKit,
  factoryPadForGmNote,
  normalizeKit,
} from '@/audio/sampler/factoryKit';
import { getClassicVoice } from '@/audio/sampler/classic/machines';

export function beatToPx(beat: number, pxPerBeat: number): number {
  return beat * pxPerBeat;
}

export function pxToBeat(px: number, pxPerBeat: number): number {
  return px / pxPerBeat;
}

export function snapBeat(beat: number, snapValue: number): number {
  if (snapValue <= 0) return beat;
  return Math.round(beat / snapValue) * snapValue;
}

// ---------------------------------------------------------------------------
// Beat mode: loop recorder settings, step grid, quantize + swing
// ---------------------------------------------------------------------------

/** Positions closer than this (in beats) are the same step. */
export const STEP_EPSILON = 1e-4;
const MAX_SWING = 0.75;
/** Velocity of a step drawn in the step grid (0..1). */
export const DEFAULT_STEP_VELOCITY = 0.8;

export const DEFAULT_LOOP_SETTINGS: LoopRecordSettings = {
  bars: 2,
  quantize: '1/16',
  swing: 0,
  swingGrid: '1/16',
  countInBars: 1,
  metronome: true,
  overdub: true,
};

const GRID_STEP_BEATS: Record<QuantizeGrid, number | null> = {
  off: null,
  '1/4': 1,
  '1/8': 0.5,
  '1/8T': 1 / 3,
  '1/16': 0.25,
  '1/16T': 1 / 6,
  '1/32': 0.125,
};

const LOOP_BARS = [1, 2, 4, 8] as const;
const COUNT_IN_BARS = [0, 1, 2] as const;

/** Grid step length in beats (quarter notes); null for 'off'. */
export function gridStepBeats(grid: QuantizeGrid): number | null {
  return GRID_STEP_BEATS[grid] ?? null;
}

/** Triplet steps (1/3, 1/6 beat...) are never swung. */
function isTripletStep(stepBeats: number): boolean {
  const l = Math.log2(stepBeats);
  return Math.abs(l - Math.round(l)) > 1e-6;
}

const clampSwing = (swing: number) => (Number.isFinite(swing) ? Math.min(MAX_SWING, Math.max(0, swing)) : 0);
/** Strip float noise (1/3-beat grids) so equal steps compare equal. */
const clean = (beat: number) => Math.round(beat * 1e9) / 1e9;

/**
 * Step-grid cell of a stored (straight) pattern note at `beat` (pattern-relative).
 * Nearest step, clamped to [0, steps): a note in the last half-step of the
 * pattern stays visible (and toggleable) on the last step instead of mapping
 * to the non-existent step `steps`.
 */
export function patternStepAt(beat: number, stepBeats: number, steps: number): number {
  const step = Math.round(clean(Math.max(0, beat) / stepBeats));
  return Math.min(Math.max(0, steps - 1), Math.max(0, step));
}

/**
 * Snap a played beat to the quantize grid. The nearest step is chosen by where
 * each step SOUNDS (swing delays the odd steps of `swingGrid`: what the player
 * heard), but the STRAIGHT step position is returned: patterns are stored
 * straight and swung at playback. `swingGrid` defaults to the quantize grid
 * itself; triplet grids are never swung. 'off' returns the beat as is.
 */
export function quantizeBeat(
  beat: number,
  grid: QuantizeGrid,
  swing = 0,
  swingGrid: SwingGrid | QuantizeGrid = grid,
): number {
  const step = gridStepBeats(grid);
  if (step === null) return beat;
  const k0 = Math.round(clean(beat / step));
  // The largest swing delay can move a step this many grid steps late.
  const span = Math.ceil((MAX_SWING * swingStepBeats(swingGrid)) / step) + 1;
  let best = k0;
  let bestDist = Number.POSITIVE_INFINITY;
  for (let k = k0 - span; k <= k0 + span; k++) {
    const straight = clean(k * step);
    const dist = Math.abs(beat - (straight + swingOffsetBeats(straight, swingGrid, swing)));
    if (dist < bestDist - 1e-9) {
      best = k;
      bestDist = dist;
    }
  }
  return clean(best * step);
}

/** Step length (beats) the swing applies to (1/16 for 'off'). */
function swingStepBeats(grid: SwingGrid | QuantizeGrid): number {
  return gridStepBeats(grid) ?? 0.25;
}

/** Swing grid of loop settings (older data without one: 1/16). */
export function loopSwingGrid(loop: Partial<Pick<LoopRecordSettings, 'swingGrid'>>): SwingGrid {
  return loop.swingGrid === '1/8' ? '1/8' : '1/16';
}

/**
 * Playback delay (beats) for a drum note stored straight at absolute `beat`:
 * notes exactly on an odd step of the swing grid are delayed by
 * `swing × step`; everything else (even steps, off-grid notes, triplet grids)
 * plays as stored.
 */
export function swingOffsetBeats(beat: number, grid: SwingGrid | QuantizeGrid, swing: number): number {
  const s = clampSwing(swing);
  if (s === 0) return 0;
  const step = swingStepBeats(grid);
  if (isTripletStep(step)) return 0;
  const idx = Math.round(beat / step);
  if (Math.abs(idx * step - beat) > STEP_EPSILON || Math.abs(idx % 2) !== 1) return 0;
  return clean(s * step);
}

/** Shortest recorded gate length (beats): a 1/64 note. */
const MIN_GATE_BEATS = 1 / 16;

/**
 * Held length (beats) of a gate pad recorded in a loop, in MUSICAL time.
 * `onBeat`/`offBeat` are transport positions (from ticks, so tempo changes
 * during the hold are accounted for) inside the looping range, i.e. the off
 * position may have wrapped. `wrappedBeats` is what the transport jumped back
 * between them: the loop length of every loop wrap that happened while the pad
 * was held (counted from the transport's loop events, never estimated from
 * elapsed seconds × BPM, which is wrong after a tempo change).
 * Result in [1/64 note, loop length].
 */
export function loopGateBeats(onBeat: number, offBeat: number, wrappedBeats: number, loopBeats: number): number {
  let beats = offBeat - onBeat + (Number.isFinite(wrappedBeats) ? Math.max(0, wrappedBeats) : 0);
  if (!Number.isFinite(beats)) return MIN_GATE_BEATS;
  // A wrap the clock had not reported yet (or no tick clock): positions only move forward.
  if (beats < 0 && loopBeats > 0) beats += Math.ceil(-beats / loopBeats) * loopBeats;
  if (loopBeats > 0) beats = Math.min(loopBeats, beats);
  return Math.max(MIN_GATE_BEATS, beats);
}

/** Wrap a beat into [0, length). */
export function wrapBeat(beat: number, length: number): number {
  if (length <= 0) return beat;
  const w = ((beat % length) + length) % length;
  return length - w < STEP_EPSILON ? 0 : clean(w);
}

/**
 * Add a drum hit to a note list. A hit on the same pad at the same position
 * (within STEP_EPSILON) as an existing note is merged: the louder one wins.
 * Returns a new array (the input is not modified).
 */
export function mergeDrumHit(notes: RegionNote[], hit: RegionNote): RegionNote[] {
  const i = notes.findIndex((n) => n.note === hit.note && Math.abs(n.startBeat - hit.startBeat) < STEP_EPSILON);
  if (i < 0) return [...notes, hit];
  if (notes[i].velocity >= hit.velocity) return notes.slice();
  const out = notes.slice();
  out[i] = { ...notes[i], velocity: hit.velocity };
  return out;
}

export function normalizeLoopSettings(raw: unknown, base: LoopRecordSettings = DEFAULT_LOOP_SETTINGS): LoopRecordSettings {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Partial<Record<keyof LoopRecordSettings, unknown>>;
  const bars = LOOP_BARS.find((b) => b === r.bars) ?? base.bars;
  const countInBars = COUNT_IN_BARS.find((b) => b === r.countInBars) ?? base.countInBars;
  const quantize =
    typeof r.quantize === 'string' && r.quantize in GRID_STEP_BEATS ? (r.quantize as QuantizeGrid) : base.quantize;
  let swingGrid: SwingGrid;
  if (r.swingGrid === '1/8' || r.swingGrid === '1/16') swingGrid = r.swingGrid;
  else if (r.swingGrid === undefined && typeof r.quantize === 'string')
    // Older data swung the quantize grid: keep a 1/8 swing sounding the same.
    swingGrid = r.quantize === '1/8' ? '1/8' : '1/16';
  else swingGrid = loopSwingGrid(base);
  return {
    bars,
    quantize,
    swing: typeof r.swing === 'number' ? clampSwing(r.swing) : base.swing,
    swingGrid,
    countInBars,
    metronome: typeof r.metronome === 'boolean' ? r.metronome : base.metronome,
    overdub: typeof r.overdub === 'boolean' ? r.overdub : base.overdub,
  };
}

/** Valid BeatState for the given tracks (unknown/non-drum active ids fall back to the first drum track). */
export function normalizeBeatState(raw: unknown, tracks: Track[]): BeatState {
  const r = (raw && typeof raw === 'object' ? raw : {}) as Partial<Record<keyof BeatState, unknown>>;
  const drumIds = tracks.filter((t) => t.instrument.type === 'drums').map((t) => t.id);
  const active =
    typeof r.activeDrumTrackId === 'string' && drumIds.includes(r.activeDrumTrackId)
      ? r.activeDrumTrackId
      : drumIds[0] ?? null;
  const pad = typeof r.selectedPad === 'number' ? Math.round(r.selectedPad) : 0;
  return {
    activeDrumTrackId: active,
    loop: normalizeLoopSettings(r.loop),
    selectedPad: pad >= 0 && pad < PAD_COUNT ? pad : 0,
  };
}

// ---------------------------------------------------------------------------
// Drum note migration (General MIDI → pad notes) and export mapping
// ---------------------------------------------------------------------------

/**
 * Map General MIDI drum notes (legacy drum regions/templates) to factory-kit
 * pad notes. Notes without a factory equivalent are dropped; hits that land on
 * the same pad at the same time are merged (louder wins).
 */
export function migrateGmDrumNotes(notes: RegionNote[]): { notes: RegionNote[]; dropped: number } {
  let out: RegionNote[] = [];
  let dropped = 0;
  for (const n of notes) {
    const pad = factoryPadForGmNote(n.note);
    if (pad === null) {
      dropped++;
      continue;
    }
    out = mergeDrumHit(out, { ...n, note: padToNote(pad), isDrum: true });
  }
  return { notes: out, dropped };
}

/**
 * Bring a drum track up to the kit model. A drum track without a kit is a
 * legacy track: its GM drum notes are mapped to factory pads and it gets the
 * factory kit. Tracks that already have a kit only get their kit validated.
 * Returns the number of legacy notes that had no pad and were dropped.
 */
export function migrateDrumTrack(track: Track): number {
  if (track.instrument.type !== 'drums') return 0;
  if (track.instrument.kit) {
    track.instrument.kit = normalizeKit(track.instrument.kit);
    return 0;
  }
  let dropped = 0;
  for (const region of track.regions) {
    if (region.audio) continue;
    const res = migrateGmDrumNotes(region.notes);
    region.notes = res.notes;
    dropped += res.dropped;
  }
  track.instrument.kit = createFactoryKit();
  return dropped;
}

/**
 * MIDI export note for a drum-track pad note: the GM note of the factory or
 * classic voice on that pad when there is one, else the pad note itself (36..51).
 */
export function drumNoteToGm(note: number, kit: KitState | undefined): number {
  const pad = noteToPad(note);
  if (pad === null) return note;
  const source = kit?.pads[pad]?.sample?.source;
  if (source?.kind === 'factory') return FACTORY_VOICE_GM_NOTE[source.voice];
  if (source?.kind === 'classic') return getClassicVoice(source.machine, source.voice)?.gm ?? note;
  return note;
}
