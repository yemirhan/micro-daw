//! `ArrangementDoc`: the pure data / undo side of `src/services/ArrangementEngine.ts`.
//!
//! Everything the engine does to (or reads from) the `Arrangement` lives here, with the same
//! method names (snake_case), the same undo labels and the same quirks (see "Quirks kept" below).
//! What the TS does to audio or the transport (rebuild track nodes, `withPlaybackRestart`,
//! `refreshPlaybackSchedule`, `syncDrumSink`, `applyLoopToTransport`, stop a loop take...) is
//! reported as a typed [`DocEvent`] in an outbox the engine drains ([`ArrangementDoc::drain_events`]).
//! The engine's listener groups become counters: `emitStateChange` bumps
//! [`ArrangementDoc::state_version`], `emitKitChange` bumps [`ArrangementDoc::kit_version`],
//! `onKitSelected` callbacks become [`DocEvent::KitSelected`].
//!
//! Engine-only state (transport state, Tone nodes, metronome, the loop recorder's timing, the
//! song-recording capture buffers) stays out. The pure part of a loop-recording take
//! (`LoopRecordingSession` minus its count-in time) is held here as [`LoopTake`], because the
//! arrangement edits read it (`getActiveLoopRange`, `setLoopSettings`, `removeTrack`...).
//!
//! Hooks the engine provides:
//! - [`ArrangementDoc::set_take_end_hook`]: called synchronously wherever the TS ends a running
//!   loop take (`loopRecorder.end()` in `endLoopTake`, incl. `settleHistory` before undo/redo),
//!   BEFORE the doc's own bookkeeping, so the engine can close held gate notes through
//!   [`ArrangementDoc::loop_take_set_gate_length`] at the same point as the TS.
//! - [`ArrangementDoc::set_id_generator`] (= `crypto.randomUUID`, uuid v4 by default).
//! - [`ArrangementDoc::set_packs_dir`] (`currentPacksDir()` used by `normalizeKit` / `normalizePad`).
//!
//! Quirks kept (1:1 with the TS):
//! - most mutators push the undo snapshot BEFORE validating the target (a missing track still
//!   leaves a no-op entry); `setKit` / `updatePad` / `setPadSample` / `setStepVelocity` /
//!   `clearPattern` / `editRegionNotes` validate first;
//! - `setTrackVolume/Mute/Pan/Solo`, `toggleAutomationLaneVisibility`, `toggleGroupCollapsed`,
//!   `setLoopEnabled`, `setBpm`, `setSelectedPad`, `setLoopSettings`, `setActiveDrumTrack` write
//!   no history;
//! - the TS getters that lazily create state (`beat()` creates the beat state,
//!   `getActiveDrumTrackId` rewrites a stale id, `getKit` creates a missing kit) are `&self` here
//!   (review M5: views only get `&State`); they return the same values, and the stores happen
//!   where the TS persists them anyway (`syncDrumSink` → `resolve_active_drum_track_id`, every
//!   mutator through `beat()`), which the doc goldens check after every op;
//! - `setTrackInstrument` renames a non-drum track to its preset name (even audio);
//! - `withPlaybackRestart` stops a running take with its own 'Loop Record' entry AFTER the edit's
//!   snapshot; `restoreFromSnapshot` never writes history;
//! - `splitRegion` / `moveRegionToTrack` don't auto-extend the song; an empty region name is falsy.
//!
//! Deviations: `duplicateRegion` of an audio region does not alias the `audio` object between the
//! copies (the TS spread shares it, so a later left-trim of one moves both offsets); unknown
//! effect names / ill-typed effect params are ignored instead of throwing.

use serde_json::Value;

use crate::kit::create_factory_kit;
use crate::loop_settings::normalize_beat_state;
use crate::migrate::{
    DroppedDrumNotes, MigrateError, ProjectLoadError, migrate_arrangement_in_place, random_uuid,
    remap_template_ids_value,
};
use crate::open::{CoerceWarning, open_arrangement_value};
use crate::types::{Arrangement, BeatState, InstrumentType, Track, TrackEffectState};
use crate::undo::UndoManager;

