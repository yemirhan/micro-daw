//! MIDI settings and persistence.
//!
//! **Single owner of the settings file.** The [`SettingsStore`] (the `localStorage` file) is
//! owned by the UI thread and written only there. The MIDI thread never touches the disk:
//! - the controller profile ([`DeferredProfileStore`], the [`ProfileStore`] of the
//!   `ControllerMapping` living on the MIDI thread) only *queues* each save; the UI thread
//!   writes the latest one with [`ProfileSaves::apply`] (TS key `micro-daw:controller`);
//! - the preferred device is written by the host's persist callback, which runs on the UI
//!   thread ([`enable_with_settings`]).
//!
//! **Last device** (Electron gap fixed in Rust: the TS never persists it and never reads
//! `settings.midi.autoConnectLastDevice`). Stored under [`MIDI_LAST_DEVICE_KEY`] as
//! `{"id": …, "name": …}`. The key is Rust-only, so `micro-daw-settings` keeps exactly the
//! Electron format.
//!
//! **Split point** ([`MIDI_SPLIT_POINT_KEY`], a JSON number; default 60): the same key and
//! shape as the Electron app's `localStorage` (`src/services/midiSplit.ts`).

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

use crossbeam_channel::{Receiver, Sender, unbounded};
use mdaw_model::controller::{CONTROLLER_STORAGE_KEY, ProfileStore};
use mdaw_model::settings::{SettingsStore, read_app_settings};

use crate::devices::{LastDevice, normalize_port_name};
use crate::host::MidiHost;
use crate::router::{DEFAULT_SPLIT_POINT, clamp_split_point};

pub const MIDI_LAST_DEVICE_KEY: &str = "micro-daw:midi-last-device";
/// Same key as the Electron app (`MIDI_SPLIT_POINT_KEY` in `src/services/midiSplit.ts`).
pub const MIDI_SPLIT_POINT_KEY: &str = "micro-daw:midi-split-point";

/// `readSplitPoint()`: the stored split point (a JSON number, truncated and clamped to 0..=127),
/// else [`DEFAULT_SPLIT_POINT`].
pub fn read_split_point(s: &SettingsStore) -> i64 {
    s.get_item(MIDI_SPLIT_POINT_KEY)
        .and_then(|v| serde_json::from_str::<f64>(v).ok())
        .filter(|v| v.is_finite())
        .map_or(DEFAULT_SPLIT_POINT, |v| clamp_split_point(v.trunc() as i64))
}

/// `writeSplitPoint(note)`: stores the clamped value and returns it.
pub fn write_split_point(s: &mut SettingsStore, note: i64) -> io::Result<i64> {
    let v = clamp_split_point(note);
    s.set_item(MIDI_SPLIT_POINT_KEY, v.to_string())?;
    Ok(v)
}

/// The controller profile store of the MIDI thread (see the module docs): loads the value read
/// at startup, queues every save for the UI thread.
pub struct DeferredProfileStore {
    value: Option<String>,
    tx: Sender<String>,
}

/// The UI thread's end of [`DeferredProfileStore`].
pub struct ProfileSaves {
    rx: Receiver<String>,
}

/// A profile store seeded from the settings, and the queue of its saves.
pub fn profile_store(s: &SettingsStore) -> (DeferredProfileStore, ProfileSaves) {
    let (tx, rx) = unbounded();
    (
        DeferredProfileStore {
            value: s.get_item(CONTROLLER_STORAGE_KEY).map(str::to_owned),
            tx,
        },
        ProfileSaves { rx },
    )
}

impl ProfileStore for DeferredProfileStore {
    fn load(&self) -> Option<String> {
        self.value.clone()
    }
    fn save(&mut self, s: &str) {
        self.value = Some(s.to_owned());
        let _ = self.tx.send(s.to_owned());
    }
}

impl ProfileSaves {
    /// Write the latest queued profile (if any) to the settings. Returns true when one was
    /// written. Call it on the UI thread (every frame, or when the MIDI waker fires).
    pub fn apply(&self, s: &mut SettingsStore) -> io::Result<bool> {
        let Some(latest) = self.rx.try_iter().last() else {
            return Ok(false);
        };
        s.set_item(CONTROLLER_STORAGE_KEY, latest)?;
        Ok(true)
    }
}

/// `enable()` wired to the settings: restores the last device when
/// `settings.midi.autoConnectLastDevice` is on ([`device_to_restore`]) and persists every
/// later preferred-device change worth persisting (an explicit selection, an MPK, a restored
/// device re-keyed to a new id) with [`write_last_device`]. Runs on the UI thread, which owns
/// `settings`.
pub fn enable_with_settings<St: ProfileStore + Send + 'static>(
    host: &mut MidiHost<St>,
    settings: Rc<RefCell<SettingsStore>>,
) -> bool {
    let last = device_to_restore(&settings.borrow());
    host.set_persist(Box::new(move |d| {
        // a full disk must not take MIDI down; the next change retries
        let _ = write_last_device(&mut settings.borrow_mut(), d);
    }));
    host.enable(last)
}

