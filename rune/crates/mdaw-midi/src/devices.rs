//! `MidiService`'s device logic (`src/services/MidiService.ts:111-196`) over a device list, with
//! no I/O: [`DeviceSelector`] decides which input to listen to and returns the attach / detach
//! operations for [`crate::host::MidiHost`] to carry out. [`HeldNotes`] is the service's
//! `heldNotes` map (note-offs synthesised when an input is lost).
//!
//! **Electron gap fixed in Rust** (inventory §3.1 / §5 gap 2): the TS keeps the preferred device
//! in memory only and never reads `settings.midi.autoConnectLastDevice`. Here the preferred
//! device can be seeded from the persisted last device ([`DeviceSelector::with_last_device`]) and
//! every change of the preferred device is reported ([`DeviceSelector::take_preferred_change`])
//! so the host can persist it. With the setting off, the selector starts with no preferred
//! device, which is exactly the TS behaviour (MPK, else the first input).

use serde::{Deserialize, Serialize};

/// `MidiDeviceInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub manufacturer: String,
    pub connected: bool,
}

impl DeviceInfo {
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            manufacturer: String::new(),
            connected: true,
        }
    }
}

/// `MPK_MINI_PATTERN` = `/mpk\s*mini/i`.
pub fn is_mpk_mini(name: &str) -> bool {
    let lower = name.to_lowercase();
    let b = lower.as_bytes();
    let mut i = 0;
    while let Some(p) = lower[i..].find("mpk") {
        let mut j = i + p + 3;
        while j < b.len() && is_js_space(lower[j..].chars().next().unwrap_or('x')) {
            j += lower[j..].chars().next().map_or(1, char::len_utf8);
        }
        if lower[j..].starts_with("mini") {
            return true;
        }
        i += p + 1;
    }
    false
}

/// JS `\s`.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// What the host has to do to the MIDI connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceOp {
    /// Drop the active connection (and release its held notes).
    Detach,
    /// Open a connection to this input.
    Attach(String),
}

/// The persisted last device (`micro-daw:midi-last-device`, a Rust-only key): matched by id,
/// then by exact name (ALSA port ids change across replugs; CoreMIDI unique ids do not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastDevice {
    pub id: String,
    pub name: String,
}

/// The device half of `MidiService` (see the module docs).
#[derive(Debug, Default, Clone)]
pub struct DeviceSelector {
    preferred: Option<String>,
    /// Name of the preferred device when it came from settings (id may be stale).
    preferred_name: Option<String>,
    active: Option<String>,
    preferred_changed: bool,
}