mod beat;
mod events;
mod pattern;
mod regions;
mod sounds;
mod tracks;

pub use beat::{LoopHit, LoopSettingsPatch, LoopTake, LoopTakeStart};
pub use events::{DocEvent, KitSelectedEvent, KitSelectionSource, UnavailableAudioRegion};
pub use regions::{MarkerUpdate, RegionClipboard};
pub use tracks::EffectName;

/// `DEFAULT_BPM` (src/utils/constants.ts).
pub const DEFAULT_BPM: f64 = 120.0;
/// `DEFAULT_ARRANGEMENT_LENGTH`: 64 beats (16 bars).
pub const DEFAULT_ARRANGEMENT_LENGTH: f64 = 64.0;

/// `TRACK_COLORS` (src/utils/constants.ts), cycled by `addTrack` and `createGroup`.
pub const TRACK_COLORS: [&str; 8] = [
    "oklch(0.65 0.20 250)",
    "oklch(0.65 0.20 155)",
    "oklch(0.62 0.22 25)",
    "oklch(0.70 0.18 65)",
    "oklch(0.62 0.22 310)",
    "oklch(0.75 0.17 90)",
    "oklch(0.62 0.16 195)",
    "oklch(0.65 0.20 345)",
];

/// `MARKER_COLORS` (src/utils/constants.ts), cycled by `addMarker`.
pub const MARKER_COLORS: [&str; 8] = [
    "oklch(0.65 0.20 25)",
    "oklch(0.70 0.18 65)",
    "oklch(0.75 0.17 90)",
    "oklch(0.65 0.20 155)",
    "oklch(0.65 0.20 250)",
    "oklch(0.62 0.22 310)",
    "oklch(0.65 0.20 345)",
    "oklch(0.62 0.16 195)",
];

/// `SYNTH_PRESETS[i].name` (src/audio/presets.ts), used for track names.
pub const SYNTH_PRESET_NAMES: [&str; 41] = [
    "Grand Piano",
    "Saw Lead",
    "FM Keys",
    "Warm Pad",
    "Sync Lead",
    "Synth Pluck",
    "Supersaw Pad",
    "Analog Bass",
    "Sub Bass",
    "Drawbar Organ",
    "Square Lead",
    "Tubular Bell",
    "String Ensemble",
    "Rhodes EP",
    "Synth Brass",
    "Bright Piano",
    "Marimba",
    "Music Box",
    "Vibraphone",
    "FM Bass",
    "Acid Bass",
    "Pulse Lead",
    "Theremin",
    "Airy Pad",
    "Reese Bass",
    "Pluck Bass",
    "Mono Lead",
    "Glide Lead",
    "Nylon Guitar",
    "Harp",
    "Kalimba",
    "Timpani",
    "Steel Drum",
    "Wobble Bass",
    "Sweep Pad",
    "Laser Zap",
    "Wurlitzer",
    "Rock Organ",
    "Supersaw Lead",
    "Brass Section",
    "Glass Pad",
];

/// `SYNTH_PRESETS[presetIndex]?.name ?? 'Synth'`.
pub fn synth_preset_name(preset_index: i64) -> &'static str {
    usize::try_from(preset_index)
        .ok()
        .and_then(|i| SYNTH_PRESET_NAMES.get(i))
        .copied()
        .unwrap_or("Synth")
}

/// The id generator (`crypto.randomUUID`).
pub type IdGenerator = Box<dyn FnMut() -> String>;
/// Called where the TS ends a running loop take (see the module docs).
pub type TakeEndHook = Box<dyn FnMut(&mut ArrangementDoc)>;

