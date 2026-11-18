//! Realtime partitioned convolution ≙ Chromium `ConvolverNode` (stereo IR: `L ⊛ IR_L`,
//! `R ⊛ IR_R`; mono IR: both channels through it), zero added latency, allocation-free `process`.
//!
//! # Scheme: non-uniform partitions, each stage a uniformly partitioned overlap-save FDL
//!
//! The IR is cut into stages. Stage `s` has block size `B` (FFT size `N = 2B`) and covers the IR
//! range `[O, O + P·B)` in `P` partitions; its frequency-domain delay line (FDL) holds the spectra
//! of the last `P` input windows `[(m−1)B, (m+1)B)` and output block `m` of the stage is
//! `IFFT(Σ_p X_{m−p}·H_p)` (second half, overlap-save), heard at frames `mB + O …`.
//! - Head: `B = 128` = the render quantum, `O = 0`: computed in the quantum that completes its input
//!   block, so its output is the current quantum (zero latency, no direct FIR needed).
//! - Later stages: `B ∈ {256 … 32768}`, `O = 2B − 256`. Block `m` completes at frame `(m+1)B`;
//!   its job (forward FFT, FDL insert, MAC, inverse FFT) is a list of fixed-size ops spread evenly
//!   over the next `Q = B/128` quanta (op `i` runs once the cumulative weight before it is below
//!   `⌈total·q/Q⌉` in the job's `q`-th quantum), finishing in the quantum that starts at frame
//!   `(m+2)B − 256 = mB + O`, the first one that needs it. So every quantum does ≈ the same work.
//! - Stage sizes are chosen at construction by a cost model (FFT butterflies vs MAC bins per
//!   quantum, over every increasing subset of the candidate sizes) from the actual IR: partitions
//!   whose IR samples are all zero are skipped (a pre-delay costs nothing; a stage with no non-zero
//!   partition is dropped, the head included). For the RoomReverb IR at 48 kHz (87 456 frames, 1056
//!   leading zeros) this gives head + 1024 + 8192.
//!
//! Stereo: the two real input channels are packed into one complex window (`re = L`, `im = R`),
//! one complex FFT per window; the FDL stores the separated half spectra `X_L`, `X_R` (bins
//! `0..=B`, from `X[k]` and `conj X[N−k]`), the MAC runs over half spectra only, and the two
//! accumulated half spectra are re-packed (`Y[k] = A_L + i·A_R`, `Y[N−k] = conj A_L + i·conj A_R`)
//! into one complex inverse FFT whose real / imaginary parts are the L / R outputs. The IR scale,
//! the 1/N of the inverse and the ½ of each unpacking are folded into the stored IR spectra.
//!
//! Precision: f32 data and transforms (like Chromium), twiddles and IR spectra computed in f64 and
//! rounded once. Measured vs an f64 direct convolution: ≈ −135 dB error energy (convolver_tests).
//!
//! Silence: after the input has been exactly zero for longer than every stage's reach
//! (`O + (P+2)·B` frames), the whole state is exactly zero (FFTs of zero windows are zero; every
//! pending output has been read and cleared), so `process` outputs zeros and does no work, and the
//! clock freezes (the state is time-shift invariant). A new convolver starts in that state.

use mdaw_dsp::buffer::AudioBuffer;

#[path = "fft.rs"]
mod fft;

use fft::Plan;

/// Quantum size the convolver processes.
pub const BLOCK: usize = 128;

/// Block sizes a stage after the head may use.
const CANDIDATE_SIZES: [usize; 8] = [256, 512, 1024, 2048, 4096, 8192, 16384, 32768];
/// Elements per scheduled op of a spread stage (the scheduling granularity).
const CHUNK: usize = 256;

// Op weights per element (≈ flops / 1.6): radix-2 butterfly, radix-4 group, bin of a stereo
// MAC (two complex multiply-adds), bin of unpack / pack, output sample.
const W_BFLY: u32 = 10;
const W_R4: u32 = 16;
const W_MAC: u32 = 16;
const W_BIN: u32 = 8;
const W_OUT: u32 = 8;

