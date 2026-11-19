//! `MidiService`'s device logic (`src/services/MidiService.ts:111-196`) over a device list, with
//! no I/O: [`DeviceSelector`] decides which input to listen to and returns the attach / detach
//! operations for [`crate::host::MidiHost`] to carry out. [`HeldNotes`] is the service's
//! `heldNotes` map (note-offs synthesised when an input is lost).
//!
//! **Electron gap fixed in Rust** (inventory §3.1 / §5 gap 2): the TS keeps the preferred device
//! in memory only and never reads `settings.midi.autoConnectLastDevice`. Here the preferred
//! device can be seeded from the persisted last device ([`DeviceSelector::with_last_device`]),
//! and a change worth persisting is reported ([`DeviceSelector::take_preferred_change`]): an
//! explicit `select`, an automatic pick of an MPK mini, or a remembered device re-keyed to a new
//! id. The automatic pick of any other first input (an IAC bus, a virtual port) becomes the
//! in-memory preferred device like in the TS, but is **not** persisted, so a fallback can never
//! become sticky over the MPK across restarts. With the setting off, the selector starts with
//! no preferred device, which is exactly the TS behaviour (MPK, else the first input).
//!
//! **Name matching.** The preferred device keeps its id *and* its port name for the whole
//! session. When its id is gone but a port with the same (normalised, [`normalize_port_name`])
//! name is present, that port is the preferred device (re-keyed to its id): ALSA port ids change
//! across replugs, CoreMIDI unique ids do not. With several ports of that name, the one at the
//! same position among them is taken. The TS matches by id only; with stable ids (CoreMIDI,
//! WinMM, every golden session) the two agree.

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

/// A port name without the ALSA client:port suffix midir appends (`"MPK mini 3 20:0"` →
/// `"MPK mini 3"`): ALSA client numbers change across replugs, so names are compared without it.
pub fn normalize_port_name(name: &str) -> &str {
    let t = name.trim_end();
    let Some((head, tail)) = t.rsplit_once(char::is_whitespace) else {
        return t;
    };
    let mut parts = tail.split(':');
    let numeric =
        |p: Option<&str>| p.is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    if numeric(parts.next()) && numeric(parts.next()) && parts.next().is_none() {
        head.trim_end()
    } else {
        t
    }
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
    /// Normalised port name of the preferred device (kept for the whole session).
    preferred_name: Option<String>,
    /// Position of the preferred device among the ports of that name.
    preferred_ordinal: usize,
    /// The preferred device is one to persist (explicit choice, MPK, or restored).
    preferred_persist: bool,
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
            s.preferred_name = Some(normalize_port_name(&l.name).to_owned());
            s.preferred = Some(l.id);
            s.preferred_persist = true;
        }
        s
    }

    /// The preferred device as it would be persisted (None while it is only an automatic
    /// fallback pick, which is never persisted).
    pub fn persisted_preferred(&self, inputs: &[DeviceInfo]) -> Option<LastDevice> {
        if !self.preferred_persist {
            return None;
        }
        let id = self.preferred.clone()?;
        let name = inputs
            .iter()
            .find(|d| d.id == id)
            .map(|d| normalize_port_name(&d.name).to_owned())
            .or_else(|| self.preferred_name.clone())?;
        Some(LastDevice { id, name })
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

    /// Make `id` (present in `inputs`) the preferred device. `persist`: worth persisting
    /// (reported by [`Self::take_preferred_change`] when it changes).
    fn set_preferred(&mut self, id: &str, inputs: &[DeviceInfo], persist: bool) {
        let changed = self.preferred.as_deref() != Some(id) || persist != self.preferred_persist;
        self.preferred = Some(id.to_owned());
        if let Some(d) = inputs.iter().find(|d| d.id == id) {
            let name = normalize_port_name(&d.name);
            self.preferred_ordinal = inputs
                .iter()
                .take_while(|x| x.id != id)
                .filter(|x| normalize_port_name(&x.name) == name)
                .count();
            self.preferred_name = Some(name.to_owned());
        }
        self.preferred_persist = persist;
        if changed && persist {
            self.preferred_changed = true;
        }
    }

    /// The preferred device as present in `inputs`: by id, else by name (re-keyed to the
    /// current id; see the module docs).
    fn find_preferred<'a>(&mut self, inputs: &'a [DeviceInfo]) -> Option<&'a DeviceInfo> {
        let id = self.preferred.as_deref()?;
        if let Some(d) = inputs.iter().find(|d| d.id == id) {
            return Some(d);
        }
        let name = self.preferred_name.as_deref()?;
        let same: Vec<&DeviceInfo> = inputs
            .iter()
            .filter(|d| normalize_port_name(&d.name) == name)
            .collect();
        let d = *same.get(self.preferred_ordinal).or_else(|| same.first())?;
        let persist = self.preferred_persist;
        self.set_preferred(&d.id.clone(), inputs, persist);
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
            // the TS's "first automatic pick becomes preferred"; persisted only for an MPK
            let persist = is_mpk_mini(&fallback.name);
            self.set_preferred(&id, inputs, persist);
        }
        self.attach_to(&id, inputs, ops);
    }

    /// `selectDevice(id)`: ignored when absent; else preferred + attach.
    pub fn select(&mut self, id: &str, inputs: &[DeviceInfo]) -> Vec<DeviceOp> {
        let mut ops = Vec::new();
        if !inputs.iter().any(|d| d.id == id) {
            return ops;
        }
        self.set_preferred(id, inputs, true);
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
        self.preferred_ordinal = 0;
        self.preferred_persist = false;
        vec![DeviceOp::Detach]
    }
}

