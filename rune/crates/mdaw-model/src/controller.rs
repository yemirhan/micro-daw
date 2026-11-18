//! App-level hardware mapping: which incoming MIDI notes trigger which pad and which CCs drive
//! which knob target. Ported from `src/services/ControllerMapping.ts`.
//!
//! Persisted (TS: localStorage `micro-daw:controller`) as `{ version, profile }` through the
//! [`ProfileStore`] trait; anything unreadable falls back to the default profile. Also owns the
//! "learn" state (pad wizard / single pad / knob): while learning, every note-on / CC is handed
//! here first and the learner consumes it.
//!
//! Rust shape of the TS singleton class:
//! - `subscribe` / `onActivity` listeners → [`ControllerMapping::version`] and
//!   [`ControllerMapping::activity_version`] counters (bumped where TS notifies).
//! - `onKnob` handlers → [`ControllerMapping::handle_cc`] returns the knob dispatches
//!   ([`CcOutcome::knobs`], in the order the TS code would call a single handler).
//! - `performance.now()` → an injected clock (`at` timestamps of [`MidiActivity`]).

use serde::Serialize;
use serde_json::Value;

use crate::types::{
    CCMapping, ControllerProfile, KnobTarget, PAD_COUNT, PadIndex, PadMidiMap, PadParam,
};

pub const CONTROLLER_STORAGE_KEY: &str = "micro-daw:controller";
pub const CONTROLLER_STORAGE_VERSION: i64 = 1;
pub const DEFAULT_PROFILE_ID: &str = "mpk-mini-default";

// --- Knob targets ------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnobTargetOption {
    pub key: &'static str,
    pub label: &'static str,
    pub target: KnobTarget,
}

const fn pad_opt(key: &'static str, label: &'static str, param: PadParam) -> KnobTargetOption {
    KnobTargetOption {
        key,
        label,
        target: KnobTarget::Pad { param },
    }
}

/// Every knob target, in menu order.
pub const KNOB_TARGET_OPTIONS: [KnobTargetOption; 10] = [
    pad_opt("pad:pitch", "Pad pitch", PadParam::Pitch),
    pad_opt("pad:decay", "Pad decay", PadParam::Decay),
    pad_opt("pad:filterCutoff", "Pad filter", PadParam::FilterCutoff),
    pad_opt("pad:gainDb", "Pad gain", PadParam::GainDb),
    pad_opt("pad:pan", "Pad pan", PadParam::Pan),
    pad_opt("pad:startSec", "Pad start", PadParam::StartSec),
    KnobTargetOption {
        key: "swing",
        label: "Swing",
        target: KnobTarget::Swing,
    },
    KnobTargetOption {
        key: "bpm",
        label: "Tempo (BPM)",
        target: KnobTarget::Bpm,
    },
    KnobTargetOption {
        key: "trackVolume",
        label: "Drum track volume",
        target: KnobTarget::TrackVolume,
    },
    KnobTargetOption {
        key: "masterVolume",
        label: "Master volume",
        target: KnobTarget::MasterVolume,
    },
];

/// Stable string id for a knob target.
pub fn knob_target_key(target: &KnobTarget) -> String {
    match target {
        KnobTarget::Pad { param } => format!("pad:{}", param.id()),
        KnobTarget::Swing => "swing".into(),
        KnobTarget::Bpm => "bpm".into(),
        KnobTarget::TrackVolume => "trackVolume".into(),
        KnobTarget::MasterVolume => "masterVolume".into(),
    }
}

pub fn knob_target_from_key(key: &str) -> Option<KnobTarget> {
    KNOB_TARGET_OPTIONS
        .iter()
        .find(|o| o.key == key)
        .map(|o| o.target)
}

pub fn knob_target_label(target: &KnobTarget) -> String {
    let key = knob_target_key(target);
    KNOB_TARGET_OPTIONS
        .iter()
        .find(|o| o.key == key)
        .map(|o| o.label.to_string())
        .unwrap_or(key)
}

pub fn same_knob_target(a: &KnobTarget, b: &KnobTarget) -> bool {
    knob_target_key(a) == knob_target_key(b)
}

// --- Default profile ----------------------------------------------------------------------------

