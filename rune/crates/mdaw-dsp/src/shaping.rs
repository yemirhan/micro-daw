//! Waveshaping: the master-bus soft clipper (`src/audio/masterBus.ts`), the factory kick/snare
//! saturation curves (`src/audio/drumKit.ts`) and a Web Audio `WaveShaperNode` evaluator.
//!
//! Curves are built exactly like `Tone.WaveShaper(mapping, length)` (`Tone.WaveShaper.setMap`):
//! `curve[i] = f32(mapping((i / (len - 1)) * 2 - 1))`, and evaluated like Chromium's
//! `WaveShaperDSPKernel` with `oversample = 'none'` (the app never oversamples these shapers):
//! `v = 0.5·(x + 1)·(len − 1)`; `v < 0` → `curve[0]`, `v ≥ len − 1` → `curve[len − 1]`, else linear
//! interpolation between `curve[floor v]` and `curve[floor v + 1]` (in f64, stored as f32).
//!
//! Master bus (`installMasterBus`): `Volume(master dB)` → `Gain(10^(2/20) / 2)` →
//! `WaveShaper(u ↦ softClip(2u), 4096)`, no oversampling. The gain stage folds the +2 dB "glue"
//! drive and the shaper's input scale (`u = x / CLIP_RANGE`) into one multiply.

use std::sync::LazyLock;

/// Soft-clip knee: below this the curve is the identity.
pub const CLIP_KNEE: f64 = 0.85;
/// Hard ceiling the clipper converges to (≈ −0.18 dBFS).
pub const CLIP_CEILING: f64 = 0.98;
/// Fixed bus gain (dB) before the clipper (`MASTER_DRIVE_DB`).
pub const MASTER_DRIVE_DB: f64 = 2.0;
/// Signal range covered by the master curve: x ∈ [−2, 2] ↔ shaper input u ∈ [−1, 1].
pub const CLIP_RANGE: f64 = 2.0;
/// Length of the master clip curve.
pub const MASTER_CURVE_LEN: usize = 4096;
/// Length of the kick / snare shaper curves (Tone default).
pub const DRUM_CURVE_LEN: usize = 1024;

/// `softClip(x)` of masterBus.ts: identity up to |x| = 0.85, then
/// `sign(x)·(0.85 + 0.13·tanh((|x| − 0.85)/0.13))`.
pub fn soft_clip(x: f64) -> f64 {
    let a = x.abs();
    if a <= CLIP_KNEE {
        return x;
    }
    let room = CLIP_CEILING - CLIP_KNEE;
    js_sign(x) * (CLIP_KNEE + room * ((a - CLIP_KNEE) / room).tanh())
}

fn js_sign(x: f64) -> f64 {
    if x > 0.0 {
        1.0
    } else if x < 0.0 {
        -1.0
    } else {
        x
    }
}

/// Gain of the master `drive` node: `10^(MASTER_DRIVE_DB/20) / CLIP_RANGE`.
pub fn master_drive_gain() -> f64 {
    10f64.powf(MASTER_DRIVE_DB / 20.0) / CLIP_RANGE
}

/// [`master_drive_gain`] as the f32 the Web Audio `GainNode` applies, evaluated once (the same
/// f64 expression, rounded to f32 once, so per-sample results are unchanged).
pub fn master_drive_gain_f32() -> f32 {
    static GAIN: LazyLock<f32> = LazyLock::new(|| master_drive_gain() as f32);
    *GAIN
}

/// `new Tone.WaveShaper(mapping, len).curve`.
pub fn tone_curve(len: usize, mapping: impl Fn(f64) -> f64) -> Vec<f32> {
    let last = (len.max(2) - 1) as f64;
    (0..len)
        .map(|i| {
            let normalized = (i as f64 / last) * 2.0 - 1.0;
            mapping(normalized) as f32
        })
        .collect()
}

/// The master clipper curve: `u ↦ softClip(u · CLIP_RANGE)`, 4096 points.
pub fn master_clip_curve() -> Vec<f32> {
    tone_curve(MASTER_CURVE_LEN, |u| soft_clip(u * CLIP_RANGE))
}

/// Kick bus saturation: `tanh(1.6x) / tanh(1.6)`, 1024 points. Freshly built; renders use the
/// process-wide copy [`kick_curve_shared`].
pub fn kick_curve() -> Vec<f32> {
    tone_curve(DRUM_CURVE_LEN, |x| (1.6 * x).tanh() / 1.6f64.tanh())
}

/// Snare bus saturation: `tanh(1.4x) / tanh(1.4)`, 1024 points. Freshly built; renders use the
/// process-wide copy [`snare_curve_shared`].
pub fn snare_curve() -> Vec<f32> {
    tone_curve(DRUM_CURVE_LEN, |x| (1.4 * x).tanh() / 1.4f64.tanh())
}

static KICK_CURVE: LazyLock<Vec<f32>> = LazyLock::new(kick_curve);
static SNARE_CURVE: LazyLock<Vec<f32>> = LazyLock::new(snare_curve);
static MASTER_CLIP_CURVE: LazyLock<Vec<f32>> = LazyLock::new(master_clip_curve);

/// [`kick_curve`], built once per process (no allocation per render).
pub fn kick_curve_shared() -> &'static [f32] {
    &KICK_CURVE
}

/// [`snare_curve`], built once per process (no allocation per render).
pub fn snare_curve_shared() -> &'static [f32] {
    &SNARE_CURVE
}