#[derive(Debug, Clone, Copy)]
enum Op {
    /// First forward pass (half `B`) reading the window from the input ring, butterflies.
    FwdFirst(u32, u32),
    /// Forward pass with half size `.0`, butterflies `[.1, .2)`.
    Fwd(u32, u32, u32),
    /// Fused last two forward passes, radix-4 groups.
    FwdLast(u32, u32),
    /// Separate the packed spectrum into the FDL slot (and clear the accumulators), bins.
    Unpack(u32, u32),
    /// Multiply-accumulate non-zero partition `.0` (index into `parts`), bins `[.1, .2)`.
    Mac(u32, u32, u32),
    /// Re-pack the accumulators into the inverse FFT input, bins.
    Pack(u32, u32),
    /// Fused first two inverse passes, radix-4 groups.
    InvFirst(u32, u32),
    /// Inverse pass with half size `.0`, butterflies.
    Inv(u32, u32, u32),
    /// Upper half of the last inverse pass, added into the output ring, samples.
    InvLast(u32, u32),
}

/// A stereo ring of frames indexed by absolute frame (power-of-two size).
struct Ring {
    l: Vec<f32>,
    r: Vec<f32>,
    mask: usize,
}

impl Ring {
    fn new(size: usize) -> Self {
        Self {
            l: vec![0.0; size],
            r: vec![0.0; size],
            mask: size - 1,
        }
    }
}

struct Stage {
    /// Block size B (N = 2B), quanta per block, IR offset O (output delay of block m: mB + O).
    b: usize,
    quanta: u32,
    offset: usize,
    plan: Plan<f32>,
    work_re: Vec<f32>,
    work_im: Vec<f32>,
    /// Accumulators `[A_L re | A_L im | A_R re | A_R im]`, `B + 1` bins each.
    acc: Vec<f32>,
    /// FDL: `slots` × `[X_L re | X_L im | X_R re | X_R im]`; block m lives in slot m % slots.
    fdl: Vec<f32>,
    slots: usize,
    /// Non-zero partitions and their spectra (same layout as an FDL slot, scaled).
    parts: Vec<u32>,
    h: Vec<f32>,
    ops: Vec<Op>,
    /// Weight of the ops before op i; `total` = weight of all.
    before: Vec<u32>,
    total: u32,
    // Job state: block being computed, next op, quanta into the job.
    block: u64,
    next: usize,
    job_q: u32,
    /// Op weight executed so far (scheduler test instrumentation).
    #[cfg(test)]
    done: u64,
}

/// Total op weight of a stage with block `b` and `nnz` non-zero partitions.
fn stage_weight(b: usize, nnz: usize) -> u64 {
    let n = 2 * b;
    let passes = n.trailing_zeros() as u64 - 2; // radix-2 passes (the last two are one radix-4)
    let m = (b + 1) as u64;
    2 * (passes * (n as u64 / 2) * W_BFLY as u64 + (n as u64 / 4) * W_R4 as u64)
        + (b as u64) * W_OUT as u64
        + 2 * m * W_BIN as u64
        + nnz as u64 * m * W_MAC as u64
}