const DEFAULT_KNOB_TARGETS: [KnobTarget; 8] = [
    KnobTarget::Pad {
        param: PadParam::Pitch,
    },
    KnobTarget::Pad {
        param: PadParam::Decay,
    },
    KnobTarget::Pad {
        param: PadParam::FilterCutoff,
    },
    KnobTarget::Pad {
        param: PadParam::GainDb,
    },
    KnobTarget::Swing,
    KnobTarget::Bpm,
    KnobTarget::TrackVolume,
    KnobTarget::MasterVolume,
];

/// "MPK Mini (default)": pads send notes 36..51 on any channel; knobs CC 70..77.
pub fn create_default_profile() -> ControllerProfile {
    ControllerProfile {
        id: DEFAULT_PROFILE_ID.into(),
        name: "MPK Mini (default)".into(),
        pad_map: PadMidiMap {
            notes: (0..PAD_COUNT as i64).map(|pad| Some(36 + pad)).collect(),
            channel: None,
        },
        cc_mappings: DEFAULT_KNOB_TARGETS
            .iter()
            .enumerate()
            .map(|(i, target)| CCMapping {
                cc: 70 + i as i64,
                channel: None,
                target: *target,
            })
            .collect(),
    }
}

pub fn clone_profile(p: &ControllerProfile) -> ControllerProfile {
    p.clone()
}

// --- Validation / persistence (pure) -----------------------------------------------------------

/// JS `typeof v === 'number' && Number.isInteger(v) && v >= min && v <= max`.
fn as_int(v: Option<&Value>, min: i64, max: i64) -> Option<i64> {
    let f = v?.as_f64()?;
    if f.is_finite() && f.fract() == 0.0 && f >= min as f64 && f <= max as f64 {
        Some(f as i64)
    } else {
        None
    }
}

/// `isChannel`: null, or an integer 1..16. Outer None = invalid. (A missing key is `undefined`
/// in JS, which is not null → invalid.)
fn as_channel(v: Option<&Value>) -> Option<Option<i64>> {
    match v {
        Some(Value::Null) => Some(None),
        other => as_int(other, 1, 16).map(Some),
    }
}

/// JS truthiness + `typeof === 'object'` (arrays included; they simply have none of the keys).
fn as_object_like(v: Option<&Value>) -> Option<Option<&serde_json::Map<String, Value>>> {
    match v {
        Some(Value::Object(m)) => Some(Some(m)),
        Some(Value::Array(_)) => Some(None),
        _ => None,
    }
}

fn get<'a>(m: Option<&'a serde_json::Map<String, Value>>, key: &str) -> Option<&'a Value> {
    m.and_then(|m| m.get(key))
}

fn parse_knob_target(v: Option<&Value>) -> Option<KnobTarget> {
    let t = as_object_like(v)?;
    let kind = get(t, "kind");
    if kind.and_then(Value::as_str) == Some("pad") {
        return match get(t, "param") {
            Some(Value::String(param)) => knob_target_from_key(&format!("pad:{param}")),
            _ => None,
        };
    }
    match kind {
        // Note: kind "pad:pitch" resolves through the option keys too (TS quirk, kept).
        Some(Value::String(k)) if k != "pad" => knob_target_from_key(k),
        _ => None,
    }
}

/// Validate an unknown value as a ControllerProfile. Returns a clean copy or None. The pad map
/// must be fully valid; individual malformed CC mappings are dropped.
pub fn validate_profile(value: &Value) -> Option<ControllerProfile> {
    let p = as_object_like(Some(value))?;
    let id = get(p, "id")?.as_str()?;
    let name = get(p, "name")?.as_str()?;
    let pad_map = as_object_like(get(p, "padMap"))?;
    let notes = get(pad_map, "notes")?.as_array()?;
    if notes.len() != PAD_COUNT {
        return None;
    }
    let mut clean_notes = Vec::with_capacity(PAD_COUNT);
    for n in notes {
        if n.is_null() {
            clean_notes.push(None);
        } else {
            clean_notes.push(Some(as_int(Some(n), 0, 127)?));
        }
    }
    let channel = as_channel(get(pad_map, "channel"))?;
    let raw_mappings = get(p, "ccMappings")?.as_array()?;

    let mut cc_mappings = Vec::new();
    for raw in raw_mappings {
        let Some(m) = as_object_like(Some(raw)) else {
            continue;
        };
        let target = parse_knob_target(get(m, "target"));
        let cc = as_int(get(m, "cc"), 0, 127);
        let ch = as_channel(get(m, "channel"));
        let (Some(cc), Some(channel), Some(target)) = (cc, ch, target) else {
            continue;
        };
        cc_mappings.push(CCMapping {
            cc,
            channel,
            target,
        });
    }
    Some(ControllerProfile {
        id: id.to_string(),
        name: name.to_string(),
        pad_map: PadMidiMap {
            notes: clean_notes,
            channel,
        },
        cc_mappings,
    })
}