/// The arrangement document: arrangement + undo history + the engine's pure bookkeeping.
pub struct ArrangementDoc {
    pub(crate) arrangement: Arrangement,
    pub(crate) undo: UndoManager<Arrangement>,
    /// Project session (review N2): bumped when another project replaces the arrangement.
    pub(crate) session_id: u64,
    /// `colorIndex`: next `TRACK_COLORS` entry for `addTrack` (reset by `resetToStarter` only).
    pub(crate) color_index: usize,
    /// The Song loop toggle (`loopEnabled`).
    pub(crate) loop_enabled: bool,
    pub(crate) armed_track_id: Option<String>,
    pub(crate) clipboard: Option<RegionClipboard>,
    pub(crate) loop_take: Option<LoopTake>,
    pub(crate) packs_dir: Option<String>,
    pub(crate) new_id: IdGenerator,
    take_end_hook: Option<TakeEndHook>,
    events: Vec<DocEvent>,
    state_version: u64,
    kit_version: u64,
}

impl Default for ArrangementDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ArrangementDoc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ArrangementDoc")
            .field("arrangement", &self.arrangement)
            .field("session_id", &self.session_id)
            .field("loop_enabled", &self.loop_enabled)
            .field("loop_take", &self.loop_take.as_ref().map(|t| &t.track_id))
            .finish_non_exhaustive()
    }
}

/// `JSON.stringify` of a snapshot (undo-group "unchanged" test).
fn json_string(a: &Arrangement) -> String {
    serde_json::to_string(a).unwrap_or_default()
}

impl ArrangementDoc {
    /// A fresh doc with random uuids (the engine constructor: `createDefaultArrangement()`).
    pub fn new() -> Self {
        Self::with_id_generator(Box::new(random_uuid))
    }

    /// A fresh doc whose ids come from `new_id` (the default arrangement takes the first one).
    pub fn with_id_generator(mut new_id: IdGenerator) -> Self {
        let arrangement = default_arrangement(new_id());
        Self {
            arrangement,
            undo: UndoManager::new(),
            session_id: 0,
            color_index: 0,
            loop_enabled: false,
            armed_track_id: None,
            clipboard: None,
            loop_take: None,
            packs_dir: None,
            new_id,
            take_end_hook: None,
            events: Vec::new(),
            state_version: 0,
            kit_version: 0,
        }
    }

    /// Replace the id generator (tests / golden replays).
    pub fn set_id_generator(&mut self, new_id: IdGenerator) {
        self.new_id = new_id;
    }

    /// The packs folder (`currentPacksDir()`), used when kits / pads are normalized.
    pub fn set_packs_dir(&mut self, dir: Option<String>) {
        self.packs_dir = dir;
    }

    /// Install the engine's take-end hook (see module docs).
    pub fn set_take_end_hook(&mut self, hook: Option<TakeEndHook>) {
        self.take_end_hook = hook;
    }

    // --- Notifications ---------------------------------------------------------------------

    /// Bumped by every `emitStateChange()`.
    pub fn state_version(&self) -> u64 {
        self.state_version
    }

    /// Bumped by every `emitKitChange()`.
    pub fn kit_version(&self) -> u64 {
        self.kit_version
    }

    /// Pending side effects for the engine, oldest first (not cleared).
    pub fn events(&self) -> &[DocEvent] {
        &self.events
    }

    /// Take the pending side effects, oldest first.
    pub fn drain_events(&mut self) -> Vec<DocEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn emit(&mut self, event: DocEvent) {
        self.events.push(event);
    }

    /// `emitStateChange()` (engine-only state changes call this too, via [`Self::notify_state_change`]).
    pub(crate) fn emit_state_change(&mut self) {
        self.state_version += 1;
    }

    pub(crate) fn emit_kit_change(&mut self) {
        self.kit_version += 1;
    }

    /// For the engine: notify state listeners of an engine-only change (transport, metronome...).
    pub fn notify_state_change(&mut self) {
        self.emit_state_change();
    }

    // --- Getters -----------------------------------------------------------------------------

    pub fn get_arrangement(&self) -> &Arrangement {
        &self.arrangement
    }

    pub fn get_tracks(&self) -> &[Track] {
        &self.arrangement.tracks
    }

    pub fn get_bpm(&self) -> f64 {
        self.arrangement.bpm
    }

    pub fn is_loop_enabled(&self) -> bool {
        self.loop_enabled
    }

