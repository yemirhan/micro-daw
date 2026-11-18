//! The persisted preferred MIDI device (Electron gap fixed in Rust: the TS never persists it and
//! never reads `settings.midi.autoConnectLastDevice`).
//!
//! Stored in the settings store under [`MIDI_LAST_DEVICE_KEY`] as `{"id": …, "name": …}`. The key
//! is Rust-only, so `micro-daw-settings` keeps exactly the Electron format.

use std::io;

use mdaw_model::settings::{SettingsStore, read_app_settings};

use crate::devices::LastDevice;

pub const MIDI_LAST_DEVICE_KEY: &str = "micro-daw:midi-last-device";

/// The stored last device (None when missing or unreadable).
pub fn read_last_device(s: &SettingsStore) -> Option<LastDevice> {
    serde_json::from_str(s.get_item(MIDI_LAST_DEVICE_KEY)?).ok()
}

pub fn write_last_device(s: &mut SettingsStore, d: &LastDevice) -> io::Result<()> {
    let v = serde_json::to_string(d).map_err(io::Error::other)?;
    s.set_item(MIDI_LAST_DEVICE_KEY, v)
}

/// What [`crate::MidiHost::enable`] should reconnect to: the stored device when
/// `settings.midi.autoConnectLastDevice` is on, else None (the TS behaviour).
pub fn device_to_restore(s: &SettingsStore) -> Option<LastDevice> {
    if read_app_settings(s).midi.auto_connect_last_device {
        read_last_device(s)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn round_trip_and_setting_gate() {
        let mut s = SettingsStore::in_memory();
        assert_eq!(device_to_restore(&s), None);
        let d = LastDevice {
            id: "123".into(),
            name: "MPK mini Play mk3".into(),
        };
        write_last_device(&mut s, &d).unwrap();
        assert_eq!(read_last_device(&s), Some(d.clone()));
        assert_eq!(device_to_restore(&s), Some(d));
        mdaw_model::settings::update_app_settings(
            &mut s,
            &json!({"midi": {"autoConnectLastDevice": false}}),
        )
        .unwrap();
        assert_eq!(device_to_restore(&s), None);
        s.set_item(MIDI_LAST_DEVICE_KEY, "garbage").unwrap();
        assert_eq!(read_last_device(&s), None);
    }
}
