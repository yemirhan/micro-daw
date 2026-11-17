import * as Tone from 'tone';
import type { Arrangement, Region, RegionNote, Track } from '@/types/arrangement';
import { noteToPad, padToNote, type LoopRecordSettings, type PadIndex } from '@/types/sampler';
import {
  STEP_EPSILON,
  gridStepBeats,
  loopGateBeats,
  loopSwingGrid,
  mergeDrumHit,
  quantizeBeat,
  swingOffsetBeats,
  wrapBeat,
} from '@/utils/arrangementHelpers';
import { undoManager } from '../UndoManager';

/** Hits this close before the end of the count-in are recorded on the first beat. */
const COUNT_IN_PRE_ROLL_BEATS = 0.5;
/** Extra time (s) a suppressed pattern hit stays suppressed after its expected play time. */
const SUPPRESS_MARGIN_SEC = 0.15;
/** Loop wraps older than this (s) are dropped when no gate pad is held (see pruneWraps). */
const WRAP_KEEP_SEC = 1;

const hitKey = (beat: number, note: number) => `${note}@${Math.round(beat * 1000)}`;
const clamp01 = (v: number) => Math.min(1, Math.max(0, Number.isFinite(v) ? v : 0));

/** One loop-recording take (startLoopRecording → stopLoopRecording). */
export interface LoopRecordingSession {
  trackId: string;
  /** Absolute beat where the pattern (and the loop) starts. */
  patternStart: number;
  /** Loop length in beats (bars × 4). */
  loopBeats: number;
  /** Context time the count-in ends (transport start), null when not counting in. */
  countInEnd: number | null;
  countInBeats: number;
  /** overdub=false: the pattern was already cleared by the first hit of this take. */
  replaced: boolean;
  /** Arrangement before the take; pushed as ONE undo step on the first change. */
  undoSnapshot: Arrangement;
  undoPushed: boolean;
  /** The take created/grew the pattern region (needs an undo step even without hits). */
  structureChanged: boolean;
}

/** What the loop recorder needs from the arrangement engine. */
export interface LoopRecorderHost {
  getBpm(): number;
  getLoopSettings(): LoopRecordSettings;
  findTrack(trackId: string): Track | undefined;
  /** The pad plays in gate mode (held length matters). */
  isGatePad(trackId: string, pad: PadIndex): boolean;
  /** Pattern region of the track, created/grown to cover the pattern. */
  ensurePatternRegion(track: Track): { region: Region; changed: boolean };
  /** Predicate: a pad note inside the pattern window of `region`. */
  patternNoteFilter(region: Region): (n: RegionNote) => boolean;
  /** Musical transport position (beats, from ticks) at an audio time (default now). */
  transportBeatAt(time?: number): number;
  /** Add one new hit to the running playback; false = the track needs a full reschedule. */
  addToPlayback(track: Track, region: Region, hit: RegionNote): boolean;
  /** Rebuild the track's playback schedule. */
  refreshPlayback(trackId: string): void;
  emitStateChange(): void;
}

/** A gate-mode pad held during the take. */
interface HeldGate {
  regionId: string;
  /** Region-relative start of the stored note. */
  startBeat: number;
  /** Audio time of the hit. */
  onTime: number;
  /** Transport beat of the hit (virtual, before the pattern start, for a count-in pre-roll hit). */
  onBeat: number;
}

/**
 * The beat view's loop recorder: one take at a time. Hits are quantized (to
 * the straight grid against the sounding swing), wrapped into the loop and
 * merged into the active drum track's pattern live; gate pads get their held
 * (musical) length on release; the count-in is tracked for the UI.
 *
 * The engine owns the transport: it starts/stops it around `begin`/`end`.
 */
export class LoopRecorder {
  private take: LoopRecordingSession | null = null;
  /** Pattern hits (hitKey) not to play once, until the given context time: the player just played them live. */
  private suppressedHits = new Map<string, number>();
  private heldGates = new Map<number, HeldGate>();
  /** Transport loop wraps during the take: audio time + beats jumped back (loop length). */
  private wraps: { time: number; beats: number }[] = [];
  private countInTimer: ReturnType<typeof setTimeout> | null = null;
  private detachLoopListener: (() => void) | null = null;

  constructor(private readonly host: LoopRecorderHost) {}

  isActive(): boolean {
    return this.take !== null;
  }

  /** The running take (null when not recording). */
  get session(): Readonly<LoopRecordingSession> | null {
    return this.take;
  }

  /** Start a take (the engine has prepared the pattern region and starts the transport). */
  begin(take: Omit<LoopRecordingSession, 'countInEnd' | 'replaced' | 'undoPushed'>): void {
    this.reset();
    this.take = { ...take, countInEnd: null, replaced: false, undoPushed: false };
    const transport = Tone.getTransport();
    const onLoop = (time: number) => {
      if (!this.take || !Number.isFinite(time)) return;
      this.wraps.push({ time, beats: this.take.loopBeats });
      this.pruneWraps();
    };
    try {
      transport.on('loop', onLoop);
      this.detachLoopListener = () => {
        try {
          transport.off('loop', onLoop);
        } catch {
          // transport gone
        }
      };
    } catch {
      this.detachLoopListener = null;
    }
  }