impl DeviceSelector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeded with the persisted last device (`autoConnectLastDevice` on).
    pub fn with_last_device(last: Option<LastDevice>) -> Self {
        let mut s = Self::new();
        if let Some(l) = last {
            s.preferred = Some(l.id);
            s.preferred_name = Some(l.name);
        }
        s
    }

    pub fn active(&self) -> Option<&str> {
        self.active.as_deref()
    }
    pub fn preferred(&self) -> Option<&str> {
        self.preferred.as_deref()
    }

    /// The preferred device changed since the last call (persist it).
    pub fn take_preferred_change(&mut self) -> bool {
        std::mem::take(&mut self.preferred_changed)
    }

    fn set_preferred(&mut self, id: &str) {
        if self.preferred.as_deref() != Some(id) {
            self.preferred = Some(id.to_owned());
            self.preferred_changed = true;
        }
        self.preferred_name = None;
    }

    /// The preferred device as present in `inputs` (by id; a settings-seeded one also by name,
    /// which re-keys it to the current id).
    fn find_preferred<'a>(&mut self, inputs: &'a [DeviceInfo]) -> Option<&'a DeviceInfo> {
        let id = self.preferred.as_deref()?;
        if let Some(d) = inputs.iter().find(|d| d.id == id) {
            return Some(d);
        }
        let name = self.preferred_name.as_deref()?;
        let d = inputs.iter().find(|d| d.name == name)?;
        self.set_preferred(&d.id.clone());
        Some(d)
    }

    fn attach_to(&mut self, id: &str, inputs: &[DeviceInfo], ops: &mut Vec<DeviceOp>) {
        if !inputs.iter().any(|d| d.id == id) {
            return;
        }
        if self.active.as_deref() == Some(id) {
            return;
        }
        ops.push(DeviceOp::Detach);
        self.active = Some(id.to_owned());
        ops.push(DeviceOp::Attach(id.to_owned()));
    }

    /// `autoSelectDevice()`: the preferred device, else an MPK mini, else the first input. The
    /// first automatic pick becomes the preferred device.
    pub fn auto_select(&mut self, inputs: &[DeviceInfo]) -> Vec<DeviceOp> {
        let mut ops = Vec::new();
        self.auto_select_into(inputs, &mut ops);
        ops
    }

    fn auto_select_into(&mut self, inputs: &[DeviceInfo], ops: &mut Vec<DeviceOp>) {
        if let Some(p) = self.find_preferred(inputs) {
            let id = p.id.clone();
            self.attach_to(&id, inputs, ops);
            return;
        }
        let Some(fallback) = inputs
            .iter()
            .find(|d| is_mpk_mini(&d.name))
            .or_else(|| inputs.first())
        else {
            return;
        };
        let id = fallback.id.clone();
        if self.preferred.is_none() {
            self.set_preferred(&id);
        }
        self.attach_to(&id, inputs, ops);
    }

    /// `selectDevice(id)`: ignored when absent; else preferred + attach.
    pub fn select(&mut self, id: &str, inputs: &[DeviceInfo]) -> Vec<DeviceOp> {
        let mut ops = Vec::new();
        if !inputs.iter().any(|d| d.id == id) {
            return ops;
        }
        self.set_preferred(id);
        self.attach_to(id, inputs, &mut ops);
        ops
    }

    /// `handleDeviceChange` (a port appeared or disappeared). The caller then notifies the
    /// device-change listeners.
    pub fn device_change(&mut self, inputs: &[DeviceInfo]) -> Vec<DeviceOp> {
        let mut ops = Vec::new();
        let active_gone = self
            .active
            .as_deref()
            .is_some_and(|a| !inputs.iter().any(|d| d.id == a));
        if active_gone {
            ops.push(DeviceOp::Detach);
            self.active = None;
            self.auto_select_into(inputs, &mut ops);
        } else {
            // (`find_preferred` may re-key a settings-seeded device to its current id first)
            let preferred_back = self.active.is_some()
                && self.find_preferred(inputs).is_some()
                && self.active != self.preferred;
            if self.active.is_none() || preferred_back {
                self.auto_select_into(inputs, &mut ops);
            }
        }
        ops
    }

    /// `dispose()`: detach and forget the preferred device.
    pub fn dispose(&mut self) -> Vec<DeviceOp> {
        self.active = None;
        self.preferred = None;
        self.preferred_name = None;
        vec![DeviceOp::Detach]
    }
}

/// `heldNotes` (`"channel:note"` → note), in insertion order.
#[derive(Debug, Default, Clone)]
pub struct HeldNotes(Vec<(u8, u8)>);