impl Stage {
    /// `h[c]` = IR channel c (already the stage's own range is `[offset, end)`).
    fn new(b: usize, offset: usize, end: usize, ir: [&[f32]; 2], scale: f64, nz: &[usize]) -> Self {
        let n = 2 * b;
        let m = b + 1;
        let parts: Vec<u32> = nz.iter().map(|&p| p as u32).collect();
        let slots = nz.last().map_or(1, |&p| p + 1);
        // IR spectra in f64: pack (hL + i·hR)·scale/(4N), forward, unpack (no ½) → 2·H·scale/(4N);
        // with the FDL's unpacked 2·X and the unnormalized inverse this yields X·H·scale.
        let plan64 = Plan::<f64>::new(n);
        let c = scale / (4.0 * n as f64);
        let mut h = vec![0f32; parts.len() * 4 * m];
        let (mut re, mut im) = (vec![0f64; n], vec![0f64; n]);
        for (i, &p) in nz.iter().enumerate() {
            re.fill(0.0);
            im.fill(0.0);
            let s = offset + p * b;
            for t in 0..b.min(end - s) {
                re[t] = ir[0][s + t] as f64 * c;
                im[t] = ir[1][s + t] as f64 * c;
            }
            plan64.forward(&mut re, &mut im);
            let dst = &mut h[i * 4 * m..(i + 1) * 4 * m];
            for k in 0..m {
                let (i1, i2) = (
                    plan64.rev[k] as usize,
                    plan64.rev[(n - k) & (n - 1)] as usize,
                );
                let (ar, ai, cr, ci) = (re[i1], im[i1], re[i2], im[i2]);
                dst[k] = (ar + cr) as f32;
                dst[m + k] = (ai - ci) as f32;
                dst[2 * m + k] = (ai + ci) as f32;
                dst[3 * m + k] = (cr - ar) as f32;
            }
        }
        let quanta = (b / BLOCK) as u32;
        let chunk = if quanta == 1 { usize::MAX } else { CHUNK };
        let mut ops = Vec::new();
        let mut push = |len: usize, f: &dyn Fn(u32, u32) -> Op| {
            let mut s = 0;
            while s < len {
                let e = len.min(s.saturating_add(chunk));
                ops.push(f(s as u32, e as u32));
                s = e;
            }
        };
        push(b, &|s, e| Op::FwdFirst(s, e));
        let mut half = b / 2;
        while half >= 4 {
            push(b, &|s, e| Op::Fwd(half as u32, s, e));
            half /= 2;
        }
        push(n / 4, &|s, e| Op::FwdLast(s, e));
        push(m, &|s, e| Op::Unpack(s, e));
        for i in 0..parts.len() {
            push(m, &|s, e| Op::Mac(i as u32, s, e));
        }
        push(m, &|s, e| Op::Pack(s, e));
        push(n / 4, &|s, e| Op::InvFirst(s, e));
        let mut half = 4;
        while half < b {
            push(b, &|s, e| Op::Inv(half as u32, s, e));
            half *= 2;
        }
        push(b, &|s, e| Op::InvLast(s, e));
        let weight = |op: &Op| -> u32 {
            match *op {
                Op::FwdFirst(s, e) | Op::Fwd(_, s, e) | Op::Inv(_, s, e) => (e - s) * W_BFLY,
                Op::FwdLast(s, e) | Op::InvFirst(s, e) => (e - s) * W_R4,
                Op::Unpack(s, e) | Op::Pack(s, e) => (e - s) * W_BIN,
                Op::Mac(_, s, e) => (e - s) * W_MAC,
                Op::InvLast(s, e) => (e - s) * W_OUT + (e - s) * W_BFLY,
            }
        };
        let mut before = Vec::with_capacity(ops.len());
        let mut total = 0u32;
        for op in &ops {
            before.push(total);
            total += weight(op);
        }
        let len = ops.len();
        Self {
            b,
            quanta,
            offset,
            plan: Plan::new(n),
            work_re: vec![0.0; n],
            work_im: vec![0.0; n],
            acc: vec![0.0; 4 * m],
            fdl: vec![0.0; slots * 4 * m],
            slots,
            parts,
            h,
            ops,
            before,
            total,
            block: 0,
            next: len,
            job_q: 0,
            #[cfg(test)]
            done: 0,
        }
    }

    fn reset(&mut self) {
        self.work_re.fill(0.0);
        self.work_im.fill(0.0);
        self.acc.fill(0.0);
        self.fdl.fill(0.0);
        self.block = 0;
        self.next = self.ops.len();
        self.job_q = 0;
    }

    /// Frames after the last non-zero input until this stage's state is all zero again.
    fn reach(&self) -> usize {
        self.offset + (self.slots + 2) * self.b
    }

    /// Advance by one quantum; `t` = frames written to the input ring so far (this quantum
    /// included).
    fn tick(&mut self, t: u64, input: &Ring, out: &mut Ring) {
        if t.is_multiple_of(self.b as u64) {
            debug_assert_eq!(self.next, self.ops.len(), "job overran its quanta");
            self.block = t / self.b as u64 - 1;
            self.next = 0;
            self.job_q = 0;
        }
        if self.next == self.ops.len() {
            return;
        }
        self.job_q += 1;
        let q = self.quanta as u64;
        let target = (self.total as u64 * self.job_q as u64).div_ceil(q);
        while self.next < self.ops.len() && (self.before[self.next] as u64) < target {
            let op = self.ops[self.next];
            self.exec(op, input, out);
            self.next += 1;
            #[cfg(test)]
            {
                let end = self.before.get(self.next).copied().unwrap_or(self.total);
                self.done += (end - self.before[self.next - 1]) as u64;
            }
        }
    }