/// `heldNotes` (`"channel:note"` → note), in insertion order, and the channels whose sustain
/// pedal (CC 64) is down on the input (`sustainChannels`).
///
/// **Electron bug fixed in both apps (user decision 2026-11-18):** losing an input (switch,
/// unplug, dispose) released its held keys but left the pedal down, so the notes it sustained
/// rang on (and every later note was sustained) until the next pedal or panic. A detach now
/// also lifts that input's pedal: [`Self::take_sustain`] → CC 64 = 0 on each channel, after
/// the note-offs, which releases the sustained voices.
#[derive(Debug, Default, Clone)]
pub struct HeldNotes {
    notes: Vec<(u8, u8)>,
    sustain: Vec<u8>,
}

impl HeldNotes {
    pub fn note_on(&mut self, note: u8, channel: u8) {
        if !self.notes.contains(&(channel, note)) {
            self.notes.push((channel, note));
        }
    }
    pub fn note_off(&mut self, note: u8, channel: u8) {
        self.notes.retain(|k| *k != (channel, note));
    }
    /// Track the sustain pedal (CC 64, down at ≥ 64 like the router).
    pub fn control_change(&mut self, cc: u8, value: u8, channel: u8) {
        if cc != crate::router::CC_SUSTAIN {
            return;
        }
        if value >= 64 {
            if !self.sustain.contains(&channel) {
                self.sustain.push(channel);
            }
        } else {
            self.sustain.retain(|c| *c != channel);
        }
    }
    /// `releaseHeldNotes()`: every held (note, channel), cleared.
    pub fn take(&mut self) -> Vec<(u8, u8)> {
        std::mem::take(&mut self.notes)
            .into_iter()
            .map(|(c, n)| (n, c))
            .collect()
    }
    /// The channels whose pedal is down, cleared (send CC 64 = 0 on each after [`Self::take`]).
    pub fn take_sustain(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.sustain)
    }
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.sustain.is_empty()
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
        let inputs = [d("a", "IAC Bus"), d("c", "Other")];
        s.auto_select(&inputs);
        assert_eq!(s.active(), Some("a"));
        // preferred in memory (TS), but a fallback pick is never persisted
        assert_eq!(s.preferred(), Some("a"));
        assert!(!s.take_preferred_change());
        assert_eq!(s.persisted_preferred(&inputs), None);
        // an explicit choice of the same device is
        s.select("a", &inputs);
        assert!(s.take_preferred_change());
        assert_eq!(
            s.persisted_preferred(&inputs),
            Some(LastDevice {
                id: "a".into(),
                name: "IAC Bus".into()
            })
        );
    }

    /// Phase-5 review finding 7 (Opus): the IAC bus picked as a fallback at one start must not
    /// win over the MPK at the next one.
    #[test]
    fn a_fallback_pick_never_becomes_sticky_across_restarts() {
        let mut first = DeviceSelector::with_last_device(None);
        let only_iac = [d("iac", "IAC Driver Bus 1")];
        first.auto_select(&only_iac);
        assert!(!first.take_preferred_change());
        let persisted = first.persisted_preferred(&only_iac);
        assert_eq!(persisted, None);
        let mut next = DeviceSelector::with_last_device(persisted);
        next.auto_select(&[d("iac", "IAC Driver Bus 1"), d("mpk", "MPK mini Play mk3")]);
        assert_eq!(next.active(), Some("mpk"));
    }

    #[test]
    fn alsa_names_are_normalised() {
        assert_eq!(
            normalize_port_name("MPK mini Play mk3 20:0"),
            "MPK mini Play mk3"
        );
        assert_eq!(
            normalize_port_name("MPK mini Play mk3:MPK mini Play mk3 MIDI 1 24:0"),
            "MPK mini Play mk3:MPK mini Play mk3 MIDI 1"
        );
        assert_eq!(normalize_port_name("IAC Driver Bus 1"), "IAC Driver Bus 1");
        assert_eq!(normalize_port_name("Port 1:"), "Port 1:");
        assert_eq!(normalize_port_name("Synth 12:x"), "Synth 12:x");
        assert_eq!(normalize_port_name("128:0"), "128:0");
        // a remembered ALSA port comes back under a new client number and id
        let last = LastDevice {
            id: "20:0".into(),
            name: "Keystation 49 20:0".into(),
        };
        let mut s = DeviceSelector::with_last_device(Some(last));
        s.auto_select(&[
            d("14:0", "Midi Through 14:0"),
            d("24:0", "Keystation 49 24:0"),
        ]);
        assert_eq!(s.active(), Some("24:0"));
        assert!(s.take_preferred_change());
    }

    /// Phase-5 review finding 7 (Sol): name recovery used to work once (the name was dropped
    /// when the device was re-keyed); it now works for the whole session.
    #[test]
    fn name_recovery_works_repeatedly_and_with_duplicate_names() {
        let mut s = DeviceSelector::new();
        let a = [d("1", "IAC"), d("10", "Keystation")];
        s.select("10", &a);
        for (round, id) in ["11", "12", "13"].into_iter().enumerate() {
            s.device_change(&[d("1", "IAC")]);
            assert_eq!(s.active(), Some("1"), "round {round}");
            s.device_change(&[d("1", "IAC"), d(id, "Keystation")]);
            assert_eq!(s.active(), Some(id), "round {round}");
            assert_eq!(s.preferred(), Some(id));
        }
        // two ports of one name: the second one is chosen; after a replug (new ids) the second
        // one is taken again
        let mut s = DeviceSelector::new();
        let two = [d("a", "Twin"), d("b", "Twin"), d("c", "Other")];
        s.select("b", &two);
        s.device_change(&[d("c", "Other")]);
        s.device_change(&[d("x", "Twin"), d("y", "Twin"), d("c", "Other")]);
        assert_eq!(s.active(), Some("y"));
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
        h.control_change(64, 127, 1);
        h.control_change(64, 100, 3);
        h.control_change(64, 10, 3);
        h.control_change(64, 64, 2);
        h.control_change(7, 127, 4);
        assert_eq!(h.take(), vec![(60, 1), (62, 2)]);
        assert_eq!(h.take_sustain(), vec![1, 2]);
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