impl HeldNotes {
    pub fn note_on(&mut self, note: u8, channel: u8) {
        if !self.0.contains(&(channel, note)) {
            self.0.push((channel, note));
        }
    }
    pub fn note_off(&mut self, note: u8, channel: u8) {
        self.0.retain(|k| *k != (channel, note));
    }
    /// `releaseHeldNotes()`: every held (note, channel), cleared.
    pub fn take(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.0)
            .into_iter()
            .map(|(c, n)| (n, c))
            .collect()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, name: &str) -> DeviceInfo {
        DeviceInfo::new(id, name)
    }

    #[test]
    fn mpk_pattern() {
        assert!(is_mpk_mini("MPK mini Play mk3"));
        assert!(is_mpk_mini("Akai MPKMINI"));
        assert!(is_mpk_mini("mpk \t mini"));
        assert!(!is_mpk_mini("MPK 249"));
        assert!(!is_mpk_mini("mini MPK"));
    }

    #[test]
    fn auto_select_prefers_mpk_then_first() {
        let mut s = DeviceSelector::new();
        let ops = s.auto_select(&[d("a", "IAC Bus"), d("b", "MPK mini 3")]);
        assert_eq!(ops, vec![DeviceOp::Detach, DeviceOp::Attach("b".into())]);
        assert_eq!(s.preferred(), Some("b"));
        assert!(s.take_preferred_change());
        let mut s = DeviceSelector::new();
        s.auto_select(&[d("a", "IAC Bus"), d("c", "Other")]);
        assert_eq!(s.active(), Some("a"));
    }

    #[test]
    fn last_device_reconnects_by_id_or_name() {
        let last = LastDevice {
            id: "old".into(),
            name: "Keystation".into(),
        };
        let mut s = DeviceSelector::with_last_device(Some(last.clone()));
        s.auto_select(&[d("x", "MPK mini"), d("new", "Keystation")]);
        assert_eq!(s.active(), Some("new"));
        assert_eq!(s.preferred(), Some("new"));
        assert!(s.take_preferred_change());
        // absent at start: fall back to the MPK, keep the remembered device preferred
        let mut s = DeviceSelector::with_last_device(Some(last));
        s.auto_select(&[d("x", "MPK mini")]);
        assert_eq!(s.active(), Some("x"));
        assert_eq!(s.preferred(), Some("old"));
        assert!(!s.take_preferred_change());
        // it comes back: re-attach
        let ops = s.device_change(&[d("x", "MPK mini"), d("k", "Keystation")]);
        assert_eq!(ops, vec![DeviceOp::Detach, DeviceOp::Attach("k".into())]);
    }

    #[test]
    fn held_notes_keep_insertion_order() {
        let mut h = HeldNotes::default();
        h.note_on(60, 1);
        h.note_on(38, 10);
        h.note_on(60, 1);
        h.note_off(38, 10);
        h.note_on(62, 2);
        assert_eq!(h.take(), vec![(60, 1), (62, 2)]);
        assert!(h.is_empty());
    }

    // --- MidiService.test.ts ----------------------------------------------------------------

    fn kbd() -> DeviceInfo {
        d("kbd", "MPK mini 3")
    }
    fn pads() -> DeviceInfo {
        d("pads", "Other Device")
    }

    #[test]
    fn auto_selects_the_mpk_and_reselect_is_a_no_op() {
        let mut s = DeviceSelector::new();
        let inputs = [kbd(), pads()];
        s.auto_select(&inputs);
        assert_eq!(s.active(), Some("kbd"));
        assert!(s.select("kbd", &inputs).is_empty());
        assert!(s.auto_select(&inputs).is_empty());
    }

    #[test]
    fn switching_devices_detaches_the_old_one_first() {
        let mut s = DeviceSelector::new();
        let inputs = [kbd(), pads()];
        s.auto_select(&inputs);
        assert_eq!(
            s.select("pads", &inputs),
            vec![DeviceOp::Detach, DeviceOp::Attach("pads".into())]
        );
        assert!(s.select("ghost", &inputs).is_empty());
        assert_eq!(s.active(), Some("pads"));
    }

    #[test]
    fn unplugging_the_active_device_picks_another_and_replug_reattaches() {
        let mut s = DeviceSelector::new();
        s.auto_select(&[kbd(), pads()]);
        let ops = s.device_change(&[pads()]);
        assert_eq!(
            ops,
            vec![
                DeviceOp::Detach,
                DeviceOp::Detach,
                DeviceOp::Attach("pads".into())
            ]
        );
        assert_eq!(s.active(), Some("pads"));
        // the preferred keyboard returns after the fallback
        s.device_change(&[kbd(), pads()]);
        assert_eq!(s.active(), Some("kbd"));
        // only input: unplug then replug
        let mut s = DeviceSelector::new();
        s.auto_select(&[kbd()]);
        s.device_change(&[]);
        assert_eq!(s.active(), None);
        s.device_change(&[kbd()]);
        assert_eq!(s.active(), Some("kbd"));
    }

    #[test]
    fn a_manual_choice_is_remembered_across_reconnects() {
        let mut s = DeviceSelector::new();
        s.auto_select(&[kbd(), pads()]);
        s.select("pads", &[kbd(), pads()]);
        let extra = d("extra", "Another Keyboard");
        s.device_change(&[kbd(), pads(), extra.clone()]);
        assert_eq!(s.active(), Some("pads"));
        s.device_change(&[kbd(), extra.clone()]);
        assert_eq!(s.active(), Some("kbd"));
        s.device_change(&[kbd(), pads(), extra]);
        assert_eq!(s.active(), Some("pads"));
    }
}