    fn exec(&mut self, op: Op, input: &Ring, out: &mut Ring) {
        let (b, n, m) = (self.b, 2 * self.b, self.b + 1);
        let plan = &self.plan;
        let (re, im) = (&mut self.work_re[..], &mut self.work_im[..]);
        match op {
            Op::FwdFirst(s, e) => {
                let (s, e) = (s as usize, e as usize);
                let start = self.block * b as u64;
                let pb = (start.wrapping_sub(b as u64) as usize & input.mask) + s;
                let cb = (start as usize & input.mask) + s;
                fft::dif_first_from(
                    (&input.l[pb..], &input.r[pb..]),
                    (&input.l[cb..], &input.r[cb..]),
                    &plan.tw_re,
                    &plan.tw_im,
                    b,
                    s,
                    e,
                    re,
                    im,
                );
            }
            Op::Fwd(h, s, e) => fft::dif_pass(
                re,
                im,
                &plan.tw_re,
                &plan.tw_im,
                h as usize,
                s as usize,
                e as usize,
            ),
            Op::FwdLast(s, e) => fft::dif_radix4_last(re, im, s as usize, e as usize),
            Op::Unpack(s, e) => {
                let slot = (self.block % self.slots as u64) as usize;
                let x = &mut self.fdl[slot * 4 * m..(slot + 1) * 4 * m];
                let (xlr, rest) = x.split_at_mut(m);
                let (xli, rest) = rest.split_at_mut(m);
                let (xrr, xri) = rest.split_at_mut(m);
                for k in s as usize..e as usize {
                    let i1 = plan.rev[k] as usize;
                    let i2 = plan.rev[(n - k) & (n - 1)] as usize;
                    let (ar, ai, cr, ci) = (re[i1], im[i1], re[i2], im[i2]);
                    xlr[k] = ar + cr;
                    xli[k] = ai - ci;
                    xrr[k] = ai + ci;
                    xri[k] = cr - ar;
                }
                for a in self.acc.chunks_exact_mut(m) {
                    a[s as usize..e as usize].fill(0.0);
                }
            }
            Op::Mac(i, s, e) => {
                let (i, s, e) = (i as usize, s as usize, e as usize);
                let p = self.parts[i] as usize;
                let slot =
                    ((self.block % self.slots as u64) as usize + self.slots - p) % self.slots;
                let x = &self.fdl[slot * 4 * m..(slot + 1) * 4 * m];
                let hh = &self.h[i * 4 * m..(i + 1) * 4 * m];
                let (alr, rest) = self.acc.split_at_mut(m);
                let (ali, rest) = rest.split_at_mut(m);
                let (arr, ari) = rest.split_at_mut(m);
                mac(
                    (&mut alr[s..e], &mut ali[s..e]),
                    (&x[s..e], &x[m + s..m + e]),
                    (&hh[s..e], &hh[m + s..m + e]),
                );
                mac(
                    (&mut arr[s..e], &mut ari[s..e]),
                    (&x[2 * m + s..2 * m + e], &x[3 * m + s..3 * m + e]),
                    (&hh[2 * m + s..2 * m + e], &hh[3 * m + s..3 * m + e]),
                );
            }
            Op::Pack(s, e) => {
                let a = &self.acc;
                for k in s as usize..e as usize {
                    let (lr, li, rr, ri) = (a[k], a[m + k], a[2 * m + k], a[3 * m + k]);
                    let i1 = plan.rev[k] as usize;
                    re[i1] = lr - ri;
                    im[i1] = li + rr;
                    if k != 0 && k != b {
                        let i2 = plan.rev[n - k] as usize;
                        re[i2] = lr + ri;
                        im[i2] = rr - li;
                    }
                }
            }
            Op::InvFirst(s, e) => fft::dit_radix4_first(re, im, s as usize, e as usize),
            Op::Inv(h, s, e) => fft::dit_pass(
                re,
                im,
                &plan.tw_re,
                &plan.tw_im,
                h as usize,
                s as usize,
                e as usize,
            ),
            Op::InvLast(s, e) => {
                let (mut s, e) = (s as usize, e as usize);
                while s < e {
                    let pos = (self.block as usize * b + self.offset + s) & out.mask;
                    let len = (e - s).min(out.mask + 1 - pos);
                    fft::dit_last_upper_add(
                        re,
                        im,
                        &plan.tw_re,
                        &plan.tw_im,
                        b,
                        s,
                        s + len,
                        &mut out.l[pos..],
                        &mut out.r[pos..],
                    );
                    s += len;
                }
            }
        }
    }
}

