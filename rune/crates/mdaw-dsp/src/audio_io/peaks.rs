//! Waveform peaks from an [`AudioBuffer`], reusing the already-ported helpers in `mdaw-model`
//! (`computePeaksFromBuffer` of `waveformHelpers.ts`, `computePeaks` of
//! `components/beat/waveform.ts`).

use mdaw_model::beat_helpers::{BufferLike, Peaks, compute_peaks};
use mdaw_model::notes::compute_peaks_from_buffer;

use crate::buffer::AudioBuffer;

impl BufferLike for AudioBuffer {
    fn number_of_channels(&self) -> usize {
        self.channel_count()
    }
    fn length(&self) -> usize {
        self.frames()
    }
    fn sample_rate(&self) -> f64 {
        f64::from(self.sample_rate)
    }
    fn channel_data(&self, channel: usize) -> &[f32] {
        &self.channels[channel]
    }
}

/// `computePeaksFromBuffer(buffer, numPeaks)` (SampleManager / waveformHelpers): absolute max
/// per bucket over all channels.
pub fn abs_peaks(buffer: &AudioBuffer, num_peaks: usize) -> Vec<f32> {
    let channels: Vec<&[f32]> = buffer.channels.iter().map(Vec::as_slice).collect();
    compute_peaks_from_buffer(&channels, buffer.frames(), num_peaks)
}

/// `computePeaks(buffer, columns)` (beat waveform): min/max per column over all channels.
pub fn min_max_peaks(buffer: &AudioBuffer, columns: f64) -> Peaks {
    compute_peaks(buffer, columns)
}
