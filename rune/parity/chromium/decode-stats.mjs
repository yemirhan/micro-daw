/**
 * Per-channel decode statistics (Node side of the decode capture). Mirrored exactly by
 * rune/crates/mdaw-dsp/src/audio_io/parity.rs — keep both in sync.
 *
 * - digest:    FNV-1a 64 over the little-endian f32 bytes of the whole channel (hex, 16 digits)
 * - peak, rms: over the finite samples (non-finite samples count as 0), rms over the full length
 * - nonFinite: number of NaN / ±Inf samples
 * - blockRms:  RMS per BLOCK-frame block (last block partial)
 * - blockProj: per block, PROJECTIONS sums Σ x[i]·s_k[i] (f64, in index order) with s_k[i] = ±1
 *              from xorshift32 streams (seed SEEDS[k], one step per sample, continuous over the
 *              channel; sign = top bit set → −1). For two signals the difference of a projection
 *              is Σ e·s, whose square is an unbiased estimate of the block's error energy Σ e².
 */
export const BLOCK = 1024;
export const PROJECTIONS = 4;
export const SEEDS = [0x9e3779b9, 0x7f4a7c15, 0x85ebca6b, 0xc2b2ae35];

const FNV_OFFSET = 0xcbf29ce484222325n;
const FNV_PRIME = 0x100000001b3n;
const MASK = 0xffffffffffffffffn;

export function fnv1a64(f32) {
  const u8 = new Uint8Array(f32.buffer, f32.byteOffset, f32.byteLength);
  // 32-bit halves: h·prime = h·0x1b3 + (h << 40) (mod 2^64), done on two 32-bit words for speed.
  let lo = Number(FNV_OFFSET & 0xffffffffn);
  let hi = Number(FNV_OFFSET >> 32n);
  for (let i = 0; i < u8.length; i++) {
    lo = (lo ^ u8[i]) >>> 0;
    const loMul = lo * 0x1b3; // < 2^41, exact in a double
    const carry = Math.floor(loMul / 0x100000000);
    const newLo = loMul >>> 0;
    // hi·0x1b3 + carry + (lo << 8) (the << 40 term lands in the high word shifted by 8)
    const newHi = (Math.imul(hi, 0x1b3) + carry + ((lo << 8) >>> 0)) >>> 0;
    lo = newLo;
    hi = newHi;
  }
  return ((BigInt(hi) << 32n) | BigInt(lo)).toString(16).padStart(16, '0');
}

/** Reference BigInt implementation (used once to self-check the fast path). */
function fnv1a64Slow(f32) {
  const u8 = new Uint8Array(f32.buffer, f32.byteOffset, f32.byteLength);
  let h = FNV_OFFSET;
  for (let i = 0; i < u8.length; i++) h = ((h ^ BigInt(u8[i])) * FNV_PRIME) & MASK;
  return h.toString(16).padStart(16, '0');
}
{
  const probe = Float32Array.from([0, 1, -1, 0.123, NaN, -3.5e-9, 7]);
  if (fnv1a64(probe) !== fnv1a64Slow(probe)) throw new Error('fnv1a64 fast path is wrong');
}

export function onset(chans, threshold) {
  const n = chans.length ? chans[0].length : 0;
  for (let i = 0; i < n; i++) for (const d of chans) if (Math.abs(d[i]) > threshold) return i;
  return -1;
}

export function channelStats(d) {
  let peak = 0;
  let sq = 0;
  let nonFinite = 0;
  const blockRms = [];
  const blockProj = [];
  const state = SEEDS.slice();
  for (let s = 0; s < d.length; s += BLOCK) {
    const e = Math.min(d.length, s + BLOCK);
    let bsq = 0;
    const proj = new Array(PROJECTIONS).fill(0);
    for (let i = s; i < e; i++) {
      let x = d[i];
      if (!Number.isFinite(x)) {
        nonFinite++;
        x = 0;
      }
      const a = Math.abs(x);
      if (a > peak) peak = a;
      bsq += x * x;
      for (let k = 0; k < PROJECTIONS; k++) {
        let v = state[k];
        v ^= v << 13;
        v ^= v >>> 17;
        v ^= v << 5;
        v >>>= 0;
        state[k] = v;
        proj[k] += v & 0x80000000 ? -x : x;
      }
    }
    sq += bsq;
    blockRms.push(Math.sqrt(bsq / (e - s)));
    blockProj.push(proj);
  }
  return {
    digest: fnv1a64(d),
    peak,
    rms: Math.sqrt(sq / Math.max(1, d.length)),
    nonFinite,
    blockRms,
    blockProj,
  };
}
