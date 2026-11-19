//! Device enumeration and buffer-size selection.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SampleFormat, SupportedBufferSize};

use crate::HostError;

/// Settings `bufferSizeHint` range (Settings → Audio offers 128..2048).
pub const BUFFER_SIZE_MIN: u32 = 128;
pub const BUFFER_SIZE_MAX: u32 = 2048;

/// An output device as listed for the settings UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    /// Default output config ([`crate::OutputHost`] opens at its context rate when the device
    /// supports it, else with this config, resampled).
    pub default_sample_rate: u32,
    pub channels: u16,
    pub sample_format: String,
    /// Supported buffer sizes in frames, when the backend reports them.
    pub buffer_size_range: Option<(u32, u32)>,
}

/// What a stream asked the device for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferRequest {
    /// `BufferSize::Fixed(frames)`.
    Fixed(u32),
    /// The backend's default (the device reported no range, or the fixed request failed).
    Default,
}

impl BufferRequest {
    pub fn to_cpal(self) -> BufferSize {
        match self {
            Self::Fixed(n) => BufferSize::Fixed(n),
            Self::Default => BufferSize::Default,
        }
    }
}

/// Settings `bufferSizeHint` → frames in [`BUFFER_SIZE_MIN`]..=[`BUFFER_SIZE_MAX`] (0 / garbage →
/// the settings default 256 is the caller's business; 0 here means "min").
pub fn clamp_buffer_size_hint(hint: u32) -> u32 {
    hint.clamp(BUFFER_SIZE_MIN, BUFFER_SIZE_MAX)
}

/// The buffer size to request: the clamped hint as `Fixed`, moved into the device's supported
/// range when it reports one; `Default` when it reports none or no hint is given.
pub fn choose_buffer_size(hint: Option<u32>, supported: &SupportedBufferSize) -> BufferRequest {
    let Some(hint) = hint else {
        return BufferRequest::Default;
    };
    match *supported {
        SupportedBufferSize::Range { min, max } if min <= max && max > 0 => {
            BufferRequest::Fixed(clamp_buffer_size_hint(hint).clamp(min.max(1), max))
        }
        _ => BufferRequest::Default,
    }
}

/// One `supported_output_configs()` entry, reduced to what config selection needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigRange {
    pub channels: u16,
    pub format: SampleFormat,
    pub min_rate: u32,
    pub max_rate: u32,
    pub buffer_range: Option<(u32, u32)>,
}

impl From<&cpal::SupportedStreamConfigRange> for ConfigRange {
    fn from(r: &cpal::SupportedStreamConfigRange) -> Self {
        Self {
            channels: r.channels(),
            format: r.sample_format(),
            min_rate: r.min_sample_rate().0,
            max_rate: r.max_sample_rate().0,
            buffer_range: match r.buffer_size() {
                SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                SupportedBufferSize::Unknown => None,
            },
        }
    }
}

/// Sample formats the host can write (cpal's 24-bit formats are not).
pub(crate) fn format_supported(format: SampleFormat) -> bool {
    use SampleFormat::*;
    matches!(
        format,
        F32 | F64 | I8 | I16 | I32 | I64 | U8 | U16 | U32 | U64
    )
}

/// The index of the config range to open at `rate` (the context rate), or None when no range
/// supports it (then the host opens the default config and resamples). Among the ranges that
/// contain `rate` (with ≥ 1 channel and a writable format) it prefers, in order: the default
/// config's channel count, the default config's sample format, at least two channels, f32, fewer
/// channels; ties keep the backend's order.
pub fn choose_output_config(
    ranges: &[ConfigRange],
    rate: u32,
    default_channels: u16,
    default_format: SampleFormat,
) -> Option<usize> {
    ranges
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.channels >= 1
                && format_supported(r.format)
                && r.min_rate <= rate
                && rate <= r.max_rate
        })
        .max_by_key(|(i, r)| {
            (
                r.channels == default_channels,
                r.format == default_format,
                r.channels >= 2,
                r.format == SampleFormat::F32,
                std::cmp::Reverse(r.channels),
                std::cmp::Reverse(*i),
            )
        })
        .map(|(i, _)| i)
}

pub(crate) fn device_name(device: &cpal::Device) -> String {
    #[allow(deprecated)] // cpal 0.16: `name` is the stable identifier the settings store
    device
        .name()
        .unwrap_or_else(|_| "Unknown device".to_string())
}

fn info_of(device: &cpal::Device, default_name: Option<&str>) -> Option<DeviceInfo> {
    let config = device.default_output_config().ok()?;
    let name = device_name(device);
    Some(DeviceInfo {
        is_default: default_name == Some(name.as_str()),
        name,
        default_sample_rate: config.sample_rate().0,
        channels: config.channels(),
        sample_format: config.sample_format().to_string(),
        buffer_size_range: match config.buffer_size() {
            SupportedBufferSize::Range { min, max } => Some((*min, *max)),
            SupportedBufferSize::Unknown => None,
        },
    })
}

