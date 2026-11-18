//! `Tone.Meter` (defaults: one channel, smoothing 0.8, dB): what `getTrackLevel` /
//! `getMasterLevel` read.
//!
//! The meter is an `Analyser({ size: 256, type: 'waveform', channels: 1 })`: a one-output
//! `ChannelSplitter` (the stereo input down-mixed to mono, `(L + R)/2`) into an `AnalyserNode` with
//! `fftSize = 512`, read with `getFloatTimeDomainData` into a 256-sample array. Chromium copies the
//! first 256 of the last 512 frames the analyser saw, so the window is the **older half** of the
//! last four render quanta. `getValue()` then takes the RMS of the window and smooths it per call:
//! `rms = max(rms_now, rms_prev · 0.8)` (a rise shows at once, a fall decays per read, so the decay
//! rate depends on how often the UI reads), and returns `gainToDb(rms)`.
//!
//! [`MeterWindow`] runs on the audio thread (per quantum); [`Meter`] is the per-read smoothing on
//! the control side.

use super::Q;

/// The audio side: the mono sum of squares of the last four quanta.
#[derive(Debug, Clone, Copy, Default)]
pub struct MeterWindow {
    ss: [f64; 4],
    n: usize,
}

impl MeterWindow {
    /// Feed one quantum of the metered stereo signal.
    pub fn push(&mut self, l: &[f32; Q], r: &[f32; Q]) {
        let mut s = 0f64;
        for k in 0..Q {
            // ChannelSplitter(1) down-mix: 0.5·(L + R) in f32
            let m = 0.5 * (l[k] + r[k]);
            s += (m * m) as f64;
        }
        self.ss[self.n % 4] = s;
        self.n += 1;
    }

    /// Mean square of the analyser window (the older 256 of the last 512 frames; quanta not yet
    /// rendered count as the analyser's initial zeros).
    pub fn mean_square(&self) -> f32 {
        let a = self.ss[self.n % 4];
        let b = self.ss[(self.n + 1) % 4];
        ((a + b) / (2 * Q) as f64) as f32
    }
}

/// The control side: `Meter.getValue()`'s smoothing.
#[derive(Debug, Clone, Copy)]
pub struct Meter {
    rms: f64,
    pub smoothing: f64,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            rms: 0.0,
            smoothing: 0.8,
        }
    }
}

impl Meter {
    /// One `getValue()` read of a window with mean square `ms`: the smoothed level in dB.
    pub fn read(&mut self, ms: f32) -> f64 {
        let rms = (ms.max(0.0) as f64).sqrt();
        self.rms = rms.max(self.rms * self.smoothing);
        20.0 * (self.rms.ln() / std::f64::consts::LN_10)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_the_older_half_of_the_last_four_quanta() {
        let mut w = MeterWindow::default();
        let one = [1f32; Q];
        let zero = [0f32; Q];
        assert_eq!(w.mean_square(), 0.0);
        w.push(&one, &one);
        w.push(&zero, &zero);
        w.push(&zero, &zero);
        // last 512 = [zeros (initial), one, zero, zero] → window [initial zeros, one]
        assert_eq!(w.mean_square(), 0.5);
        w.push(&zero, &zero);
        // [one, zero, zero, zero] → window [one, zero]
        assert_eq!(w.mean_square(), 0.5);
        w.push(&zero, &zero);
        assert_eq!(w.mean_square(), 0.0);
        // a hard-left signal is metered at half amplitude (mono down-mix)
        let mut w = MeterWindow::default();
        for _ in 0..4 {
            w.push(&one, &zero);
        }
        assert_eq!(w.mean_square(), 0.25);
    }

    #[test]
    fn rises_at_once_and_falls_by_the_smoothing_per_read() {
        let mut m = Meter::default();
        assert_eq!(m.read(0.0), f64::NEG_INFINITY);
        assert!((m.read(1.0) - 0.0).abs() < 1e-12);
        let db = m.read(0.0);
        assert!((db - 20.0 * 0.8f64.log10()).abs() < 1e-9);
        assert!((m.read(0.0) - 20.0 * 0.64f64.log10()).abs() < 1e-9);
    }
}