/// [`validate_profile`] for an already-typed profile (what `setProfile(p)` does in TS).
pub fn validate_typed_profile(profile: &ControllerProfile) -> Option<ControllerProfile> {
    serde_json::to_value(profile)
        .ok()
        .and_then(|v| validate_profile(&v))
}

#[derive(Serialize)]
struct Stored<'a> {
    version: i64,
    profile: &'a ControllerProfile,
}

/// `JSON.stringify({ version: 1, profile })` — byte-identical for a validated profile (keys in
/// the order validateProfile/createDefaultProfile produce them).
pub fn serialize_profile(profile: &ControllerProfile) -> String {
    serde_json::to_string(&Stored {
        version: CONTROLLER_STORAGE_VERSION,
        profile,
    })
    .expect("controller profile serializes")
}

/// Parse stored JSON; anything missing, corrupt or from an unknown version → default profile.
pub fn parse_stored_profile(raw: Option<&str>) -> ControllerProfile {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else {
        return create_default_profile();
    };
    let Ok(data) = serde_json::from_str::<Value>(raw) else {
        return create_default_profile();
    };
    // `!data || data.version !== 1` (primitives / arrays have no `version`).
    let version_ok =
        matches!(&data, Value::Object(m) if m.get("version").and_then(Value::as_f64) == Some(1.0));
    if !version_ok {
        return create_default_profile();
    }
    data.get("profile")
        .and_then(validate_profile)
        .unwrap_or_else(create_default_profile)
}

// --- Pure lookups / edits -----------------------------------------------------------------------

/// Pad for an incoming note, honouring the pad map's channel filter.
pub fn find_pad_for_note(profile: &ControllerProfile, note: i64, channel: i64) -> Option<PadIndex> {
    let PadMidiMap {
        notes,
        channel: pad_channel,
    } = &profile.pad_map;
    if pad_channel.is_some_and(|c| c != channel) {
        return None;
    }
    notes.iter().position(|n| *n == Some(note))
}

/// CC mappings that match an incoming CC.
pub fn find_cc_mappings(profile: &ControllerProfile, cc: i64, channel: i64) -> Vec<CCMapping> {
    profile
        .cc_mappings
        .iter()
        .filter(|m| m.cc == cc && m.channel.is_none_or(|c| c == channel))
        .cloned()
        .collect()
}

/// Assign `note` to `pad`. A note already on another pad moves (that pad becomes unassigned).
pub fn assign_pad_note(profile: &ControllerProfile, pad: PadIndex, note: i64) -> ControllerProfile {
    let mut next = clone_profile(profile);
    for (i, n) in next.pad_map.notes.iter_mut().enumerate() {
        if i != pad && *n == Some(note) {
            *n = None;
        }
    }
    // JS writes past the end with holes (serialized as null).
    if pad >= next.pad_map.notes.len() {
        next.pad_map.notes.resize(pad + 1, None);
    }
    next.pad_map.notes[pad] = Some(note);
    next
}

