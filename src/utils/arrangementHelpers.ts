import type { BeatState, RegionNote, Track } from '@/types/arrangement';
import type { RecordedEvent } from '@/types/recorder';
import {
  PAD_COUNT,
  noteToPad,
  padToNote,
  type KitState,
  type LoopRecordSettings,
  type QuantizeGrid,
} from '@/types/sampler';
import {
  FACTORY_VOICE_GM_NOTE,
  createFactoryKit,
  factoryPadForGmNote,
  normalizeKit,
} from '@/audio/sampler/factoryKit';

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

export function msToBeats(ms: number, bpm: number): number {
  return (ms / 60000) * bpm;
}

export function beatsToMs(beats: number, bpm: number): number {
  return (beats / bpm) * 60000;
}

export function formatBeatPosition(beat: number): string {
  const bar = Math.floor(beat / 4) + 1;
  const beatInBar = Math.floor(beat % 4) + 1;
  return `${bar}.${beatInBar}`;
}

/** Convert legacy ms-based recording events to beat-based RegionNotes */
export function recordingEventsToRegionNotes(events: RecordedEvent[], bpm: number): RegionNote[] {
  const notes: RegionNote[] = [];
  const openNotes = new Map<number, { startBeat: number; velocity: number; isDrum: boolean }>();

  for (const event of events) {
    const beatTime = msToBeats(event.time, bpm);

    if (event.type === 'noteOn') {
      openNotes.set(event.note, {
        startBeat: beatTime,
        velocity: event.velocity,
        isDrum: event.isDrum,
      });
    } else {
      const open = openNotes.get(event.note);
      if (open) {
        notes.push({
          note: event.note,
          velocity: open.velocity,
          startBeat: open.startBeat,
          durationBeats: Math.max(0.125, beatTime - open.startBeat),
          isDrum: open.isDrum,
        });
        openNotes.delete(event.note);
      }
    }
  }

  // Close any remaining open notes with default duration
  for (const [note, open] of openNotes) {
    notes.push({
      note,
      velocity: open.velocity,
      startBeat: open.startBeat,
      durationBeats: open.isDrum ? 0.25 : 0.5,
      isDrum: open.isDrum,
    });
  }

  return notes;
}

// ---------------------------------------------------------------------------
// Beat mode: loop recorder settings, step grid, quantize + swing
// ---------------------------------------------------------------------------

/** Positions closer than this (in beats) are the same step. */
export const STEP_EPSILON = 1e-4;
export const MAX_SWING = 0.75;
/** Velocity of a step drawn in the step grid (0..1). */
export const DEFAULT_STEP_VELOCITY = 0.8;

export const DEFAULT_LOOP_SETTINGS: LoopRecordSettings = {
  bars: 2,
  quantize: '1/16',
  swing: 0,
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
export function isTripletStep(stepBeats: number): boolean {
  const l = Math.log2(stepBeats);
  return Math.abs(l - Math.round(l)) > 1e-6;
}

const clampSwing = (swing: number) => (Number.isFinite(swing) ? Math.min(MAX_SWING, Math.max(0, swing)) : 0);
/** Strip float noise (1/3-beat grids) so equal steps compare equal. */
const clean = (beat: number) => Math.round(beat * 1e9) / 1e9;

/**
 * Beat position of grid step `step`. Swing delays every odd step by
 * `swing × stepBeats` (0 = straight, 0.33 ≈ triplet feel, 0.5 = hard swing).
 * Triplet grids ignore swing.
 */
export function stepToBeat(step: number, stepBeats: number, swing: number): number {
  const s = isTripletStep(stepBeats) ? 0 : clampSwing(swing);
  const odd = Math.abs(step % 2) === 1;
  return clean(step * stepBeats + (odd ? s * stepBeats : 0));
}

/** Grid step nearest to `beat` (inverse of stepToBeat, swing-aware). */
export function stepAtBeat(beat: number, stepBeats: number, swing: number): number {
  const s = isTripletStep(stepBeats) ? 0 : clampSwing(swing);
  if (s === 0) return Math.round(clean(beat / stepBeats));
  const pair = 2 * stepBeats;
  const k = Math.floor(beat / pair);
  const r = beat - k * pair;
  const candidates = [0, (1 + s) * stepBeats, pair];
  let best = 0;
  for (let i = 1; i < candidates.length; i++) {
    if (Math.abs(r - candidates[i]) < Math.abs(r - candidates[best]) - 1e-9) best = i;
  }
  return 2 * k + best;
}

/**
 * Snap a beat to the quantize grid, honouring swing (odd steps are delayed by
 * `swing × step`; triplet grids are never swung). 'off' returns the beat as is.
 */
export function quantizeBeat(beat: number, grid: QuantizeGrid, swing: number): number {
  const step = gridStepBeats(grid);
  if (step === null) return beat;
  return stepToBeat(stepAtBeat(beat, step, swing), step, swing);
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
  return {
    bars,
    quantize,
    swing: typeof r.swing === 'number' ? clampSwing(r.swing) : base.swing,
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
 * MIDI export note for a drum-track pad note: the GM note of the factory voice
 * on that pad when there is one, else the pad note itself (36..51).
 */
export function drumNoteToGm(note: number, kit: KitState | undefined): number {
  const pad = noteToPad(note);
  if (pad === null) return note;
  const source = kit?.pads[pad]?.sample?.source;
  return source?.kind === 'factory' ? FACTORY_VOICE_GM_NOTE[source.voice] : note;
}
