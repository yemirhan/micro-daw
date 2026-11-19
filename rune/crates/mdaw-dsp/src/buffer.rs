//! Shared audio buffer type (≙ Web Audio `AudioBuffer`): planar f32 channels at a sample rate.
//!
//! # Invariants
//!
//! A *valid* buffer (see [`AudioBuffer::is_valid`]) has
//! - `sample_rate > 0`,
//! - at least one channel,
//! - every channel the same length (no ragged channels).
//!
//! Zero frames is valid (an empty clip). The fields stay public so existing code can build and
//! move buffers with struct literals; code that takes channel data from outside should go
//! through the checked constructors [`AudioBuffer::from_channels`] / [`AudioBuffer::try_new`].
//! [`AudioBuffer::frames`] reads the first channel, so it is only meaningful on a valid buffer.

use std::sync::Arc;

/// A buffer shared between threads / caches without copying.
pub type SharedBuffer = Arc<AudioBuffer>;

#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    /// Frames per second; > 0 on a valid buffer.
    pub sample_rate: u32,
    /// Planar channel data; at least one channel, every channel the same length.
    pub channels: Vec<Vec<f32>>,
}

/// Why channel data does not form a valid [`AudioBuffer`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferError {
    /// `sample_rate == 0`.
    ZeroSampleRate,
    /// No channels.
    NoChannels,
    /// Channel `channel` has `len` frames; channel 0 has `expected`.
    RaggedChannels {
        channel: usize,
        len: usize,
        expected: usize,
    },
}

impl std::fmt::Display for BufferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ZeroSampleRate => write!(f, "audio buffer sample rate is 0"),
            Self::NoChannels => write!(f, "audio buffer has no channels"),
            Self::RaggedChannels {
                channel,
                len,
                expected,
            } => write!(
                f,
                "audio buffer channel {channel} has {len} frames, expected {expected}"
            ),
        }
    }
}

impl std::error::Error for BufferError {}

fn check(sample_rate: u32, channels: &[Vec<f32>]) -> Result<(), BufferError> {
    if sample_rate == 0 {
        return Err(BufferError::ZeroSampleRate);
    }
    let Some(first) = channels.first() else {
        return Err(BufferError::NoChannels);
    };
    let expected = first.len();
    match channels.iter().position(|c| c.len() != expected) {
        Some(channel) => Err(BufferError::RaggedChannels {
            channel,
            len: channels[channel].len(),
            expected,
        }),
        None => Ok(()),
    }
}

impl AudioBuffer {
    /// Silent buffer of `channels` × `frames`. Infallible for compatibility; debug builds assert
    /// the invariants (`sample_rate > 0`, `channels > 0`). See [`AudioBuffer::try_new`].
    pub fn new(sample_rate: u32, channels: usize, frames: usize) -> Self {
        debug_assert!(sample_rate > 0, "AudioBuffer::new: sample rate 0");
        debug_assert!(channels > 0, "AudioBuffer::new: no channels");
        Self {
            sample_rate,
            channels: vec![vec![0.0; frames]; channels],
        }
    }

    /// Checked [`AudioBuffer::new`].
    pub fn try_new(sample_rate: u32, channels: usize, frames: usize) -> Result<Self, BufferError> {
        if sample_rate == 0 {
            return Err(BufferError::ZeroSampleRate);
        }
        if channels == 0 {
            return Err(BufferError::NoChannels);
        }
        Ok(Self::new(sample_rate, channels, frames))
    }

    /// Buffer from planar channel data, rejecting a zero rate, zero channels and ragged
    /// channels.
    pub fn from_channels(sample_rate: u32, channels: Vec<Vec<f32>>) -> Result<Self, BufferError> {
        check(sample_rate, &channels)?;
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    /// One-channel buffer (always rectangular). Debug builds assert `sample_rate > 0`.
    pub fn mono(sample_rate: u32, data: Vec<f32>) -> Self {
        debug_assert!(sample_rate > 0, "AudioBuffer::mono: sample rate 0");
        Self {
            sample_rate,
            channels: vec![data],
        }
    }

    /// Whether the buffer satisfies the module invariants.
    pub fn is_valid(&self) -> bool {
        self.validate().is_ok()
    }

    /// The first broken invariant, if any.
    pub fn validate(&self) -> Result<(), BufferError> {
        check(self.sample_rate, &self.channels)
    }

    /// Wrap in an [`Arc`] for sharing.
    pub fn into_shared(self) -> SharedBuffer {
        Arc::new(self)
    }

    /// Samples of channel `index`, or `None` past the last channel.
    pub fn channel(&self, index: usize) -> Option<&[f32]> {
        self.channels.get(index).map(Vec::as_slice)
    }

    /// Mutable samples of channel `index` (the length is fixed, so the buffer stays rectangular).
    pub fn channel_mut(&mut self, index: usize) -> Option<&mut [f32]> {
        self.channels.get_mut(index).map(Vec::as_mut_slice)
    }

    /// Frames per channel (the length of channel 0; 0 without channels).
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Length in seconds. 0 for a zero sample rate rather than NaN / infinity.
    pub fn duration(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.frames() as f64 / self.sample_rate as f64
    }

    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_channels_validates() {
        let b = AudioBuffer::from_channels(48000, vec![vec![0.0; 4], vec![1.0; 4]]).unwrap();
        assert_eq!((b.channel_count(), b.frames()), (2, 4));
        assert!(b.is_valid());
        assert_eq!(b.channel(1), Some(&[1.0f32; 4][..]));
        assert_eq!(b.channel(2), None);
        assert_eq!(
            AudioBuffer::from_channels(0, vec![vec![0.0]]),
            Err(BufferError::ZeroSampleRate)
        );
        assert_eq!(
            AudioBuffer::from_channels(44100, vec![]),
            Err(BufferError::NoChannels)
        );
        assert_eq!(
            AudioBuffer::from_channels(44100, vec![vec![0.0; 3], vec![0.0; 3], vec![0.0; 2]]),
            Err(BufferError::RaggedChannels {
                channel: 2,
                len: 2,
                expected: 3
            })
        );
        // Zero frames is a valid (empty) buffer.
        assert!(AudioBuffer::from_channels(44100, vec![vec![], vec![]]).is_ok());
    }

    #[test]
    fn try_new_and_literal_validation() {
        assert_eq!(
            AudioBuffer::try_new(0, 1, 1),
            Err(BufferError::ZeroSampleRate)
        );
        assert_eq!(
            AudioBuffer::try_new(44100, 0, 1),
            Err(BufferError::NoChannels)
        );
        let b = AudioBuffer::try_new(44100, 2, 10).unwrap();
        assert_eq!(b, AudioBuffer::new(44100, 2, 10));
        let ragged = AudioBuffer {
            sample_rate: 44100,
            channels: vec![vec![0.0; 2], vec![0.0; 3]],
        };
        assert!(!ragged.is_valid());
        let zero_rate = AudioBuffer {
            sample_rate: 0,
            channels: vec![vec![0.0; 2]],
        };
        assert_eq!(zero_rate.validate(), Err(BufferError::ZeroSampleRate));
        assert_eq!(zero_rate.duration(), 0.0);
    }

    #[test]
    fn channel_mut_and_shared() {
        let mut b = AudioBuffer::mono(44100, vec![0.0; 3]);
        b.channel_mut(0).unwrap()[1] = 0.5;
        let s: SharedBuffer = b.into_shared();
        assert_eq!(s.channel(0), Some(&[0.0, 0.5, 0.0][..]));
        assert!(s.is_valid());
    }
}