/// `acc += x·h` over complex split slices of equal length.
#[inline(always)]
fn mac(acc: (&mut [f32], &mut [f32]), x: (&[f32], &[f32]), h: (&[f32], &[f32])) {
    let len = acc.0.len();
    let (ar, ai) = (acc.0, &mut acc.1[..len]);
    let (xr, xi, hr, hi) = (&x.0[..len], &x.1[..len], &h.0[..len], &h.1[..len]);
    for k in 0..len {
        ar[k] += xr[k] * hr[k] - xi[k] * hi[k];
        ai[k] += xr[k] * hi[k] + xi[k] * hr[k];
    }
}

/// Stereo convolver: output channel `c` = input channel `c` ⊛ (`scale` · IR channel `c`)
/// (a 1-channel IR is used for both channels; channels past the second are ignored).
pub struct Convolver {
    stages: Vec<Stage>,
    input: Ring,
    out: Ring,
    /// Frames processed (frozen while idle).
    t: u64,
    /// Consecutive all-zero input frames, and how many make the state exactly zero.
    silent: usize,
    idle_after: usize,
}

impl Convolver {
    /// Built on the control side (allocates everything).
    pub fn new(ir: &AudioBuffer, scale: f32) -> Self {
        Self::build(ir, scale, None)
    }

    /// With the given stage block sizes after the head (testing all partition boundaries).
    #[cfg(test)]
    pub(crate) fn with_stage_sizes(ir: &AudioBuffer, scale: f32, sizes: &[usize]) -> Self {
        Self::build(ir, scale, Some(sizes))
    }

    fn build(ir: &AudioBuffer, scale: f32, sizes: Option<&[usize]>) -> Self {
        let empty: &[f32] = &[];
        let l = ir.channels.first().map_or(empty, |c| &c[..]);
        let r = ir.channels.get(1).map_or(l, |c| &c[..l.len().min(c.len())]);
        let l = &l[..r.len()];
        let len = l.len();
        // nzc[i] = non-zero frames (either channel) among the first i.
        let mut nzc = Vec::with_capacity(len + 1);
        nzc.push(0u32);
        for i in 0..len {
            nzc.push(nzc[i] + u32::from(l[i] != 0.0 || r[i] != 0.0));
        }
        let layout = match sizes {
            Some(s) => s.to_vec(),
            None => choose_layout(&nzc),
        };
        let mut stages = Vec::new();
        for (bounds, b) in stage_bounds(&layout, len) {
            let nz = nonzero_parts(&nzc, b, bounds.0, bounds.1);
            if !nz.is_empty() {
                stages.push(Stage::new(b, bounds.0, bounds.1, [l, r], scale as f64, &nz));
            }
        }
        let bmax = stages.iter().map(|s| s.b).max().unwrap_or(BLOCK);
        let ring = (4 * bmax).max(4 * BLOCK);
        let idle_after = stages.iter().map(|s| s.reach() + BLOCK).max().unwrap_or(0);
        Self {
            stages,
            input: Ring::new(ring),
            out: Ring::new(ring),
            t: 0,
            silent: idle_after,
            idle_after,
        }
    }

