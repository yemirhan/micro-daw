/**
 * Offline sanity metrics for rendered drum voices (tests + the headless
 * verification script): peak, length, spectral centroid and how the energy
 * splits between lows (< 150 Hz) and highs (> 5 kHz).
 */

export interface VoiceAnalysis {
  peakDb: number;
  lengthSec: number;
  /** Energy-weighted mean frequency, Hz. */
  centroidHz: number;
  /** Fraction of spectral energy below 150 Hz. */
  lowFrac: number;
  /** Fraction of spectral energy above 5 kHz. */
  highFrac: number;
  /** Frequency of the strongest FFT bin, Hz. */
  peakHz: number;
}

/** In-place iterative radix-2 FFT (re/im, length = power of two). */
function fft(re: Float64Array, im: Float64Array): void {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [re[i], re[j]] = [re[j], re[i]];
      [im[i], im[j]] = [im[j], im[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wr = Math.cos(ang);
    const wi = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let cr = 1;
      let ci = 0;
      for (let k = 0; k < len / 2; k++) {
        const a = i + k;
        const b = a + len / 2;
        const xr = re[b] * cr - im[b] * ci;
        const xi = re[b] * ci + im[b] * cr;
        re[b] = re[a] - xr;
        im[b] = im[a] - xi;
        re[a] += xr;
        im[a] += xi;
        const t = cr * wr - ci * wi;
        ci = cr * wi + ci * wr;
        cr = t;
      }
    }
  }
}

export function analyzeVoice(data: Float32Array, sampleRate: number): VoiceAnalysis {
  let peak = 0;
  for (let i = 0; i < data.length; i++) peak = Math.max(peak, Math.abs(data[i]));
  // Whole sound (zero-padded), capped at 2^18 samples (~6 s at 44.1 kHz).
  let size = 1;
  while (size < data.length && size < 1 << 18) size <<= 1;
  const re = new Float64Array(size);
  const im = new Float64Array(size);
  re.set(data.subarray(0, size));
  fft(re, im);
  let total = 0, low = 0, high = 0, weighted = 0, best = 0, bestBin = 0;
  for (let k = 1; k < size / 2; k++) {
    const f = (k * sampleRate) / size;
    const e = re[k] * re[k] + im[k] * im[k];
    total += e;
    weighted += e * f;
    if (f < 150) low += e;
    if (f > 5000) high += e;
    if (e > best) {
      best = e;
      bestBin = k;
    }
  }
  const t = total || 1;
  return {
    peakDb: peak > 0 ? 20 * Math.log10(peak) : -Infinity,
    lengthSec: data.length / sampleRate,
    centroidHz: weighted / t,
    lowFrac: low / t,
    highFrac: high / t,
    peakHz: (bestBin * sampleRate) / size,
  };
}