    pub fn get_loop_markers(&self) -> Option<crate::types::LoopMarkers> {
        self.arrangement.loop_markers
    }

    pub fn get_armed_track_id(&self) -> Option<&str> {
        self.armed_track_id.as_deref()
    }

    /// The undo history (labels, can_undo...).
    pub fn undo_manager(&self) -> &UndoManager<Arrangement> {
        &self.undo
    }

    /// Push a ready-made undo entry (e.g. the song recorder's own snapshot).
    pub fn push_undo_entry(&mut self, label: &str, snapshot: Arrangement) {
        self.undo.push_undo(label, snapshot);
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
    }

    // --- Project session (review N2) ---------------------------------------------------------

    /// Current project session id; changes on new / open / template / first load only.
    pub fn get_session_id(&self) -> u64 {
        self.session_id
    }

    fn begin_session(&mut self) {
        self.session_id += 1;
    }

    // --- Helpers -------------------------------------------------------------------------------

    pub(crate) fn track_index(&self, id: &str) -> Option<usize> {
        self.arrangement.tracks.iter().position(|t| t.id == id)
    }

    pub(crate) fn find_track(&self, id: &str) -> Option<&Track> {
        self.arrangement.tracks.iter().find(|t| t.id == id)
    }

    pub(crate) fn drum_track_index(&self, id: &str) -> Option<usize> {
        self.track_index(id)
            .filter(|&i| self.arrangement.tracks[i].instrument.kind == InstrumentType::Drums)
    }

    pub(crate) fn next_id(&mut self) -> String {
        (self.new_id)()
    }

    /// `beat()`: the beat state, created on first use (a getter that mutates, as in the TS).
    pub(crate) fn beat(&mut self) -> &mut BeatState {
        if self.arrangement.beat.is_none() {
            let b = normalize_beat_state(None, &self.arrangement.tracks);
            self.arrangement.beat = Some(b);
        }
        self.arrangement.beat.as_mut().expect("just created")
    }

    /// `autoExtendLength()`: `ceil(max(64, max(start + len + 4)) / 4) * 4`.
    pub(crate) fn auto_extend_length(&mut self) {
        let mut max_beat = DEFAULT_ARRANGEMENT_LENGTH;
        for track in &self.arrangement.tracks {
            for region in &track.regions {
                max_beat = js_max(max_beat, region.start_beat + region.length_beats + 4.0);
            }
        }
        self.arrangement.length_beats = (max_beat / 4.0).ceil() * 4.0;
    }

    /// The data half of `createTrackAudio`: `pan ??= 0` (always set here), `effects ??= defaults`,
    /// a drum track's `kit ??= createFactoryKit()`.
    pub(crate) fn ensure_track_defaults(&mut self, index: usize) {
        let track = &mut self.arrangement.tracks[index];
        track.effects.get_or_insert_with(TrackEffectState::default);
        if track.instrument.kind == InstrumentType::Drums {
            track.instrument.kit.get_or_insert_with(create_factory_kit);
        }
    }

    // --- Undo -----------------------------------------------------------------------------------

    /// `pushUndoSnapshot(label)`: inside a group only marks it dirty (no clone).
    pub(crate) fn push_undo_snapshot(&mut self, label: &str) {
        if self.undo.is_grouping() {
            self.undo.mark_dirty();
            return;
        }
        self.undo.push_undo(label, self.arrangement.clone());
    }

    /// `undoSnapshot()`: "before" snapshot for `commit_undo` (None inside a group).
    pub(crate) fn undo_snapshot(&self) -> Option<Arrangement> {
        (!self.undo.is_grouping()).then(|| self.arrangement.clone())
    }

    pub(crate) fn commit_undo(&mut self, label: &str, snapshot: Option<Arrangement>) {
        match snapshot {
            Some(s) => self.undo.push_undo(label, s),
            None => self.undo.mark_dirty(),
        }
    }

    /// Group edits into ONE undo step (nests; the outermost begin captures state and label).
    pub fn begin_undo_group(&mut self, label: &str) {
        let arrangement = &self.arrangement;
        self.undo.begin_undo_group(label, || arrangement.clone());
    }

