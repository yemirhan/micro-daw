/**
 * Signal statistics shared by the Chromium capture (this file, Node) and the Rust parity test
 * (rune/crates/mdaw-dsp/src/factory/parity_tests.rs mirrors every definition here — keep them in sync).
 *
 * - peak:     max |x|
 * - rms:      sqrt(mean x²)
 * - env:      RMS per 5 ms frame (frame = round(0.005·sr) samples, last frame partial), in dBFS (floor −200)
 * - centroid: spectral centroid (Hz) of 4 windows: N = 2048, starts floor(w·len/4) (w = 0..3),
 *             zero-padded past the end, periodic Hann, POWER-weighted (|X|²) over bins 0..N/2
 *             (magnitude weighting made near-pure low tones, e.g. the kick tail at −57 dBFS, measure
 *             the f32 rounding floor: a −110 dB broadband floor moved a 46 Hz centroid by 6 %)
 * - onset:    first index with |x| ≥ 0.1·peak
 * - zcr:      sign changes ((x<0) differs from previous) per second
 * - blocks:   full-length waveform evidence (see `blockEvidence`): per BLOCK-sample block
 *             (last block partial) the energy Σx² and PROJ random ±1 projections Σ r·x.
 */
export const FRAME_SEC = 0.005;
export const CENTROID_N = 2048;
export const HEAD = 2048;
export const BLOCK = 1024;
export const PROJ = 16;
export const PROJ_SEED = 0x9a0f0001;

export const toDb = (x) => (x > 1e-10 ? 20 * Math.log10(x) : -200);

export function envelope(x, sr) {
  const frame = Math.round(FRAME_SEC * sr);
  const out = [];
  for (let s = 0; s < x.length; s += frame) {
    const e = Math.min(x.length, s + frame);
    let acc = 0;
    for (let i = s; i < e; i++) acc += x[i] * x[i];
    out.push(Math.sqrt(acc / (e - s)));
  }
  return out;
}

function fftMag(re) {
  const n = re.length;
  const im = new Float64Array(n);
  const r = Float64Array.from(re);
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      [r[i], r[j]] = [r[j], r[i]];
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    for (let i = 0; i < n; i += len) {
      for (let k = 0; k < len / 2; k++) {
        const wr = Math.cos(ang * k);
        const wi = Math.sin(ang * k);
        const ar = r[i + k + len / 2];
        const ai = im[i + k + len / 2];
        const tr = ar * wr - ai * wi;
        const ti = ar * wi + ai * wr;
        r[i + k + len / 2] = r[i + k] - tr;
        im[i + k + len / 2] = im[i + k] - ti;
        r[i + k] += tr;
        im[i + k] += ti;
      }
    }
  }
  const mag = new Float64Array(n / 2 + 1);
  for (let k = 0; k <= n / 2; k++) mag[k] = Math.hypot(r[k], im[k]);
  return mag;
}

export function centroids(x, sr) {
  const n = CENTROID_N;
  const out = [];
  for (let w = 0; w < 4; w++) {
    const start = Math.floor((w * x.length) / 4);
    const buf = new Float64Array(n);
    for (let i = 0; i < n; i++) {
      const v = start + i < x.length ? x[start + i] : 0;
      buf[i] = v * (0.5 - 0.5 * Math.cos((2 * Math.PI * i) / n));
    }
    const mag = fftMag(buf);
    let num = 0;
    let den = 0;
    for (let k = 0; k <= n / 2; k++) {
      num += ((k * sr) / n) * mag[k] * mag[k];
      den += mag[k] * mag[k];
    }
    out.push(den > 0 ? num / den : 0);
  }
  return out;
}

export function stats(x, sr) {
  let peak = 0;
  let acc = 0;
  for (let i = 0; i < x.length; i++) {
    const a = Math.abs(x[i]);
    if (a > peak) peak = a;
    acc += x[i] * x[i];
  }
  let onset = -1;
  for (let i = 0; i < x.length; i++) {
    if (Math.abs(x[i]) >= 0.1 * peak) {
      onset = i;
      break;
    }
  }
  let zc = 0;
  for (let i = 1; i < x.length; i++) if (x[i] < 0 !== x[i - 1] < 0) zc++;
  return {
    length: x.length,
    peak,
    rms: Math.sqrt(acc / Math.max(1, x.length)),
    env: envelope(x, sr),
    centroid: centroids(x, sr),
    onset,
    zcr: zc / (x.length / sr),
  };
}

/**
 * Full-length waveform evidence of `x`. For each block b of BLOCK samples (the last one partial)
 * this returns [Σx², p_0 … p_{PROJ−1}] with p_k = Σ_i r_k[i]·x[i] (f64 sums of the f32 samples),
 * flattened block after block. r_k[i] = ±1 comes from ONE mulberry32(PROJ_SEED) u32 stream per
 * signal: for each block, for each k, ceil(blockLen/32) words; bit t of word w (LSB first) gives
 * sample w·32+t of the block: 1 → −1, 0 → +1.
 *
 * Why: for another signal y of the same length, d_k = p_k(y) − p_k(x) = Σ r_k·(y−x), and for
 * Rademacher r, E[d_k²] = Σ(y−x)² exactly, so mean_k d_k² estimates the block's error energy
 * without storing the waveform (16 projections: within a factor ~2 with overwhelming
 * probability; exact for an error confined to one sample). Mirrored by `block_evidence` in
 * rune/crates/mdaw-dsp/src/factory/parity_tests.rs.
 */
export function blockEvidence(x) {
  let a = PROJ_SEED | 0;
  const next = () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return (t ^ (t >>> 14)) >>> 0;
  };
  const out = [];
  for (let s = 0; s < x.length; s += BLOCK) {
    const e = Math.min(x.length, s + BLOCK);
    let energy = 0;
    for (let i = s; i < e; i++) energy += x[i] * x[i];
    out.push(energy);
    for (let k = 0; k < PROJ; k++) {
      let p = 0;
      for (let w = s; w < e; w += 32) {
        const word = next();
        for (let t = 0; t < 32 && w + t < e; t++)
          p += (word >>> t) & 1 ? -x[w + t] : x[w + t];
      }
      out.push(p);
    }
  }
  return out;
}