/// Bind `cc` to `target`. With `index` that mapping row is rebound (an out-of-range index adds a
/// row); without it the first mapping with that target is rebound, or a new one added. Any other
/// mapping on the same CC is removed (a knob drives one target).
pub fn assign_knob_cc(
    profile: &ControllerProfile,
    target: &KnobTarget,
    cc: i64,
    index: Option<i64>,
) -> ControllerProfile {
    let mut next = clone_profile(profile);
    let len = next.cc_mappings.len();
    let at = match index {
        None => next
            .cc_mappings
            .iter()
            .position(|m| same_knob_target(&m.target, target)),
        Some(i) if i >= 0 && (i as usize) < len => Some(i as usize),
        Some(_) => None, // out of range = add a row
    };
    let binding = CCMapping {
        cc,
        channel: None,
        target: *target,
    };
    let at = match at {
        None => {
            next.cc_mappings.push(binding);
            next.cc_mappings.len() - 1
        }
        Some(at) => {
            next.cc_mappings[at] = binding;
            at
        }
    };
    let mut i = 0;
    next.cc_mappings.retain(|m| {
        let keep = i == at || m.cc != cc;
        i += 1;
        keep
    });
    next
}

// --- Learn state --------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LearnState {
    Idle,
    /// `sequence`: the "hit pads 1..16 in order" wizard; `pad` is the pad waiting for a hit.
    Pad {
        pad: PadIndex,
        sequence: bool,
    },
    /// `index`: the mapping row being re-learned, if any.
    Knob {
        target: KnobTarget,
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<i64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MidiActivity {
    Note {
        note: i64,
        velocity: f64,
        channel: i64,
        pad: Option<PadIndex>,
        at: f64,
    },
    Cc {
        cc: i64,
        value: f64,
        channel: i64,
        targets: Vec<KnobTarget>,
        at: f64,
    },
}

/// Argument of `startPadLearn`: one pad, or the `'sequence'` wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PadLearn {
    Pad(i64),
    Sequence,
}

/// `Partial<CCMapping>` for `setCCMapping`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CCMappingPatch {
    pub cc: Option<i64>,
    pub channel: Option<Option<i64>>,
    pub target: Option<KnobTarget>,
}

/// One knob-handler call: `handler(target, value01)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KnobEvent {
    pub target: KnobTarget,
    pub value01: f64,
}

/// Result of [`ControllerMapping::handle_cc`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CcOutcome {
    /// The CC was consumed (learned or mapped).
    pub consumed: bool,
    /// Knob dispatches to deliver to every `onKnob` handler, in order.
    pub knobs: Vec<KnobEvent>,
}

/// Persistence backend (TS: localStorage key [`CONTROLLER_STORAGE_KEY`]).
pub trait ProfileStore {
    fn load(&self) -> Option<String>;
    fn save(&mut self, s: &str);
}

/// In-memory store (tests, or no persistence).
#[derive(Debug, Clone, Default)]
pub struct MemoryProfileStore {
    pub value: Option<String>,
}

impl ProfileStore for MemoryProfileStore {
    fn load(&self) -> Option<String> {
        self.value.clone()
    }
    fn save(&mut self, s: &str) {
        self.value = Some(s.to_string());
    }
}

pub struct ControllerMapping<St: ProfileStore> {
    store: St,
    clock: Box<dyn Fn() -> f64 + Send>,
    profile: ControllerProfile,
    learn: LearnState,
    /// Channels heard during the current sequence wizard (to narrow padMap.channel afterwards).
    sequence_channels: Vec<i64>,
    last_activity: Option<MidiActivity>,
    version: u64,
    activity_version: u64,
}

impl<St: ProfileStore> ControllerMapping<St> {
    /// Loads the stored profile. `clock` gives activity timestamps (ms, like `performance.now()`).
    pub fn new(store: St, clock: Box<dyn Fn() -> f64 + Send>) -> Self {
        let profile = parse_stored_profile(store.load().as_deref());
        Self {
            store,
            clock,
            profile,
            learn: LearnState::Idle,
            sequence_channels: Vec::new(),
            last_activity: None,
            version: 0,
            activity_version: 0,
        }
    }

    /// Profile / learn-state change counter (TS `subscribe` listeners).
    pub fn version(&self) -> u64 {
        self.version
    }
    /// Activity counter (TS `onActivity` listeners).
    pub fn activity_version(&self) -> u64 {
        self.activity_version
    }
    pub fn store(&self) -> &St {
        &self.store
    }

    // --- Profile ----------------------------------------------------------------------------