  /** The count-in ends (transport starts) at this audio time; listeners are notified every count-in beat. */
  setCountInEnd(time: number | null): void {
    if (!this.take) return;
    this.take.countInEnd = time;
    this.scheduleCountInTick();
  }

  /** The loop length changed during the take (bars setting). */
  setLoopBeats(beats: number): void {
    if (this.take) this.take.loopBeats = beats;
  }

  markStructureChanged(): void {
    if (this.take) this.take.structureChanged = true;
  }

  /**
   * End the take: gate pads still held get their length, then everything is
   * cleared. Returns the finished take (null when none was running).
   */
  end(): LoopRecordingSession | null {
    const take = this.take;
    if (!take) return null;
    this.clearCountInTimer();
    for (const note of [...this.heldGates.keys()]) this.captureRelease(note);
    this.reset();
    return take;
  }

  /** Drop the take without closing held notes (failed start / dispose). */
  abort(): void {
    this.reset();
  }

  private reset(): void {
    this.clearCountInTimer();
    this.detachLoopListener?.();
    this.detachLoopListener = null;
    this.take = null;
    this.suppressedHits.clear();
    this.heldGates.clear();
    this.wraps = [];
  }

  /**
   * Keep only the loop wraps a gate length can still need (review L-e: the list
   * grew by one entry per pass for the whole take): those after the oldest held
   * gate's note-on, and with no gate held, the recent ones (a hit stamped a
   * little in the past, or a wrap reported ahead of time by the look-ahead).
   */
  private pruneWraps(): void {
    let cutoff = Tone.immediate() - WRAP_KEEP_SEC;
    for (const held of this.heldGates.values()) cutoff = Math.min(cutoff, held.onTime);
    if (this.wraps.length > 0 && this.wraps[0].time <= cutoff) this.wraps = this.wraps.filter((w) => w.time > cutoff);
  }

  /** Number of loop wraps remembered for gate lengths (diagnostics / tests). */
  get trackedWraps(): number {
    return this.wraps.length;
  }

  /** Beats of count-in left (0 when not counting in). */
  getCountInRemaining(): number {
    const take = this.take;
    if (!take || take.countInEnd === null) return 0;
    const left = take.countInEnd - Tone.immediate();
    if (left <= 0) return 0;
    const secPerBeat = 60 / this.host.getBpm();
    return Math.min(take.countInBeats, Math.ceil(left / secPerBeat - 1e-6));
  }

  /** Notify listeners every count-in beat (getCountInRemaining changes) and once when it ends. */
  private scheduleCountInTick(): void {
    this.clearCountInTimer();
    const take = this.take;
    if (!take || take.countInEnd === null) return;
    const left = take.countInEnd - Tone.immediate();
    if (left <= 0) {
      take.countInEnd = null;
      this.host.emitStateChange();
      return;
    }
    const secPerBeat = 60 / this.host.getBpm();
    const untilNextBeat = left % secPerBeat || secPerBeat;
    this.countInTimer = setTimeout(() => {
      this.countInTimer = null;
      this.host.emitStateChange();
      this.scheduleCountInTick();
    }, untilNextBeat * 1000 + 5);
  }

  private clearCountInTimer(): void {
    if (this.countInTimer !== null) clearTimeout(this.countInTimer);
    this.countInTimer = null;
  }

  /**
   * The pattern player is about to play `note` at absolute `beat` (audio
   * `time`): false = play it; true = skip it once (the player just hit it live
   * and it was snapped forward into this pass).
   */
  consumeSuppressedHit(beat: number, note: number, time: number): boolean {
    if (this.suppressedHits.size === 0) return false;
    const key = hitKey(beat, note);
    const until = this.suppressedHits.get(key);
    if (until === undefined) return false;
    this.suppressedHits.delete(key);
    return time <= until;
  }

