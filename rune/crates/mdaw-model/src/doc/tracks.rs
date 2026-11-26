//! Track CRUD, mixer settings, effects, groups and arming (ArrangementEngine.ts AE:430-643,
//! 836-840, 2094-2184).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::kit::{create_factory_kit, normalize_kit};
use crate::types::{InstrumentType, Track, TrackEffectState, TrackGroup, TrackInstrument};

use super::{ArrangementDoc, DocEvent, TRACK_COLORS, synth_preset_name};

/// `keyof TrackEffectState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EffectName {
    Reverb,
    Delay,
    Chorus,
    Distortion,
    Eq,
    Compressor,
    Filter,
}

impl EffectName {
    pub fn key(self) -> &'static str {
        match self {
            Self::Reverb => "reverb",
            Self::Delay => "delay",
            Self::Chorus => "chorus",
            Self::Distortion => "distortion",
            Self::Eq => "eq",
            Self::Compressor => "compressor",
            Self::Filter => "filter",
        }
    }
}

impl EffectName {
    /// The order the built-in effects run in unless the track says otherwise (the Electron
    /// app's fixed strip).
    pub const DEFAULT_ORDER: [EffectName; 7] = [
        Self::Filter,
        Self::Distortion,
        Self::Eq,
        Self::Compressor,
        Self::Chorus,
        Self::Delay,
        Self::Reverb,
    ];

    pub fn from_key(key: &str) -> Option<Self> {
        Self::DEFAULT_ORDER.into_iter().find(|e| e.key() == key)
    }
}

/// One stage of a track's effect chain: a built-in effect (one of each) or a hosted plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChainItem {
    Effect(EffectName),
    /// A plugin of [`Track::plugins`], by its instance id.
    Plugin(String),
}

impl ChainItem {
    /// Its entry in [`Track::fx_order`].
    pub fn key(&self) -> String {
        match self {
            Self::Effect(e) => e.key().to_owned(),
            Self::Plugin(id) => format!("plugin:{id}"),
        }
    }

    pub fn parse(key: &str) -> Option<Self> {
        match key.strip_prefix("plugin:") {
            Some(id) => Some(Self::Plugin(id.to_owned())),
            None => EffectName::from_key(key).map(Self::Effect),
        }
    }
}

/// A track's whole effect chain, first to last: every built-in effect (on or off) and every
/// plugin exactly once. No saved order: the plugins, then [`EffectName::DEFAULT_ORDER`] (what
/// every project sounded like before chains could be reordered). A saved order is followed as
/// far as it goes (unknown entries and plugins that are gone are skipped); whatever it does not
/// name comes after it, plugins first.
pub fn track_chain(track: &Track) -> Vec<ChainItem> {
    let mut chain: Vec<ChainItem> = Vec::new();
    for item in track.fx_order.iter().filter_map(|k| ChainItem::parse(k)) {
        let known = match &item {
            ChainItem::Plugin(id) => track.plugins.iter().any(|p| &p.id == id),
            ChainItem::Effect(_) => true,
        };
        if known && !chain.contains(&item) {
            chain.push(item);
        }
    }
    for p in &track.plugins {
        let item = ChainItem::Plugin(p.id.clone());
        if !chain.contains(&item) {
            chain.push(item);
        }
    }
    for e in EffectName::DEFAULT_ORDER {
        if !chain.contains(&ChainItem::Effect(e)) {
            chain.push(ChainItem::Effect(e));
        }
    }
    chain
}