    /// Close one level; the last end commits unless the state is JSON-equal to the start.
    pub fn end_undo_group(&mut self) {
        let current = json_string(&self.arrangement);
        let unchanged = move |before: &Arrangement| json_string(before) == current;
        self.undo.end_undo_group(Some(&unchanged));
    }

    /// `settleHistory()`: end a running take (no history) and flush an open group as its own step.
    fn settle_history(&mut self) {
        if self.loop_take.is_some() {
            self.end_loop_take(false);
        }
        let current = json_string(&self.arrangement);
        let unchanged = move |before: &Arrangement| json_string(before) == current;
        let arrangement = &self.arrangement;
        self.undo
            .flush_group(|| arrangement.clone(), Some(&unchanged));
    }

    pub fn perform_undo(&mut self) {
        self.settle_history();
        let Some(snapshot) = self.undo.undo() else {
            return;
        };
        self.undo.push_redo("Undo", self.arrangement.clone());
        self.restore_from_snapshot(&snapshot);
        if self.undo.is_grouping() {
            self.undo.rebase_group(self.arrangement.clone());
        }
    }

    pub fn perform_redo(&mut self) {
        self.settle_history();
        let Some(snapshot) = self.undo.redo() else {
            return;
        };
        self.undo
            .push_undo_from_redo("Redo", self.arrangement.clone());
        self.restore_from_snapshot(&snapshot);
        if self.undo.is_grouping() {
            self.undo.rebase_group(self.arrangement.clone());
        }
    }

    // --- Loop take end --------------------------------------------------------------------------

    /// `endLoopTake(recordStructure)` (AE:2796-2805). `stopLoopRecording()` is `end_loop_take(true)`.
    pub(crate) fn end_loop_take(&mut self, record_structure: bool) {
        if self.loop_take.is_none() {
            return;
        }
        // `loopRecorder.end()`: the engine closes held gate notes (take still set).
        if let Some(mut hook) = self.take_end_hook.take() {
            hook(self);
            if self.take_end_hook.is_none() {
                self.take_end_hook = Some(hook);
            }
        }
        let Some(take) = self.loop_take.take() else {
            return;
        };
        if record_structure && !take.undo_pushed && take.structure_changed {
            self.undo.push_undo("Loop Record", take.undo_snapshot);
        }
        // AE:2806 `this.stopTransport()`.
        self.emit(DocEvent::LoopTakeEnded {
            track_id: take.track_id,
        });
        self.emit_state_change();
    }

    /// The take-ending half of `withPlaybackRestart` (AE:1618): `if (this.loopRec) this.stopLoopRecording()`.
    pub(crate) fn with_playback_restart_prelude(&mut self) {
        if self.loop_take.is_some() {
            self.end_loop_take(true);
        }
    }

    // --- Project replacement -------------------------------------------------------------------

    /// `createDefaultArrangement()`.
    pub fn create_default_arrangement(&mut self) -> Arrangement {
        default_arrangement(self.next_id())
    }

    /// `restoreFromSnapshot(snapshot)`: never writes history; migrates (typed, infallible: see
    /// [`migrate_arrangement_in_place`] for why snapshots are still migrated); a different
    /// project id starts a new session.
    pub fn restore_from_snapshot(&mut self, snapshot: &Arrangement) {
        if snapshot.id != self.arrangement.id {
            self.begin_session();
        }
        if self.loop_take.is_some() {
            self.end_loop_take(false);
        }
        self.with_playback_restart_prelude();
        let mut migrated = snapshot.clone();
        let warnings = migrate_arrangement_in_place(
            &mut migrated,
            self.packs_dir.as_deref(),
            &mut *self.new_id,
        );
        self.install(migrated, warnings);
    }

