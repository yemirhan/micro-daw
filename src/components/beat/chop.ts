import { detectTransients, equalSlices, slicesToRanges } from '@/audio/sampler/slicer';
import { PAD_COUNT, type PadIndex } from '@/types/sampler';

export type ChopMode = { kind: 'transients'; sensitivity: number } | { kind: 'equal'; count: 4 | 8 | 16 };

interface BufferLike {
  numberOfChannels: number;
  length: number;
  sampleRate: number;
  getChannelData(channel: number): Float32Array;
}

/** Mono mix of [from, to) samples. */
function monoRange(buffer: BufferLike, from: number, to: number): Float32Array {
  const n = Math.max(0, to - from);
  const ch = Math.max(1, buffer.numberOfChannels);
  if (ch === 1) return buffer.getChannelData(0).subarray(from, to);
  const out = new Float32Array(n);
  for (let c = 0; c < ch; c++) {
    const data = buffer.getChannelData(c);
    for (let i = 0; i < n; i++) out[i] += data[from + i] / ch;
  }
  return out;
}

/**
 * Slice ranges (absolute buffer seconds) of the trimmed part of a sample.
 * At most as many slices as there are pads from `fromPad` to the last pad.
 */
export function chopRanges(
  buffer: BufferLike,
  trim: { startSec: number; endSec: number | null },
  mode: ChopMode,
  fromPad: PadIndex = 0,
): { startSec: number; endSec: number }[] {
  const sr = buffer.sampleRate;
  const total = buffer.length / sr;
  const start = Math.min(total, Math.max(0, trim.startSec || 0));
  const end = Math.min(total, Math.max(start, trim.endSec ?? total));
  const dur = end - start;
  if (!(dur > 0)) return [];
  const maxSlices = Math.max(1, PAD_COUNT - fromPad);
  const starts =
    mode.kind === 'equal'
      ? equalSlices(dur, Math.min(mode.count, maxSlices))
      : detectTransients(monoRange(buffer, Math.floor(start * sr), Math.floor(end * sr)), sr, {
          sensitivity: mode.sensitivity,
          maxSlices,
        });
  return slicesToRanges(starts, dur).map((r) => ({ startSec: start + r.startSec, endSec: start + r.endSec }));
}