/// [`master_clip_curve`], built once per process.
pub fn master_clip_curve_shared() -> &'static [f32] {
    &MASTER_CLIP_CURVE
}

/// One sample through a Web Audio `WaveShaperNode` (oversample `none`).
#[inline]
pub fn waveshape(curve: &[f32], input: f32) -> f32 {
    let n = curve.len();
    match n {
        0 => return input,
        1 => return curve[0],
        _ => {}
    }
    let v = 0.5 * (input as f64 + 1.0) * (n - 1) as f64;
    if v < 0.0 {
        curve[0]
    } else if v >= (n - 1) as f64 {
        curve[n - 1]
    } else {
        let k = v as usize;
        let f = v - k as f64;
        ((1.0 - f) * curve[k] as f64 + f * curve[k + 1] as f64) as f32
    }
}

/// Shape a buffer in place.
pub fn waveshape_in_place(curve: &[f32], data: &mut [f32]) {
    for x in data {
        *x = waveshape(curve, *x);
    }
}

/// Master bus at unity master volume (offline renders: `masterDb = 0`): drive gain, then the
/// clip curve. `x` is the bus input sample.
pub fn master_bus_sample(curve: &[f32], x: f32) -> f32 {
    waveshape(curve, x * master_drive_gain_f32())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> Value {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../parity/fixtures/");
        let s = std::fs::read_to_string(format!("{p}{name}")).expect("fixture");
        serde_json::from_str(&s).unwrap()
    }

    fn floats(v: &Value) -> Vec<f32> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap() as f32)
            .collect()
    }

    /// Compare f32 curves; returns (exact matches, max ulp distance).
    fn ulp_compare(a: &[f32], b: &[f32]) -> (usize, u32) {
        assert_eq!(a.len(), b.len());
        let mut exact = 0;
        let mut max_ulp = 0;
        for (x, y) in a.iter().zip(b) {
            if x.to_bits() == y.to_bits() {
                exact += 1;
            }
            let d = (x.to_bits() as i64 - y.to_bits() as i64).unsigned_abs() as u32;
            max_ulp = max_ulp.max(d);
        }
        (exact, max_ulp)
    }

    #[test]
    fn soft_clip_shape() {
        assert_eq!(soft_clip(0.5), 0.5);
        assert_eq!(soft_clip(-0.85), -0.85);
        assert!(soft_clip(10.0) <= CLIP_CEILING && soft_clip(10.0) > 0.979);
        assert_eq!(soft_clip(-3.0), -soft_clip(3.0));
        assert!((master_drive_gain() - 0.629_462_705_897_084).abs() < 1e-12);
    }

    #[test]
    fn shared_curves_equal_fresh_ones() {
        assert_eq!(kick_curve_shared(), kick_curve().as_slice());
        assert_eq!(snare_curve_shared(), snare_curve().as_slice());
        assert_eq!(master_clip_curve_shared(), master_clip_curve().as_slice());
        // Same allocation every call.
        assert!(std::ptr::eq(kick_curve_shared(), kick_curve_shared()));
    }

    #[test]
    fn master_bus_sample_uses_the_same_gain_bits() {
        let curve = master_clip_curve_shared();
        let reference = |x: f32| waveshape(curve, x * (master_drive_gain() as f32));
        assert_eq!(
            master_drive_gain_f32().to_bits(),
            (master_drive_gain() as f32).to_bits()
        );
        for i in -400..=400 {
            let x = i as f32 / 100.0;
            assert_eq!(
                master_bus_sample(curve, x).to_bits(),
                reference(x).to_bits()
            );
        }
    }

    #[test]
    fn waveshaper_interpolates_and_clamps() {
        let c = [-1.0f32, 0.0, 1.0];
        assert_eq!(waveshape(&c, -2.0), -1.0);
        assert_eq!(waveshape(&c, 1.0), 1.0);
        assert_eq!(waveshape(&c, 5.0), 1.0);
        assert_eq!(waveshape(&c, 0.5), 0.5);
        assert_eq!(waveshape(&c, -0.25), -0.25);
    }

    #[test]
    fn master_curve_matches_ts_golden() {
        let g = fixture("shaping.json");
        let curve = master_clip_curve();
        let golden = floats(&g["masterCurve"]);
        let (exact, ulp) = ulp_compare(&curve, &golden);
        println!(
            "master curve: {exact}/{} bit-exact, max {ulp} ulp",
            curve.len()
        );
        assert!(ulp <= 1, "master curve differs by {ulp} ulp");
        // softClip samples (f64) from the TS function.
        for pair in g["softClip"].as_array().unwrap() {
            let x = pair[0].as_f64().unwrap();
            let y = pair[1].as_f64().unwrap();
            assert!(
                (soft_clip(x) - y).abs() <= 1e-15 * y.abs().max(1.0),
                "softClip({x})"
            );
        }
        assert_eq!(master_drive_gain(), g["driveGain"].as_f64().unwrap());
    }

    #[test]
    fn drum_curves_match_chromium() {
        let g = fixture("shaping_chromium.json");
        for (name, curve) in [("kick", kick_curve()), ("snare", snare_curve())] {
            let golden = floats(&g[name]);
            let (exact, ulp) = ulp_compare(&curve, &golden);
            println!(
                "{name} curve: {exact}/{} bit-exact, max {ulp} ulp",
                curve.len()
            );
            assert!(ulp <= 1, "{name} curve differs by {ulp} ulp");
        }
    }
}
