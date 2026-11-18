//! Shared audio buffer type (≙ Web Audio `AudioBuffer`): planar f32 channels at a sample rate.

#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    pub sample_rate: u32,
    /// Planar channel data; every channel has the same length.
    pub channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, channels: usize, frames: usize) -> Self {
        Self { sample_rate, channels: vec![vec![0.0; frames]; channels] }
    }

    pub fn mono(sample_rate: u32, data: Vec<f32>) -> Self {
        Self { sample_rate, channels: vec![data] }
    }

    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}