  /**
   * A pad hit while loop recording: quantize (nearest swung step, stored on the
   * straight grid), wrap into the loop and merge into the pattern. `time` is
   * the audio time the hit sounds (default now). `hintBeats`: the grid a
   * generated hit (note repeat) is on; kept when finer than the quantize.
   */
  captureHit(note: number, velocity: number, isDrum: boolean, time?: number, hintBeats?: number): void {
    const take = this.take;
    // Synth notes are not recorded by the loop recorder (yet).
    if (!take || !isDrum) return;
    const pad = noteToPad(note);
    const track = this.host.findTrack(take.trackId);
    if (pad === null || !track) return;

    const at = time ?? Tone.immediate();
    const secPerBeat = 60 / this.host.getBpm();
    const loop = this.host.getLoopSettings();
    const length = take.loopBeats;
    const quantStep = gridStepBeats(loop.quantize);
    /** The hint grid, when it is finer than the record quantize (else null). */
    const fineGrid = hintBeats !== undefined && hintBeats > 0 && (quantStep === null || hintBeats < quantStep - STEP_EPSILON)
      ? hintBeats
      : null;
    const swingGrid = loopSwingGrid(loop);
    let quantized: number;
    /** Beats until the stored hit plays in this pass (> 0: still ahead). */
    let ahead: number;
    /** Transport beat of the hit (virtual, before the pattern start, during the count-in). */
    let onBeat: number;
    const countInLeft = take.countInEnd === null ? 0 : take.countInEnd - at;
    if (countInLeft > 0) {
      // Early hits right before the downbeat belong to beat 1; earlier ones are ignored.
      if (countInLeft > COUNT_IN_PRE_ROLL_BEATS * secPerBeat) return;
      quantized = 0;
      ahead = countInLeft / secPerBeat;
      onBeat = take.patternStart - ahead;
    } else {
      onBeat = this.host.transportBeatAt(at);
      const played = wrapBeat(onBeat - take.patternStart, length);
      // Quantize on the absolute (bar-aligned) grid, the one playback swings.
      // A generated hit (note repeat) on a finer grid than the record quantize
      // keeps its own (straight) grid so fast rolls aren't merged together.
      const abs = take.patternStart + played;
      const snapped = fineGrid !== null
        ? Math.round(abs / fineGrid) * fineGrid
        : quantizeBeat(abs, loop.quantize, loop.swing, swingGrid);
      quantized = wrapBeat(snapped - take.patternStart, length);
      const playsAt = quantized + swingOffsetBeats(take.patternStart + quantized, swingGrid, loop.swing);
      ahead = playsAt - played;
      if (ahead < -length / 2) ahead += length;
      else if (ahead > length / 2) ahead -= length;
    }

    if (!take.undoPushed) {
      undoManager.pushUndo('Loop Record', take.undoSnapshot);
      take.undoPushed = true;
    }
    const { region, changed: grown } = this.host.ensurePatternRegion(track);
    const offset = take.patternStart - region.startBeat;
    let cleared = false;
    if (!loop.overdub && !take.replaced) {
      const inPattern = this.host.patternNoteFilter(region);
      const kept = region.notes.filter((n) => !inPattern(n));
      cleared = kept.length !== region.notes.length;
      region.notes = kept;
    }
    take.replaced = true;

    const gate = this.host.isGatePad(track.id, pad);
    const hit: RegionNote = {
      note: padToNote(pad),
      velocity: clamp01(velocity > 1 ? velocity / 127 : velocity),
      startBeat: offset + quantized,
      // One-shots: a fixed short length (the sample plays out anyway). Gate pads
      // get their held length on note-off (provisionally one grid step).
      durationBeats: fineGrid ?? quantStep ?? 0.25,
      isDrum: true,
    };
    const countBefore = region.notes.length;
    region.notes = mergeDrumHit(region.notes, hit);
    const added = region.notes.length === countBefore + 1;
    if (gate) this.heldGates.set(hit.note, { regionId: region.id, startBeat: hit.startBeat, onTime: at, onBeat });

    // Snapped forward (still ahead in this pass): the pattern would replay the hit just heard.
    if (ahead > 0) {
      this.suppressedHits.set(
        hitKey(region.startBeat + hit.startBeat, hit.note),
        at + ahead * secPerBeat + SUPPRESS_MARGIN_SEC,
      );
    }

    this.host.emitStateChange();
    // A plain new hit is added to the running Part (no rebuild of the whole
    // pattern per hit); anything else (region grown, pattern replaced, a
    // merged velocity) reschedules the track.
    if (added && !grown && !cleared && this.host.addToPlayback(track, region, hit)) return;
    this.host.refreshPlayback(track.id);
  }

  /**
   * Note-off while loop recording: a held gate-mode pad gets its real length,
   * capped at the loop length. One-shot pads keep their fixed length.
   *
   * The length is MUSICAL time: transport beats (ticks, right across tempo
   * changes) at note-on/off plus the loop length of every transport loop wrap
   * between them (the transport's own loop events), so no elapsed seconds ×
   * BPM estimate can invent or miss a loop pass (review E1).
   */
  captureRelease(note: number, time?: number): void {
    const take = this.take;
    const held = this.heldGates.get(note);
    if (!take || !held) return;
    this.heldGates.delete(note);
    const track = this.host.findTrack(take.trackId);
    const region = track?.regions.find((r) => r.id === held.regionId);
    if (!track || !region) return;
    const off = time ?? Tone.immediate();
    const countInLeft = take.countInEnd === null ? 0 : take.countInEnd - off;
    const offBeat = countInLeft > 0 ? take.patternStart - countInLeft / (60 / this.host.getBpm()) : this.host.transportBeatAt(off);
    let wrapped = 0;
    for (const w of this.wraps) if (w.time > held.onTime && w.time <= off) wrapped += w.beats;
    this.pruneWraps();
    const durationBeats = loopGateBeats(held.onBeat, offBeat, wrapped, take.loopBeats);
    let changed = false;
    region.notes = region.notes.map((n) => {
      if (n.note !== note || Math.abs(n.startBeat - held.startBeat) >= STEP_EPSILON) return n;
      changed = true;
      return { ...n, durationBeats };
    });
    if (!changed) return;
    this.host.emitStateChange();
    this.host.refreshPlayback(track.id);
  }
}