    pub fn get_profile(&self) -> &ControllerProfile {
        &self.profile
    }

    /// Invalid profiles are ignored (TS logs a warning).
    pub fn set_profile(&mut self, p: &ControllerProfile) {
        let Some(valid) = validate_typed_profile(p) else {
            return;
        };
        self.store.save(&serialize_profile(&valid));
        self.profile = valid;
        self.emit();
    }

    pub fn reset_to_default(&mut self) {
        self.cancel_learn();
        self.set_profile(&create_default_profile());
    }

    /// Re-read the stored profile.
    pub fn reload(&mut self) {
        self.profile = parse_stored_profile(self.store.load().as_deref());
        self.emit();
    }

    pub fn pad_for_note(&self, note: i64, channel: i64) -> Option<PadIndex> {
        find_pad_for_note(&self.profile, note, channel)
    }

    pub fn clear_pad(&mut self, pad: i64) {
        if pad < 0 || pad >= PAD_COUNT as i64 {
            return;
        }
        let mut next = clone_profile(&self.profile);
        if let Some(n) = next.pad_map.notes.get_mut(pad as usize) {
            *n = None;
        }
        self.set_profile(&next);
    }

    pub fn set_pad_channel(&mut self, channel: Option<i64>) {
        let mut next = clone_profile(&self.profile);
        next.pad_map.channel = channel;
        self.set_profile(&next);
    }

    pub fn set_cc_mapping(&mut self, index: i64, patch: CCMappingPatch) {
        if index < 0 || index >= self.profile.cc_mappings.len() as i64 {
            return;
        }
        let mut next = clone_profile(&self.profile);
        let m = &mut next.cc_mappings[index as usize];
        if let Some(cc) = patch.cc {
            m.cc = cc;
        }
        if let Some(channel) = patch.channel {
            m.channel = channel;
        }
        if let Some(target) = patch.target {
            m.target = target;
        }
        self.set_profile(&next);
    }

    /// `ccMappings.splice(index, 1)` (a negative index counts from the end, as in JS).
    pub fn remove_cc_mapping(&mut self, index: i64) {
        let mut next = clone_profile(&self.profile);
        let len = next.cc_mappings.len() as i64;
        let start = if index < 0 {
            (len + index).max(0)
        } else {
            index.min(len)
        };
        if start < len {
            next.cc_mappings.remove(start as usize);
        }
        self.set_profile(&next);
    }

    // --- Learn -------------------------------------------------------------------------------

    pub fn get_learn_state(&self) -> LearnState {
        self.learn
    }

    pub fn is_learning(&self) -> bool {
        self.learn != LearnState::Idle
    }

    /// Learn one pad, or [`PadLearn::Sequence`] = wizard starting at pad 1 (index 0).
    pub fn start_pad_learn(&mut self, pad: PadLearn) {
        match pad {
            PadLearn::Sequence => {
                self.sequence_channels.clear();
                self.set_learn(LearnState::Pad {
                    pad: 0,
                    sequence: true,
                });
            }
            PadLearn::Pad(p) => {
                if p < 0 || p >= PAD_COUNT as i64 {
                    return;
                }
                self.set_learn(LearnState::Pad {
                    pad: p as usize,
                    sequence: false,
                });
            }
        }
    }

    /// Bind the next CC received to `target`. `index`: rebind that mapping row (pass
    /// `cc_mappings.len()` to add a new row); None = the target's first row.
    pub fn start_knob_learn(&mut self, target: KnobTarget, index: Option<i64>) {
        self.set_learn(LearnState::Knob { target, index });
    }

    pub fn cancel_learn(&mut self) {
        if self.learn == LearnState::Idle {
            return;
        }
        self.set_learn(LearnState::Idle);
    }

    /// Wizard: leave the current pad as it is and go to the next one.
    pub fn skip_pad(&mut self) {
        if !matches!(self.learn, LearnState::Pad { .. }) {
            return;
        }
        self.advance_pad_learn();
    }

    /// Wizard (Backspace): go back one pad.
    pub fn previous_pad(&mut self) {
        if let LearnState::Pad {
            pad,
            sequence: true,
        } = self.learn
        {
            self.set_learn(LearnState::Pad {
                pad: pad.saturating_sub(1),
                sequence: true,
            });
        }
    }