    /// One render quantum.
    pub fn process(
        &mut self,
        in_l: &[f32; BLOCK],
        in_r: &[f32; BLOCK],
        out_l: &mut [f32; BLOCK],
        out_r: &mut [f32; BLOCK],
    ) {
        if in_l.iter().chain(in_r).all(|&x| x == 0.0) {
            if self.silent >= self.idle_after {
                out_l.fill(0.0);
                out_r.fill(0.0);
                return;
            }
            self.silent += BLOCK;
        } else {
            self.silent = 0;
        }
        let p = self.t as usize & self.input.mask;
        self.input.l[p..p + BLOCK].copy_from_slice(in_l);
        self.input.r[p..p + BLOCK].copy_from_slice(in_r);
        self.t += BLOCK as u64;
        for st in &mut self.stages {
            st.tick(self.t, &self.input, &mut self.out);
        }
        let p = (self.t as usize - BLOCK) & self.out.mask;
        let (ol, or) = (&mut self.out.l[p..p + BLOCK], &mut self.out.r[p..p + BLOCK]);
        out_l.copy_from_slice(ol);
        out_r.copy_from_slice(or);
        ol.fill(0.0);
        or.fill(0.0);
    }

    /// Clear all state (silence in, silence out).
    pub fn reset(&mut self) {
        for st in &mut self.stages {
            st.reset();
        }
        for ring in [&mut self.input, &mut self.out] {
            ring.l.fill(0.0);
            ring.r.fill(0.0);
        }
        self.t = 0;
        self.silent = self.idle_after;
    }

    /// True while the silence fast path is active (no work per quantum).
    pub fn is_idle(&self) -> bool {
        self.silent >= self.idle_after
    }

    /// Stage block sizes (head = 128 first, if it has a non-zero partition).
    pub fn stage_sizes(&self) -> Vec<usize> {
        self.stages.iter().map(|s| s.b).collect()
    }

    /// Op weight executed so far over all stages (scheduler instrumentation).
    #[cfg(test)]
    pub(crate) fn work_done(&self) -> u64 {
        self.stages.iter().map(|s| s.done).sum()
    }

    /// Op weight per quantum (the scheduler's cost model: Σ total / Q over stages).
    pub fn weight_per_quantum(&self) -> f64 {
        self.stages
            .iter()
            .map(|s| s.total as f64 / s.quanta as f64)
            .sum()
    }
}

/// `((offset, end), b)` of the head and each stage of `sizes` for an IR of `len` frames; stages
/// starting at or past `len` are left out.
fn stage_bounds(sizes: &[usize], len: usize) -> Vec<((usize, usize), usize)> {
    let mut starts = vec![(0usize, BLOCK)];
    for &b in sizes {
        debug_assert!(b.is_power_of_two() && b > BLOCK);
        starts.push((2 * b - 2 * BLOCK, b));
    }
    let mut out = Vec::new();
    for (i, &(o, b)) in starts.iter().enumerate() {
        if o >= len {
            break;
        }
        let end = starts.get(i + 1).map_or(len, |&(next, _)| next.min(len));
        out.push(((o, end), b));
    }
    out
}

/// Partitions of `b` frames in `[offset, end)` holding a non-zero IR sample.
fn nonzero_parts(nzc: &[u32], b: usize, offset: usize, end: usize) -> Vec<usize> {
    (0..(end - offset).div_ceil(b))
        .filter(|p| {
            let s = offset + p * b;
            nzc[end.min(s + b)] > nzc[s]
        })
        .collect()
}

/// The increasing subset of [`CANDIDATE_SIZES`] with the least op weight per quantum.
fn choose_layout(nzc: &[u32]) -> Vec<usize> {
    let len = nzc.len() - 1;
    let mut best = (f64::INFINITY, Vec::new());
    for mask in 0u32..(1 << CANDIDATE_SIZES.len()) {
        let sizes: Vec<usize> = (0..CANDIDATE_SIZES.len())
            .filter(|i| mask >> i & 1 == 1)
            .map(|i| CANDIDATE_SIZES[i])
            .collect();
        if sizes.last().is_some_and(|&b| 2 * b - 2 * BLOCK >= len) {
            continue; // a stage past the end: same as a smaller subset
        }
        let cost: f64 = stage_bounds(&sizes, len)
            .into_iter()
            .map(|((o, e), b)| {
                let nnz = nonzero_parts(nzc, b, o, e).len();
                if nnz == 0 {
                    0.0
                } else {
                    stage_weight(b, nnz) as f64 / (b / BLOCK) as f64
                }
            })
            .sum();
        if cost < best.0 {
            best = (cost, sizes);
        }
    }
    best.1
}

#[cfg(test)]
#[path = "convolver_tests.rs"]
mod tests;