    /// Install a migrated arrangement: track defaults, then (AE:1597-1605 / 1541-1546) dispose
    /// every track's nodes and create them again, sync the drum sink, notify.
    fn install(&mut self, arrangement: Arrangement, warnings: Vec<DroppedDrumNotes>) {
        self.arrangement = arrangement;
        for w in warnings {
            self.emit(DocEvent::MigrationWarning(w));
        }
        for i in 0..self.arrangement.tracks.len() {
            self.ensure_track_defaults(i);
        }
        self.emit(DocEvent::RebuildAll);
        self.sync_drum_sink();
        self.emit_state_change();
        self.emit_kit_change();
    }

    /// `loadProject(arrangement)`: always a new session.
    pub fn load_project(&mut self, arrangement: &Arrangement) {
        self.begin_session();
        self.restore_from_snapshot(arrangement);
    }

    /// `loadFromTemplate(arrangement)`: new session, fresh ids (arrangement, tracks, regions,
    /// markers, groups, remapped references), then restore.
    pub fn load_from_template(&mut self, template: &Arrangement) -> Result<(), MigrateError> {
        self.begin_session();
        let mut v = serde_json::to_value(template).map_err(|e| MigrateError(e.to_string()))?;
        remap_template_ids_value(&mut v, &mut *self.new_id)?;
        let clone: Arrangement =
            serde_json::from_value(v).map_err(|e| MigrateError(e.to_string()))?;
        self.restore_from_snapshot(&clone);
        Ok(())
    }

    /// `resetToStarter()`: one active "Drums" track, default loop settings, pattern loop markers,
    /// no undo history.
    pub fn reset_to_starter(&mut self) {
        self.begin_session();
        let fresh = self.create_default_arrangement();
        self.restore_from_snapshot(&fresh);
        self.loop_enabled = false;
        self.color_index = 0;
        self.ensure_drum_track();
        self.set_loop_settings(&LoopSettingsPatch::default());
        let (start, length) = (self.get_pattern_start(), self.get_pattern_length());
        self.apply_pattern_loop(start, length);
        self.undo.clear();
    }

    /// `saveToLocalStorage()`: the JSON the engine stores.
    pub fn save_to_local_storage(&self) -> String {
        json_string(&self.arrangement)
    }

    /// `loadFromLocalStorage()` from the stored string. `Ok(None)`: nothing stored (the TS
    /// returns false). `Ok(Some(warnings))`: loaded, through the same migrate + coerce path as a
    /// project file ([`crate::open::open_arrangement_value`]), with what had to be coerced.
    /// `Err`: invalid JSON or a shape the TS migration throws on (the TS returns false; like the
    /// TS, the session is already bumped when the migration fails, and the current arrangement
    /// is kept).
    pub fn load_from_local_storage(
        &mut self,
        raw: Option<&str>,
    ) -> Result<Option<Vec<CoerceWarning>>, ProjectLoadError> {
        let Some(raw) = raw.filter(|r| !r.is_empty()) else {
            return Ok(None);
        };
        let data = serde_json::from_str::<Value>(raw)
            .map_err(|e| ProjectLoadError::Json(e.to_string()))?;
        self.begin_session();
        let opened = open_arrangement_value(data, self.packs_dir.as_deref(), &mut *self.new_id)?;
        self.install(opened.arrangement, opened.dropped_drum_notes);
        Ok(Some(opened.warnings))
    }
}

fn default_arrangement(id: String) -> Arrangement {
    Arrangement {
        id,
        name: "Untitled".to_owned(),
        bpm: DEFAULT_BPM,
        length_beats: DEFAULT_ARRANGEMENT_LENGTH,
        tracks: Vec::new(),
        loop_markers: None,
        markers: None,
        groups: None,
        beat: None,
        sounds: Vec::new(),
        extra: serde_json::Map::new(),
    }
}

// --- JS number semantics (shared with the rest of the crate) -------------------------------------

pub(crate) use crate::js::{max as js_max, min as js_min, round as js_round};

/// `clamp01`: non-finite → 0.
pub(crate) fn clamp01(v: f64) -> f64 {
    js_min(1.0, js_max(0.0, if v.is_finite() { v } else { 0.0 }))
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_beat;
#[cfg(test)]
mod tests_sounds;
