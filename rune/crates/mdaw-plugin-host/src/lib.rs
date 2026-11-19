//! micro-daw plugin host.
//!
//! Hosts third-party audio plugins inside the app: effects (an amp simulator on an audio
//! track's input, say) and instruments (a synth played from the keyboard). The format hosted is
//! **Audio Unit v2** on macOS ([`au`]); other platforms compile to an empty host ([`scan`] finds
//! nothing, [`Instance::open`] fails), so the rest of the app needs no `cfg`.
//!
//! - [`scan`]: every installed effect / instrument, from the system's component registry (no
//!   plugin code is loaded: the list is instant and cannot crash).
//! - [`Instance`]: one loaded plugin. Opened, configured and inspected on the **main thread**
//!   (plugins expect that): state ([`Instance::state`] / [`Instance::set_state`], the plugin's
//!   own preset blob), latency, the editor window ([`Instance::open_editor`]).
//! - [`Renderer`]: the audio thread's half of an instance ([`Instance::renderer`]): renders
//!   blocks without allocating or locking on the host side. Only one renderer of an instance
//!   may render at a time (the engine swaps them at a block boundary, on the audio thread).
//!
//! The stereo layout is fixed: an effect processes stereo → stereo (a mono-in plugin gets the
//! mix of both channels), an instrument renders stereo.

use std::fmt;

#[cfg(target_os = "macos")]
pub mod au;
#[cfg(target_os = "macos")]
pub use au::{EditorWindow, Instance, Renderer, scan};

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::{EditorWindow, Instance, Renderer, scan};

/// The largest block a [`Renderer`] renders in one call (frames).
pub const MAX_BLOCK: usize = 4096;

/// What a plugin does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginKind {
    /// Audio in → audio out (`aufx`, and `aumf`: an effect that also takes MIDI).
    Effect,
    /// MIDI in → audio out (`aumu`).
    Instrument,
}

/// A four-character code as the system prints it (`"aufx"`, `"-NI-"`).
pub fn fourcc_string(code: u32) -> String {
    code.to_be_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_graphic() || *b == b' ' {
                *b as char
            } else {
                '?'
            }
        })
        .collect()
}

/// [`fourcc_string`]'s inverse (exactly four ASCII bytes).
pub fn fourcc_code(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    (b.len() == 4).then(|| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

/// A plugin's identity: stable across launches and machines (what a project stores).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PluginId {
    /// Component type (`aufx`, `aumf`, `aumu`).
    pub kind: u32,
    pub subtype: u32,
    pub manufacturer: u32,
}

impl PluginId {
    /// From the three four-character codes (`"aufx"`, `"NIGR"`, `"-NI-"`).
    pub fn from_codes(kind: &str, subtype: &str, manufacturer: &str) -> Option<Self> {
        Some(Self {
            kind: fourcc_code(kind)?,
            subtype: fourcc_code(subtype)?,
            manufacturer: fourcc_code(manufacturer)?,
        })
    }

    pub fn codes(&self) -> (String, String, String) {
        (
            fourcc_string(self.kind),
            fourcc_string(self.subtype),
            fourcc_string(self.manufacturer),
        )
    }

    pub fn plugin_kind(&self) -> Option<PluginKind> {
        match &self.kind.to_be_bytes() {
            b"aufx" | b"aumf" => Some(PluginKind::Effect),
            b"aumu" => Some(PluginKind::Instrument),
            _ => None,
        }
    }
}

/// An installed plugin ([`scan`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInfo {
    pub id: PluginId,
    pub kind: PluginKind,
    /// "Guitar Rig 6".
    pub name: String,
    /// "Native Instruments".
    pub vendor: String,
    pub version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginError {
    /// Not on this platform.
    Unsupported,
    /// No installed plugin has this id.
    NotFound,
    /// The plugin is neither an effect nor an instrument.
    WrongKind,
    /// Must be called on the main thread.
    NotMainThread,
    /// The plugin refused a step (`what` failed with OSStatus `status`).
    Failed { what: &'static str, status: i32 },
    /// The plugin has no editor view.
    NoEditor,
    /// The stored state is not a plugin state.
    BadState,
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("plugins are not supported on this platform"),
            Self::NotFound => f.write_str("the plugin is not installed"),
            Self::WrongKind => f.write_str("not an effect or instrument plugin"),
            Self::NotMainThread => f.write_str("plugins must be opened on the main thread"),
            Self::Failed { what, status } => write!(f, "the plugin failed to {what} ({status})"),
            Self::NoEditor => f.write_str("the plugin has no editor"),
            Self::BadState => f.write_str("the saved plugin state could not be read"),
        }
    }
}

impl std::error::Error for PluginError {}

/// What the plugin is told about the host transport (tempo-synced delays, arpeggiators). Written
/// by the audio thread before a block, read by the plugin during it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTransport {
    pub bpm: f64,
    /// Position in quarter notes at the block's first frame.
    pub beat: f64,
    pub playing: bool,
}

impl Default for HostTransport {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beat: 0.0,
            playing: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fourcc_round_trips() {
        let id = PluginId::from_codes("aufx", "NIGR", "-NI-").unwrap();
        assert_eq!(id.codes(), ("aufx".into(), "NIGR".into(), "-NI-".into()));
        assert_eq!(id.plugin_kind(), Some(PluginKind::Effect));
        assert_eq!(
            PluginId::from_codes("aumu", "ALab", "Artu")
                .unwrap()
                .plugin_kind(),
            Some(PluginKind::Instrument)
        );
        assert_eq!(PluginId::from_codes("aufx", "toolong", "-NI-"), None);
        assert_eq!(fourcc_string(0x0001_0203), "????");
        assert_eq!(
            PluginId::from_codes("augn", "abcd", "efgh")
                .unwrap()
                .plugin_kind(),
            None
        );
    }
}