/// The stored last device (None when missing or unreadable).
pub fn read_last_device(s: &SettingsStore) -> Option<LastDevice> {
    serde_json::from_str(s.get_item(MIDI_LAST_DEVICE_KEY)?).ok()
}

pub fn write_last_device(s: &mut SettingsStore, d: &LastDevice) -> io::Result<()> {
    let d = LastDevice {
        id: d.id.clone(),
        name: normalize_port_name(&d.name).to_owned(),
    };
    let v = serde_json::to_string(&d).map_err(io::Error::other)?;
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

    /// Review finding 8 without a MIDI backend: what `enable_with_settings` + the host's
    /// persist path do, through a settings file on disk, across "restarts".
    #[test]
    fn last_device_round_trip_on_disk_across_restarts() {
        use crate::devices::{DeviceInfo, DeviceSelector};
        let dir = std::env::temp_dir().join(format!("mdaw-midi-last-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let iac = DeviceInfo::new("iac", "IAC Driver Bus 1");
        let key = DeviceInfo::new("20:0", "Keystation 49 20:0");
        let mpk = DeviceInfo::new("mpk", "MPK mini Play mk3");
        // the host's `apply`: persist when the selector reports a change worth persisting
        let start = |inputs: &[DeviceInfo], pick: Option<&str>| {
            let mut settings = SettingsStore::open(&dir);
            let mut sel = DeviceSelector::with_last_device(device_to_restore(&settings));
            sel.auto_select(inputs);
            if let Some(id) = pick {
                sel.select(id, inputs);
            }
            if sel.take_preferred_change()
                && let Some(d) = sel.persisted_preferred(inputs)
            {
                write_last_device(&mut settings, &d).unwrap();
            }
            sel.active().map(str::to_owned)
        };
        // 1st start: only the IAC bus → used, not persisted
        assert_eq!(
            start(std::slice::from_ref(&iac), None).as_deref(),
            Some("iac")
        );
        assert_eq!(read_last_device(&SettingsStore::open(&dir)), None);
        // 2nd start: the user picks the Keystation → persisted (name without the ALSA suffix)
        start(&[iac.clone(), key.clone()], Some("20:0"));
        let stored = read_last_device(&SettingsStore::open(&dir)).unwrap();
        assert_eq!(
            (stored.id.as_str(), stored.name.as_str()),
            ("20:0", "Keystation 49")
        );
        // 3rd start: restored over the MPK, under a new ALSA client number
        let key2 = DeviceInfo::new("24:0", "Keystation 49 24:0");
        assert_eq!(
            start(&[mpk.clone(), iac.clone(), key2.clone()], None).as_deref(),
            Some("24:0")
        );
        assert_eq!(
            read_last_device(&SettingsStore::open(&dir)).unwrap().id,
            "24:0"
        );
        // 4th start, absent: the MPK is used, the Keystation stays remembered
        assert_eq!(
            start(&[iac.clone(), mpk.clone()], None).as_deref(),
            Some("mpk")
        );
        assert_eq!(
            read_last_device(&SettingsStore::open(&dir)).unwrap().id,
            "24:0"
        );
        // setting off: the TS choice
        mdaw_model::settings::update_app_settings(
            &mut SettingsStore::open(&dir),
            &json!({"midi": {"autoConnectLastDevice": false}}),
        )
        .unwrap();
        assert_eq!(start(&[iac, key2, mpk], None).as_deref(), Some("mpk"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn split_point_round_trip() {
        let mut s = SettingsStore::in_memory();
        assert_eq!(read_split_point(&s), 60);
        assert_eq!(write_split_point(&mut s, 48).unwrap(), 48);
        assert_eq!(s.get_item(MIDI_SPLIT_POINT_KEY), Some("48"));
        assert_eq!(read_split_point(&s), 48);
        assert_eq!(write_split_point(&mut s, 300).unwrap(), 127);
        s.set_item(MIDI_SPLIT_POINT_KEY, "52.9").unwrap();
        assert_eq!(read_split_point(&s), 52);
        s.set_item(MIDI_SPLIT_POINT_KEY, "\"x\"").unwrap();
        assert_eq!(read_split_point(&s), 60);
    }

    /// Profile saves made on the MIDI thread reach the file only through the UI thread.
    #[test]
    fn profile_saves_are_written_by_the_ui_thread() {
        let dir = std::env::temp_dir().join(format!("mdaw-midi-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut settings = SettingsStore::open(&dir);
        let (mut store, saves) = profile_store(&settings);
        assert_eq!(store.load(), None);
        let t = std::thread::spawn(move || {
            store.save("one");
            store.save("two");
            store
        });
        let store = t.join().unwrap();
        assert_eq!(store.load().as_deref(), Some("two"));
        assert!(
            SettingsStore::open(&dir)
                .get_item(CONTROLLER_STORAGE_KEY)
                .is_none()
        );
        assert!(saves.apply(&mut settings).unwrap());
        assert!(!saves.apply(&mut settings).unwrap());
        assert_eq!(
            SettingsStore::open(&dir).get_item(CONTROLLER_STORAGE_KEY),
            Some("two")
        );
        let (store, _) = profile_store(&SettingsStore::open(&dir));
        assert_eq!(store.load().as_deref(), Some("two"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