/// Every output device with a usable default config.
pub fn list_output_devices() -> Result<Vec<DeviceInfo>, HostError> {
    let host = cpal::default_host();
    let default_name = host.default_output_device().map(|d| device_name(&d));
    let devices = host
        .output_devices()
        .map_err(|e| HostError::Devices(e.to_string()))?;
    Ok(devices
        .filter_map(|d| info_of(&d, default_name.as_deref()))
        .collect())
}

/// The system default output device, if any.
pub fn default_output_device_info() -> Option<DeviceInfo> {
    let host = cpal::default_host();
    let device = host.default_output_device()?;
    let name = device_name(&device);
    info_of(&device, Some(&name))
}

/// The named output device (exact name), else None.
pub(crate) fn find_output_device(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    host.output_devices().ok()?.find(|d| device_name(d) == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_is_clamped_to_settings_and_device_range() {
        let range = SupportedBufferSize::Range { min: 15, max: 4096 };
        assert_eq!(
            choose_buffer_size(Some(256), &range),
            BufferRequest::Fixed(256)
        );
        assert_eq!(
            choose_buffer_size(Some(64), &range),
            BufferRequest::Fixed(128)
        );
        assert_eq!(
            choose_buffer_size(Some(9999), &range),
            BufferRequest::Fixed(2048)
        );
        let narrow = SupportedBufferSize::Range {
            min: 512,
            max: 1024,
        };
        assert_eq!(
            choose_buffer_size(Some(256), &narrow),
            BufferRequest::Fixed(512)
        );
        assert_eq!(
            choose_buffer_size(Some(2048), &narrow),
            BufferRequest::Fixed(1024)
        );
        assert_eq!(
            choose_buffer_size(Some(256), &SupportedBufferSize::Unknown),
            BufferRequest::Default
        );
        assert_eq!(choose_buffer_size(None, &range), BufferRequest::Default);
        assert_eq!(BufferRequest::Fixed(256).to_cpal(), BufferSize::Fixed(256));
        assert_eq!(clamp_buffer_size_hint(0), 128);
    }

    fn range(channels: u16, format: SampleFormat, min: u32, max: u32) -> ConfigRange {
        ConfigRange {
            channels,
            format,
            min_rate: min,
            max_rate: max,
            buffer_range: Some((14, 4096)),
        }
    }

    #[test]
    fn config_selection_runs_at_the_context_rate() {
        use SampleFormat::*;
        // A CoreAudio-style list: one entry per discrete rate.
        let mac = [
            range(2, F32, 44_100, 44_100),
            range(2, F32, 48_000, 48_000),
            range(2, F32, 96_000, 96_000),
        ];
        assert_eq!(choose_output_config(&mac, 48_000, 2, F32), Some(1));
        assert_eq!(choose_output_config(&mac, 44_100, 2, F32), Some(0));
        assert_eq!(choose_output_config(&mac, 22_050, 2, F32), None); // → resample
        // Prefer the default channel count, then the default format.
        let mixed = [
            range(1, F32, 8_000, 192_000),
            range(8, I16, 8_000, 192_000),
            range(2, I16, 8_000, 192_000),
            range(2, F32, 8_000, 192_000),
            range(2, I24, 8_000, 192_000),
        ];
        assert_eq!(choose_output_config(&mixed, 48_000, 2, F32), Some(3));
        assert_eq!(choose_output_config(&mixed, 48_000, 2, I16), Some(2));
        assert_eq!(choose_output_config(&mixed, 48_000, 8, F32), Some(1));
        // The default's channel count is unavailable: stereo f32 beats mono or 8 channels.
        assert_eq!(choose_output_config(&mixed, 48_000, 6, I32), Some(3));
        // Unwritable formats and rates outside every range are never chosen.
        let only_i24 = [range(2, I24, 48_000, 48_000), range(2, F32, 44_100, 44_100)];
        assert_eq!(choose_output_config(&only_i24, 48_000, 2, I24), None);
        assert_eq!(choose_output_config(&[], 48_000, 2, F32), None);
        // Ties keep the backend's order.
        let dup = [range(2, F32, 8_000, 96_000), range(2, F32, 48_000, 48_000)];
        assert_eq!(choose_output_config(&dup, 48_000, 2, F32), Some(0));
        // Mono-only device: still opens at the context rate.
        let mono = [range(1, I16, 48_000, 48_000)];
        assert_eq!(choose_output_config(&mono, 48_000, 1, I16), Some(0));
    }
}