impl ArrangementDoc {
    /// Set the order of a track's effect chain — undo `label` ("Reorder Effects"). Items the
    /// list leaves out keep running after it ([`track_chain`]).
    pub fn set_track_chain(&mut self, track_id: &str, chain: &[ChainItem], label: &str) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        let order: Vec<String> = chain.iter().map(ChainItem::key).collect();
        if self.arrangement.tracks[i].fx_order == order {
            return;
        }
        self.push_undo_snapshot(label);
        self.arrangement.tracks[i].fx_order = order;
        self.emit(DocEvent::TrackChainChanged(track_id.to_owned()));
        self.emit_state_change();
    }

    /// "Add effect": the built-in effect is turned on and goes to the end of the chain, in one
    /// step — undo 'Add Effect'.
    pub fn add_track_effect(&mut self, track_id: &str, effect: EffectName) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        self.push_undo_snapshot("Add Effect");
        let track = &mut self.arrangement.tracks[i];
        let mut chain = track_chain(track);
        chain.retain(|c| *c != ChainItem::Effect(effect));
        chain.push(ChainItem::Effect(effect));
        track.fx_order = chain.iter().map(ChainItem::key).collect();
        let effects = track.effects.get_or_insert_with(TrackEffectState::default);
        match effect {
            EffectName::Reverb => effects.reverb.enabled = true,
            EffectName::Delay => effects.delay.enabled = true,
            EffectName::Chorus => effects.chorus.enabled = true,
            EffectName::Distortion => effects.distortion.enabled = true,
            EffectName::Eq => effects.eq.enabled = true,
            EffectName::Compressor => effects.compressor.enabled = true,
            EffectName::Filter => effects.filter.enabled = true,
        }
        self.emit(DocEvent::TrackChainChanged(track_id.to_owned()));
        self.emit(DocEvent::TrackEffectChanged {
            track_id: track_id.to_owned(),
            effect,
        });
        self.emit_state_change();
    }

    /// `addTrack(type = 'synth', presetIndex = 0)` — undo 'Add Track'. Volume −6 dB, cycling
    /// colour, drums get the factory kit (and become active when none is), unique "Drums" names.
    pub fn add_track(&mut self, kind: InstrumentType, preset_index: i64) -> Track {
        self.push_undo_snapshot("Add Track");
        let color = TRACK_COLORS[self.color_index % TRACK_COLORS.len()].to_owned();
        self.color_index += 1;
        let name = match kind {
            InstrumentType::Drums => self.unique_track_name("Drums", None),
            // "Audio", "Audio 2"…: several audio tracks are recorded side by side (the TS
            // named them all "Audio"; an imported file still renames its track).
            InstrumentType::Audio => self.unique_track_name("Audio", None),
            InstrumentType::Synth => synth_preset_name(preset_index).to_owned(),
        };
        let instrument = TrackInstrument {
            kind,
            preset_index: if kind == InstrumentType::Audio {
                0
            } else {
                preset_index
            },
            synth_overrides: None,
            kit: (kind == InstrumentType::Drums).then(create_factory_kit),
            extra: Default::default(),
            plugin: None,
        };
        let track = Track {
            id: self.next_id(),
            name,
            instrument,
            regions: Vec::new(),
            volume: -6.0,
            pan: 0.0,
            muted: false,
            solo: false,
            color,
            effects: Some(TrackEffectState::default()),
            automation: None,
            extra: Map::new(),
            input: None,
            plugins: Vec::new(),
            fx_order: Vec::new(),
        };
        let id = track.id.clone();
        self.arrangement.tracks.push(track);
        let index = self.arrangement.tracks.len() - 1;
        self.ensure_track_defaults(index);
        // AE:455 `this.createTrackAudio(track)`.
        self.emit(DocEvent::TrackAdded(id.clone()));
        if kind == InstrumentType::Drums && self.resolve_active_drum_track_id().is_none() {
            self.beat().active_drum_track_id = Some(id);
        }
        self.sync_drum_sink();
        self.emit_state_change();
        if kind == InstrumentType::Drums {
            self.emit_kit_change();
        }
        self.arrangement.tracks[index].clone()
    }

    /// `uniqueTrackName(base, exceptId)`: `base`, or "base 2", "base 3"...
    pub(crate) fn unique_track_name(&self, base: &str, except_id: Option<&str>) -> String {
        let taken: Vec<&str> = self
            .arrangement
            .tracks
            .iter()
            .filter(|t| Some(t.id.as_str()) != except_id)
            .map(|t| t.name.as_str())
            .collect();
        if !taken.contains(&base) {
            return base.to_owned();
        }
        let mut n = 2;
        while taken.contains(&format!("{base} {n}").as_str()) {
            n += 1;
        }
        format!("{base} {n}")
    }

    /// `removeTrack(id)` — undo 'Remove Track'. Ends a take on that track first; clears the arm;
    /// removes the track from groups and drops empty groups.
    pub fn remove_track(&mut self, track_id: &str) {
        if self
            .loop_take
            .as_ref()
            .is_some_and(|t| t.track_id == track_id)
        {
            self.end_loop_take(true);
        }
        self.push_undo_snapshot("Remove Track");
        // AE:471 `this.disposeTrackAudio(trackId)`.
        self.emit(DocEvent::TrackRemoved(track_id.to_owned()));
        self.arrangement.tracks.retain(|t| t.id != track_id);
        if self.armed_track_ids.iter().any(|id| id == track_id) {
            self.armed_track_ids.retain(|id| id != track_id);
            self.emit(DocEvent::ArmChanged);
        }
        if let Some(groups) = &mut self.arrangement.groups {
            for group in groups.iter_mut() {
                group.track_ids.retain(|id| id != track_id);
            }
            groups.retain(|g| !g.track_ids.is_empty());
        }
        self.sync_drum_sink();
        self.emit_state_change();
        self.emit_kit_change();
    }

    /// `setTrackInstrument(id, instrument)` — undo 'Change Instrument'. Drums keep the track's kit
    /// unless one is given (normalized); a switch to drums renames to a unique "Drums", any other
    /// type renames to the preset name.
    pub fn set_track_instrument(&mut self, track_id: &str, instrument: &TrackInstrument) {
        self.push_undo_snapshot("Change Instrument");
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        let mut next = instrument.clone();
        if next.kind == InstrumentType::Drums {
            let kit = instrument
                .kit
                .clone()
                .or_else(|| self.arrangement.tracks[i].instrument.kit.clone())
                .unwrap_or_else(create_factory_kit);
            let raw = serde_json::to_value(&kit).unwrap_or(Value::Null);
            next.kit = Some(normalize_kit(
                &raw,
                self.packs_dir.as_deref(),
                &mut *self.new_id,
            ));
        } else {
            next.kit = None;
        }
        let was_drums = self.arrangement.tracks[i].instrument.kind == InstrumentType::Drums;
        self.arrangement.tracks[i].instrument = next;
        if instrument.kind == InstrumentType::Drums {
            if !was_drums {
                self.arrangement.tracks[i].name = self.unique_track_name("Drums", Some(track_id));
            }
        } else if let Some(p) = instrument.plugin.as_ref().filter(|p| !p.name.is_empty()) {
            self.arrangement.tracks[i].name = p.name.clone();
        } else {
            self.arrangement.tracks[i].name = synth_preset_name(instrument.preset_index).to_owned();
        }
        self.with_playback_restart_prelude();
        if let Some(i) = self.track_index(track_id) {
            self.ensure_track_defaults(i);
        }
        self.emit(DocEvent::TrackAudioRebuilt(track_id.to_owned()));
        self.sync_drum_sink();
        self.emit_state_change();
        self.emit_kit_change();
    }

    /// `setTrackSynthOverrides(id, overrides)` — undo 'Edit Synth Params'. Shallow merge, synth
    /// tracks only; rebuilds the track's nodes.
    pub fn set_track_synth_overrides(&mut self, track_id: &str, overrides: &Map<String, Value>) {
        self.push_undo_snapshot("Edit Synth Params");
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        if self.arrangement.tracks[i].instrument.kind != InstrumentType::Synth {
            return;
        }
        let merged = self.arrangement.tracks[i]
            .instrument
            .synth_overrides
            .get_or_insert_with(Map::new);
        for (k, v) in overrides {
            merged.insert(k.clone(), v.clone());
        }
        self.with_playback_restart_prelude();
        if let Some(i) = self.track_index(track_id) {
            self.ensure_track_defaults(i);
        }
        self.emit(DocEvent::TrackAudioRebuilt(track_id.to_owned()));
        self.emit_state_change();
    }

    /// `setTrackVolume` (no undo).
    pub fn set_track_volume(&mut self, track_id: &str, db: f64) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[i].volume = db;
        self.emit(DocEvent::TrackVolumeChanged {
            track_id: track_id.to_owned(),
            db,
        });
        self.emit_state_change();
    }

    /// `setTrackMute` (no undo).
    pub fn set_track_mute(&mut self, track_id: &str, muted: bool) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[i].muted = muted;
        self.emit(DocEvent::TrackMuteChanged {
            track_id: track_id.to_owned(),
            muted,
        });
        self.emit_state_change();
    }

    /// `setTrackSolo` (no undo).
    pub fn set_track_solo(&mut self, track_id: &str, solo: bool) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[i].solo = solo;
        self.emit(DocEvent::SoloChanged);
        self.emit_state_change();
    }

    /// `setTrackPan` (no undo).
    pub fn set_track_pan(&mut self, track_id: &str, pan: f64) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        self.arrangement.tracks[i].pan = pan;
        self.emit(DocEvent::TrackPanChanged {
            track_id: track_id.to_owned(),
            pan,
        });
        self.emit_state_change();
    }

    /// Whether a track should sound under the solo rule (any solo → only soloed tracks, else not
    /// muted), the per-track result of `updateSoloState()`.
    pub fn track_audible(&self, track_id: &str) -> bool {
        let has_solo = self.arrangement.tracks.iter().any(|t| t.solo);
        self.find_track(track_id)
            .is_some_and(|t| if has_solo { t.solo } else { !t.muted })
    }

    /// `setTrackEffect(id, name, params)` — undo 'Change Track Effect'. `Object.assign` of the
    /// params into the effect (ill-typed params are ignored).
    pub fn set_track_effect(
        &mut self,
        track_id: &str,
        effect: EffectName,
        params: &Map<String, Value>,
    ) {
        self.push_undo_snapshot("Change Track Effect");
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        let track = &mut self.arrangement.tracks[i];
        let effects = track.effects.get_or_insert_with(TrackEffectState::default);
        let mut all = serde_json::to_value(&*effects).unwrap_or(Value::Null);
        if let Some(fx) = all.get_mut(effect.key()).and_then(Value::as_object_mut) {
            for (k, v) in params {
                fx.insert(k.clone(), v.clone());
            }
        }
        if let Ok(next) = serde_json::from_value::<TrackEffectState>(all) {
            *effects = next;
        }
        self.emit(DocEvent::TrackEffectChanged {
            track_id: track_id.to_owned(),
            effect,
        });
        self.emit_state_change();
    }

    // --- Recording arm ---

    /// `armTrackForRecording(id)`: arm this track only (None: disarm everything). Arming a drum
    /// track also makes it the active drum track.
    pub fn arm_track_for_recording(&mut self, track_id: Option<&str>) {
        self.armed_track_ids = track_id.map(str::to_owned).into_iter().collect();
        if let Some(id) = track_id
            && self.drum_track_index(id).is_some()
        {
            self.set_active_drum_track(id);
        }
        self.emit(DocEvent::ArmChanged);
        self.emit_state_change();
    }

    /// Arm or disarm one track, leaving the others as they are (several tracks can be armed:
    /// each is heard live and recorded). Unknown tracks are ignored.
    pub fn set_track_armed(&mut self, track_id: &str, armed: bool) {
        if self.track_index(track_id).is_none() {
            return;
        }
        let was = self.armed_track_ids.iter().any(|id| id == track_id);
        self.armed_track_ids.retain(|id| id != track_id);
        if armed {
            self.armed_track_ids.push(track_id.to_owned());
            if self.drum_track_index(track_id).is_some() {
                self.set_active_drum_track(track_id);
            }
        }
        if was != armed {
            self.emit(DocEvent::ArmChanged);
        }
        self.emit_state_change();
    }

    // --- Live input and hosted plugins ---

    /// The device input an audio track listens to while armed. No history (a routing setting,
    /// like volume).
    pub fn set_track_input(&mut self, track_id: &str, input: crate::types::TrackInput) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        if self.arrangement.tracks[i].input == Some(input) {
            return;
        }
        self.arrangement.tracks[i].input = Some(input);
        self.emit(DocEvent::TrackInputChanged(track_id.to_owned()));
        self.emit_state_change();
    }

    /// Append an insert plugin to a track — undo 'Add Plugin'. An empty id gets a fresh one.
    /// Returns the plugin's id.
    pub fn add_track_plugin(
        &mut self,
        track_id: &str,
        mut plugin: crate::types::PluginSlot,
    ) -> Option<String> {
        let i = self.track_index(track_id)?;
        self.push_undo_snapshot("Add Plugin");
        if plugin.id.is_empty() {
            plugin.id = self.next_id();
        }
        let id = plugin.id.clone();
        // A new plugin goes to the end of the chain (the order is written out for that).
        let track = &mut self.arrangement.tracks[i];
        let mut chain = track_chain(track);
        chain.push(ChainItem::Plugin(id.clone()));
        track.fx_order = chain.iter().map(ChainItem::key).collect();
        track.plugins.push(plugin);
        self.emit(DocEvent::TrackPluginsChanged(track_id.to_owned()));
        self.emit_state_change();
        Some(id)
    }

    /// Remove an insert plugin — undo 'Remove Plugin'.
    pub fn remove_track_plugin(&mut self, track_id: &str, plugin_id: &str) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        if !self.arrangement.tracks[i]
            .plugins
            .iter()
            .any(|p| p.id == plugin_id)
        {
            return;
        }
        self.push_undo_snapshot("Remove Plugin");
        self.arrangement.tracks[i]
            .plugins
            .retain(|p| p.id != plugin_id);
        self.emit(DocEvent::TrackPluginsChanged(track_id.to_owned()));
        self.emit_state_change();
    }

    /// Move an insert plugin to position `to` in its track's chain — undo 'Move Plugin'.
    pub fn move_track_plugin(&mut self, track_id: &str, plugin_id: &str, to: usize) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        let plugins = &self.arrangement.tracks[i].plugins;
        let Some(from) = plugins.iter().position(|p| p.id == plugin_id) else {
            return;
        };
        let to = to.min(plugins.len() - 1);
        if from == to {
            return;
        }
        self.push_undo_snapshot("Move Plugin");
        let plugins = &mut self.arrangement.tracks[i].plugins;
        let p = plugins.remove(from);
        plugins.insert(to, p);
        self.emit(DocEvent::TrackPluginsChanged(track_id.to_owned()));
        self.emit_state_change();
    }

    /// Bypass an insert plugin (no history, like mute).
    pub fn set_track_plugin_bypassed(&mut self, track_id: &str, plugin_id: &str, bypassed: bool) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        let Some(p) = self.arrangement.tracks[i]
            .plugins
            .iter_mut()
            .find(|p| p.id == plugin_id)
        else {
            return;
        };
        if p.bypassed == bypassed {
            return;
        }
        p.bypassed = bypassed;
        self.emit(DocEvent::TrackPluginsChanged(track_id.to_owned()));
        self.emit_state_change();
    }

    /// A synth track's instrument becomes a hosted plugin (None: back to its built-in preset) —
    /// undo 'Change Instrument'. The track takes the plugin's name.
    pub fn set_track_plugin_instrument(
        &mut self,
        track_id: &str,
        plugin: Option<crate::types::PluginSlot>,
    ) {
        let Some(i) = self.track_index(track_id) else {
            return;
        };
        if self.arrangement.tracks[i].instrument.kind != InstrumentType::Synth {
            return;
        }
        let mut next = self.arrangement.tracks[i].instrument.clone();
        next.plugin = plugin.map(|mut p| {
            if p.id.is_empty() {
                p.id = self.next_id();
            }
            p
        });
        self.set_track_instrument(track_id, &next);
    }

    /// Store what a plugin reports as its state (its editor changed a knob, a preset was
    /// loaded). No history and no audio side effect: the loaded plugin already is in that
    /// state. Finds the plugin among every track's inserts and instruments. True when stored.
    pub fn set_plugin_state(&mut self, plugin_id: &str, state: Option<String>) -> bool {
        let slot = self.arrangement.tracks.iter_mut().find_map(|t| {
            if t.instrument
                .plugin
                .as_ref()
                .is_some_and(|p| p.id == plugin_id)
            {
                t.instrument.plugin.as_mut()
            } else {
                t.plugins.iter_mut().find(|p| p.id == plugin_id)
            }
        });
        match slot {
            Some(p) if p.state != state => {
                p.state = state;
                self.emit_state_change();
                true
            }
            _ => false,
        }
    }

    /// Every hosted plugin of the arrangement (inserts and instruments) with its track's id.
    pub fn plugin_slots(&self) -> Vec<(&str, &crate::types::PluginSlot)> {
        self.arrangement
            .tracks
            .iter()
            .flat_map(|t| {
                t.instrument
                    .plugin
                    .iter()
                    .chain(t.plugins.iter())
                    .map(move |p| (t.id.as_str(), p))
            })
            .collect()
    }

    // --- Track groups ---

    pub fn get_groups(&self) -> &[TrackGroup] {
        self.arrangement.groups.as_deref().unwrap_or(&[])
    }

    /// `createGroup(name, trackIds)` — undo 'Create Group'. A track is in at most one group.
    pub fn create_group(&mut self, name: &str, track_ids: &[String]) -> TrackGroup {
        self.push_undo_snapshot("Create Group");
        let id = self.next_id();
        let groups = self.arrangement.groups.get_or_insert_with(Vec::new);
        for group in groups.iter_mut() {
            group.track_ids.retain(|t| !track_ids.contains(t));
        }
        groups.retain(|g| !g.track_ids.is_empty());
        let color = TRACK_COLORS[groups.len() % TRACK_COLORS.len()].to_owned();
        let group = TrackGroup {
            id,
            name: name.to_owned(),
            color,
            track_ids: track_ids.to_vec(),
            collapsed: false,
            extra: Default::default(),
        };
        groups.push(group.clone());
        self.emit_state_change();
        group
    }

    /// `removeGroup(id)` — undo 'Remove Group'.
    pub fn remove_group(&mut self, id: &str) {
        self.push_undo_snapshot("Remove Group");
        let Some(groups) = &mut self.arrangement.groups else {
            return;
        };
        groups.retain(|g| g.id != id);
        self.emit_state_change();
    }

    /// `renameGroup(id, name)` — undo 'Rename Group'.
    pub fn rename_group(&mut self, id: &str, name: &str) {
        self.push_undo_snapshot("Rename Group");
        let Some(groups) = &mut self.arrangement.groups else {
            return;
        };
        if let Some(g) = groups.iter_mut().find(|g| g.id == id) {
            g.name = name.to_owned();
        }
        self.emit_state_change();
    }

    /// `toggleGroupCollapsed(id)` (no undo).
    pub fn toggle_group_collapsed(&mut self, id: &str) {
        let Some(groups) = &mut self.arrangement.groups else {
            return;
        };
        if let Some(g) = groups.iter_mut().find(|g| g.id == id) {
            g.collapsed = !g.collapsed;
        }
        self.emit_state_change();
    }

    /// `addTrackToGroup(groupId, trackId)` — undo 'Add Track to Group'.
    pub fn add_track_to_group(&mut self, group_id: &str, track_id: &str) {
        self.push_undo_snapshot("Add Track to Group");
        let Some(groups) = &mut self.arrangement.groups else {
            return;
        };
        for g in groups.iter_mut() {
            g.track_ids.retain(|t| t != track_id);
        }
        if let Some(g) = groups.iter_mut().find(|g| g.id == group_id) {
            g.track_ids.push(track_id.to_owned());
        }
        groups.retain(|g| !g.track_ids.is_empty());
        self.emit_state_change();
    }

    /// `removeTrackFromGroup(groupId, trackId)` — undo 'Remove Track from Group'.
    pub fn remove_track_from_group(&mut self, group_id: &str, track_id: &str) {
        self.push_undo_snapshot("Remove Track from Group");
        let Some(groups) = &mut self.arrangement.groups else {
            return;
        };
        if let Some(g) = groups.iter_mut().find(|g| g.id == group_id) {
            g.track_ids.retain(|t| t != track_id);
        }
        groups.retain(|g| !g.track_ids.is_empty());
        self.emit_state_change();
    }

    /// `setGroupMute(groupId, muted)`: `setTrackMute` on each member (no undo).
    pub fn set_group_mute(&mut self, group_id: &str, muted: bool) {
        for id in self.group_track_ids(group_id) {
            self.set_track_mute(&id, muted);
        }
    }

    /// `setGroupSolo(groupId, solo)`: `setTrackSolo` on each member (no undo).
    pub fn set_group_solo(&mut self, group_id: &str, solo: bool) {
        for id in self.group_track_ids(group_id) {
            self.set_track_solo(&id, solo);
        }
    }

    fn group_track_ids(&self, group_id: &str) -> Vec<String> {
        self.get_groups()
            .iter()
            .find(|g| g.id == group_id)
            .map(|g| g.track_ids.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod chain_tests {
    use super::*;

    fn keys(t: &Track) -> Vec<String> {
        track_chain(t).iter().map(ChainItem::key).collect()
    }

    fn slot(id: &str) -> crate::types::PluginSlot {
        serde_json::from_value(serde_json::json!({
            "id": id, "type": "aufx", "subtype": "abcd", "manufacturer": "efgh"
        }))
        .unwrap()
    }

    #[test]
    fn the_chain_defaults_to_plugins_first_and_follows_a_saved_order() {
        let mut doc = ArrangementDoc::new();
        let id = doc.add_track(InstrumentType::Synth, 0).id;
        let t = |d: &ArrangementDoc| d.get_tracks().iter().find(|t| t.id == id).unwrap().clone();
        let default = [
            "filter",
            "distortion",
            "eq",
            "compressor",
            "chorus",
            "delay",
            "reverb",
        ];
        assert_eq!(keys(&t(&doc)), default);
        assert!(
            t(&doc).fx_order.is_empty(),
            "nothing is saved until it changes"
        );
        // An older project's plugins (no saved order) run first.
        let mut old = t(&doc);
        old.plugins.push(slot("p0"));
        assert_eq!(keys(&old)[0], "plugin:p0");
        assert_eq!(keys(&old)[1..], default);
        // A plugin added here goes to the end; "Add effect" moves the effect to the end and
        // turns it on (one undo step).
        doc.add_track_plugin(&id, slot("p1"));
        assert_eq!(keys(&t(&doc)).last().unwrap(), "plugin:p1");
        doc.add_track_effect(&id, EffectName::Eq);
        assert_eq!(keys(&t(&doc)).last().unwrap(), "eq");
        assert!(t(&doc).effects.unwrap().eq.enabled);
        assert_eq!(doc.undo_manager().undo_label(), Some("Add Effect"));
        // A saved order: unknown names and plugins that are gone are skipped, and what it
        // leaves out follows it.
        let order = [
            ChainItem::Effect(EffectName::Reverb),
            ChainItem::Plugin("p1".into()),
            ChainItem::Plugin("gone".into()),
        ];
        doc.set_track_chain(&id, &order, "Reorder Effects");
        let got = keys(&t(&doc));
        assert_eq!(got[..3], ["reverb", "plugin:p1", "filter"]);
        assert_eq!(got.len(), 8);
        doc.remove_track_plugin(&id, "p1");
        assert_eq!(keys(&t(&doc))[..2], ["reverb", "filter"]);
        // It survives a save and a load.
        let json = serde_json::to_value(t(&doc)).unwrap();
        assert_eq!(json["fxOrder"][0], "reverb");
        let back: Track = serde_json::from_value(json).unwrap();
        assert_eq!(keys(&back), keys(&t(&doc)));
    }
}
