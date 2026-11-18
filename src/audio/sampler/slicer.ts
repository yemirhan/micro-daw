/**
 * Sample chopping helpers (pure, no audio context needed).
 *
 * Transient detection: short-frame energy envelope in dB; an onset is a frame
 * whose level rises sharply above the recent floor (local maximum of the
 * rise), above a gate relative to the loudest frame. The onset is then
 * refined to the first sample that reaches a fraction of the hit's peak.
 */

export interface TransientOptions {
  /** 0..1 — higher finds quieter / softer hits. Default 0.5. */
  sensitivity?: number;
  /** Upper bound on returned slices (including the one at 0). Default 16. */
  maxSlices?: number;
  /** Minimum distance between slices in seconds. Default 0.05. */
  minGapSec?: number;
}

/** Minimal AudioBuffer surface we read from. */
interface BufferLike {
  numberOfChannels: number;
  length: number;
  getChannelData(channel: number): Float32Array;
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

function toMono(buffer: BufferLike | Float32Array): Float32Array {
  if (buffer instanceof Float32Array) return buffer;
  const n = buffer.numberOfChannels;
  if (n <= 1) return buffer.getChannelData(0);
  const out = new Float32Array(buffer.length);
  for (let c = 0; c < n; c++) {
    const data = buffer.getChannelData(c);
    for (let i = 0; i < out.length; i++) out[i] += data[i] / n;
  }
  return out;
}

/** Slice start times in seconds, ascending; the first is always 0. */
export function detectTransients(
  buffer: AudioBuffer | Float32Array,
  sampleRate: number,
  opts: TransientOptions = {},
): number[] {
  const data = toMono(buffer);
  const sensitivity = clamp(opts.sensitivity ?? 0.5, 0, 1);
  const maxSlices = Math.max(1, Math.floor(opts.maxSlices ?? 16));
  const minGap = Math.max(0, opts.minGapSec ?? 0.05);
  if (data.length === 0 || !(sampleRate > 0) || maxSlices === 1) return [0];

  const hop = Math.max(32, Math.round(sampleRate * 0.005));
  const win = hop * 2;
  const frames = Math.floor((data.length - 1) / hop) + 1;
  const db = new Float32Array(frames);
  let maxDb = -Infinity;
  for (let f = 0; f < frames; f++) {
    const start = f * hop;
    const end = Math.min(data.length, start + win);
    let sum = 0;
    for (let i = start; i < end; i++) sum += data[i] * data[i];
    const v = 10 * Math.log10(sum / Math.max(1, end - start) + 1e-12);
    db[f] = v;
    if (v > maxDb) maxDb = v;
  }
  if (maxDb < -100) return [0];

  const riseThreshold = 14 - 11 * sensitivity; // dB above the recent floor
  const gate = maxDb - (24 + 36 * sensitivity); // ignore frames this far below the loudest
  const lookback = 3;
  const rise = new Float32Array(frames);
  for (let f = 0; f < frames; f++) {
    let floor = Infinity;
    for (let k = 1; k <= lookback; k++) floor = Math.min(floor, f - k >= 0 ? db[f - k] : -120);
    rise[f] = db[f] - floor;
  }

  const candidates: { time: number; strength: number }[] = [];
  for (let f = 0; f < frames; f++) {
    if (db[f] < gate || rise[f] < riseThreshold) continue;
    // Local maximum of the rise (first one wins on plateaus).
    let isPeak = true;
    for (let k = -2; k <= 2 && isPeak; k++) {
      const g = f + k;
      if (k === 0 || g < 0 || g >= frames) continue;
      if (k < 0 ? rise[g] >= rise[f] : rise[g] > rise[f]) isPeak = false;
    }
    if (!isPeak) continue;
    candidates.push({ time: refineOnset(data, f, hop, win) / sampleRate, strength: rise[f] });
  }

  // Strongest first, honouring the minimum gap (a hit right at the start is slice 0).
  candidates.sort((a, b) => b.strength - a.strength);
  const accepted: number[] = [0];
  for (const c of candidates) {
    if (accepted.length >= maxSlices) break;
    if (accepted.every((t) => Math.abs(t - c.time) >= Math.max(minGap, 1e-6))) accepted.push(c.time);
  }
  return accepted.sort((a, b) => a - b);
}

/** First sample around frame `f` whose level reaches 10% of the hit's peak. */
function refineOnset(data: Float32Array, f: number, hop: number, win: number): number {
  const from = Math.max(0, (f - 1) * hop);
  const to = Math.min(data.length, f * hop + win);
  let peak = 0;
  for (let i = from; i < to; i++) peak = Math.max(peak, Math.abs(data[i]));
  const threshold = peak * 0.1;
  for (let i = from; i < to; i++) {
    if (Math.abs(data[i]) >= threshold) return i;
  }
  return f * hop;
}

/** `count` equal slices over `durationSec`: [0, d/n, 2d/n, ...]. */
export function equalSlices(durationSec: number, count: number): number[] {
  const n = Math.max(1, Math.floor(count));
  const d = Math.max(0, durationSec);
  return Array.from({ length: n }, (_, i) => (d * i) / n);
}

/**
 * Turn slice start times into [start, end) pairs (end = next start, the last
 * one ends at `durationSec`) — what "chop to pads" assigns as trim points.
 */
export function slicesToRanges(starts: number[], durationSec: number): { startSec: number; endSec: number }[] {
  const sorted = [...starts].sort((a, b) => a - b);
  return sorted.map((s, i) => ({ startSec: s, endSec: i + 1 < sorted.length ? sorted[i + 1] : durationSec }));
}