    fn advance_pad_learn(&mut self) {
        let LearnState::Pad { pad, sequence } = self.learn else {
            return;
        };
        if !sequence || pad >= PAD_COUNT - 1 {
            self.finish_pad_learn(sequence);
            return;
        }
        self.set_learn(LearnState::Pad {
            pad: pad + 1,
            sequence,
        });
    }

    fn finish_pad_learn(&mut self, sequence: bool) {
        // A completed wizard whose hits all came from one channel narrows the pad map to it.
        if sequence && self.sequence_channels.len() == 1 {
            let channel = self.sequence_channels[0];
            if self.profile.pad_map.channel != Some(channel) {
                self.set_pad_channel(Some(channel));
            }
        }
        self.sequence_channels.clear();
        self.set_learn(LearnState::Idle);
    }

    fn set_learn(&mut self, state: LearnState) {
        self.learn = state;
        self.emit();
    }

    // --- Input -------------------------------------------------------------------------------

    /// Every MIDI note-on goes through here first. Records activity; returns true when the note
    /// was consumed by a pad learner (do not play it).
    pub fn handle_note_on(&mut self, note: i64, velocity: f64, channel: i64) -> bool {
        let learn_pad = match self.learn {
            LearnState::Pad { pad, sequence } => Some((pad, sequence)),
            _ => None,
        };
        if let Some((pad, sequence)) = learn_pad {
            let mut next = assign_pad_note(&self.profile, pad, note);
            // A hit from a channel the pad map ignores widens it to "any".
            if next.pad_map.channel.is_some_and(|c| c != channel) {
                next.pad_map.channel = None;
            }
            if sequence && !self.sequence_channels.contains(&channel) {
                self.sequence_channels.push(channel);
            }
            self.set_profile(&next);
        }
        let pad = match learn_pad {
            Some((pad, _)) => Some(pad),
            None => self.pad_for_note(note, channel),
        };
        let at = (self.clock)();
        self.record_activity(MidiActivity::Note {
            note,
            velocity,
            channel,
            pad,
            at,
        });
        if learn_pad.is_some() {
            self.advance_pad_learn();
        }
        learn_pad.is_some()
    }

    /// Every CC that isn't sustain/panic goes through here. While knob-learning the CC is bound;
    /// otherwise the matching mappings are returned as knob dispatches.
    pub fn handle_cc(&mut self, cc: i64, raw_value: f64, channel: i64) -> CcOutcome {
        if let LearnState::Knob { target, index } = self.learn {
            let next = assign_knob_cc(&self.profile, &target, cc, index);
            self.set_profile(&next);
            let at = (self.clock)();
            self.record_activity(MidiActivity::Cc {
                cc,
                value: raw_value,
                channel,
                targets: vec![target],
                at,
            });
            self.set_learn(LearnState::Idle);
            return CcOutcome {
                consumed: true,
                knobs: Vec::new(),
            };
        }
        let mappings = find_cc_mappings(&self.profile, cc, channel);
        let at = (self.clock)();
        self.record_activity(MidiActivity::Cc {
            cc,
            value: raw_value,
            channel,
            targets: mappings.iter().map(|m| m.target).collect(),
            at,
        });
        if mappings.is_empty() {
            return CcOutcome::default();
        }
        // Math.min(1, Math.max(0, raw / 127)) — NaN stays NaN.
        let v = raw_value / 127.0;
        let value01 = if v.is_nan() {
            f64::NAN
        } else {
            v.clamp(0.0, 1.0)
        };
        CcOutcome {
            consumed: true,
            knobs: mappings
                .iter()
                .map(|m| KnobEvent {
                    target: m.target,
                    value01,
                })
                .collect(),
        }
    }

    pub fn get_last_activity(&self) -> Option<&MidiActivity> {
        self.last_activity.as_ref()
    }

    fn record_activity(&mut self, activity: MidiActivity) {
        self.last_activity = Some(activity);
        self.activity_version += 1;
    }

    fn emit(&mut self) {
        self.version += 1;
    }
}

#[cfg(test)]
#[path = "controller_tests.rs"]
mod tests;
