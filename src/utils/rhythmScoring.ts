/**
 * Pure timing judgement for the rhythm trainer and drum pattern practice.
 *
 * The beat grid is analytic (start + k * interval, in seconds on the audio
 * clock), so scoring never depends on when a React effect or a scheduler
 * callback happened to run.
 */
import type { RhythmFeedback } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';

export const PERFECT_WINDOW_MS = 35;
export const GOOD_WINDOW_MS = 80;
export const ACCEPTABLE_WINDOW_MS = 150;

type Timing = RhythmFeedback['timing'];

const TIMING_SCORE: Record<Timing, number> = { perfect: 1, good: 1, early: 0.5, late: 0.5, miss: 0 };

/** Judge an offset (ms, negative = early). Outside the acceptable window = miss. */
export function judgeOffset(offsetMs: number): Timing {
  const abs = Math.abs(offsetMs);
  if (abs <= PERFECT_WINDOW_MS) return 'perfect';
  if (abs <= GOOD_WINDOW_MS) return 'good';
  if (abs <= ACCEPTABLE_WINDOW_MS) return offsetMs < 0 ? 'early' : 'late';
  return 'miss';
}

export interface RhythmScorerOptions {
  /** Audio-clock time (s) of beat 0. */
  startTime: number;
  /** Seconds between beats (ticks). */
  interval: number;
  /** Number of beats in the exercise; undefined = endless free practice. */
  totalBeats?: number;
  /**
   * Drum pattern: pads expected on each beat (cycled). Without a pattern any
   * single hit per beat is expected (piano rhythm training).
   */
  pattern?: DrumPadId[][];
}

/** "any" = any key/pad counts (piano rhythm training). */
type Slot = DrumPadId | 'any';

export class RhythmScorer {
  private readonly opts: RhythmScorerOptions;
  /** beat index -> slot -> best score achieved */
  private readonly matched = new Map<number, Map<Slot, number>>();
  private strayHits = 0;
  readonly feedback: RhythmFeedback[] = [];

  constructor(opts: RhythmScorerOptions) {
    this.opts = opts;
  }

  beatTime(beat: number): number {
    return this.opts.startTime + beat * this.opts.interval;
  }

  expectedSlots(beat: number): Slot[] {
    const { pattern } = this.opts;
    if (!pattern || pattern.length === 0) return ['any'];
    return pattern[((beat % pattern.length) + pattern.length) % pattern.length];
  }

  /** Index of the beat nearest to `time` (clamped to the exercise). */
  nearestBeat(time: number): number {
    const raw = Math.round((time - this.opts.startTime) / this.opts.interval);
    const max = this.opts.totalBeats !== undefined ? this.opts.totalBeats - 1 : Infinity;
    return Math.max(0, Math.min(max, raw));
  }

  /**
   * Record a hit at audio time `time` (s). `padId` is the drum pad for
   * pattern practice; omit it for "any key" rhythm training.
   */
  recordHit(time: number, padId?: DrumPadId): RhythmFeedback {
    const beat = this.nearestBeat(time);
    const offsetMs = (time - this.beatTime(beat)) * 1000;
    let timing = judgeOffset(offsetMs);

    const slots = this.expectedSlots(beat);
    const slot: Slot | undefined = slots.includes('any')
      ? 'any'
      : padId !== undefined && slots.includes(padId)
        ? padId
        : undefined;

    const beatMatches = this.matched.get(beat) ?? new Map<Slot, number>();
    if (timing === 'miss' || slot === undefined || beatMatches.has(slot)) {
      // Off the grid, wrong pad, or a second hit for the same slot.
      timing = 'miss';
      this.strayHits += 1;
    } else {
      beatMatches.set(slot, TIMING_SCORE[timing]);
      this.matched.set(beat, beatMatches);
    }

    const fb: RhythmFeedback = { beat, timing, offsetMs };
    this.feedback.push(fb);
    return fb;
  }

  /**
   * Accuracy (0..1) over every expected slot of the beats whose window has
   * passed by `now` (all beats when `now` is omitted and totalBeats is set).
   * Missed slots count 0; stray / wrong hits add to the denominator.
   */
  accuracy(now?: number): number {
    const beats = this.beatsElapsed(now);
    // Endless free practice: start counting from the player's first hit.
    const first = this.opts.totalBeats === undefined ? this.firstMatchedBeat(beats) : 0;
    let expected = 0;
    let score = 0;
    for (let b = first; b < beats; b++) {
      const slots = this.expectedSlots(b);
      expected += slots.length;
      const m = this.matched.get(b);
      if (m) for (const s of slots) score += m.get(s) ?? 0;
    }
    // Hits on beats that are still open count too (live display).
    for (const [b, m] of this.matched) if (b >= beats) for (const v of m.values()) { score += v; expected += 1; }
    const denom = expected + this.strayHits;
    return denom === 0 ? 0 : Math.max(0, Math.min(1, score / denom));
  }

  /** Beats whose acceptable window is over at `now` (or all of them). */
  beatsElapsed(now?: number): number {
    const total = this.opts.totalBeats;
    if (now === undefined) return total ?? this.lastMatchedBeat() + 1;
    const passed = Math.floor((now - this.opts.startTime - ACCEPTABLE_WINDOW_MS / 1000) / this.opts.interval) + 1;
    const clamped = Math.max(0, passed);
    return total !== undefined ? Math.min(total, clamped) : clamped;
  }

  private firstMatchedBeat(fallback: number): number {
    let min = Infinity;
    for (const b of this.matched.keys()) min = Math.min(min, b);
    return min === Infinity ? fallback : min;
  }

  private lastMatchedBeat(): number {
    let max = -1;
    for (const b of this.matched.keys()) max = Math.max(max, b);
    return max;
  }
}
