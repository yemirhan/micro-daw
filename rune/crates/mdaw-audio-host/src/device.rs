//! Device enumeration and buffer-size selection.

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{BufferSize, SupportedBufferSize};

use crate::HostError;

/// Settings `bufferSizeHint` range (Settings → Audio offers 128..2048).
pub const BUFFER_SIZE_MIN: u32 = 128;
pub const BUFFER_SIZE_MAX: u32 = 2048;

/// An output device as listed for the settings UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
    /// Default output config (what [`crate::OutputHost`] opens with).
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
}
