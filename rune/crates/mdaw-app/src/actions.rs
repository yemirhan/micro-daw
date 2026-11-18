//! `useBeatActions` (`src/hooks/useBeat.ts`) and the other engine calls the Beat view makes,
//! as methods on [`App`]: every call goes through `ArrangementEngine::edit` (doc + undo) or the
//! engine's transport API, exactly as the TS calls `arrangementEngine`.

use mdaw_model::beat_helpers::{
    ChopMode, ChopTrim, DroppedSample, PadLayout, StepGrid, chop_ranges, file_sample, plan_chop,
};
use mdaw_model::doc::{KitSelectionSource, LoopSettingsPatch};
use mdaw_model::kit::{clone_kit, create_factory_kit, normalize_kit};
use mdaw_model::settings as store;
use mdaw_model::slicer::SliceRange;
use mdaw_model::types::{KitState, NoteRepeatRate, PadSample, PadSettings};
use serde_json::{Map, Value, json};

use crate::app::{App, ChopUi, EDIT_IDLE, EditGroup, Menu, STEP_BURST_IDLE, StepGesture};

/// `normalizeKit` for kits read from storage (fresh ids where one is missing).
pub fn normalize_stored_kit(v: &Value) -> KitState {
    normalize_kit(v, None, &mut || uuid::Uuid::new_v4().to_string())
}

/// What to do with pads whose pack can't be downloaded any more (`MissingPadFix`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingPadFix {
    Clear,
    Builtin,
}

/// `Partial<PadSettings>` as the doc's JSON patch.
pub fn patch(v: Value) -> Map<String, Value> {
    match v {
        Value::Object(m) => m,
        _ => Map::new(),
    }
}

impl App {
    /// `track()`: the active drum track, created when there is none.
    pub fn track(&mut self) -> String {
        if let Some(id) = self.svc.engine.doc().get_active_drum_track_id() {
            return id;
        }
        self.svc.engine.edit(|d| d.ensure_drum_track())
    }

    pub fn active_track(&self) -> Option<String> {
        self.svc.engine.doc().get_active_drum_track_id()
    }

    /// The kit the Beat view shows (the active drum track's, else a factory kit).
    pub fn kit(&self) -> KitState {
        match self.active_track() {
            Some(id) => self.svc.engine.doc().get_kit(&id),
            None => create_factory_kit(),
        }
    }

    pub fn selected_pad(&self) -> usize {
        self.svc.engine.doc().get_selected_pad()
    }

    // --- pads -------------------------------------------------------------------------------

    /// `selectPad`.
    pub fn select_pad(&mut self, pad: usize) {
        self.svc.engine.edit(|d| d.set_selected_pad(pad));
    }

    /// `updatePad` (not grouped: the context menu's colour and choke).
    pub fn update_pad(&mut self, pad: usize, p: Value) {
        let id = self.track();
        let p = patch(p);
        self.svc.engine.edit(|d| d.update_pad(&id, pad, &p));
    }

    /// `editPad`: like `updatePad`, merged into one "Edit pad" undo step with neighbouring calls;
    /// the group closes on `endPadEdit` or after 700 ms idle.
    pub fn edit_pad(&mut self, pad: usize, p: Value) {
        if self.edit_group.is_none() {
            self.svc.engine.edit(|d| d.begin_undo_group("Edit pad"));
        }
        self.edit_group = Some(EditGroup {
            due: self.now + EDIT_IDLE,
        });
        let id = self.track();
        let p = patch(p);
        self.svc.engine.edit(|d| d.update_pad(&id, pad, &p));
    }

    /// `endPadEdit`.
    pub fn end_pad_edit(&mut self) {
        if self.edit_group.take().is_some() {
            self.svc.engine.edit(|d| d.end_undo_group());
        }
    }

    /// `assignFile`.
    pub fn assign_file(&mut self, pad: usize, path: &str, name: &str) {
        let id = self.track();
        let sample = file_sample(&DroppedSample {
            path: path.to_owned(),
            name: name.to_owned(),
        });
        self.svc
            .engine
            .edit(|d| d.set_pad_sample(&id, pad, Some(&sample)));
    }

    /// `clearPad`.
    pub fn clear_pad(&mut self, pad: usize) {
        let id = self.track();
        self.svc.engine.edit(|d| d.set_pad_sample(&id, pad, None));
    }

    /// `renamePad`: trimmed; empty, unchanged or sample-less pads are left alone.
    pub fn rename_pad(&mut self, pad: usize, name: &str) {
        let id = self.track();
        let Some(sample) = self
            .svc
            .engine
            .doc()
            .get_kit(&id)
            .pads
            .get(pad)
            .and_then(|p| p.sample.clone())
        else {
            return;
        };
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed == sample.name {
            return;
        }
        let renamed = PadSample {
            name: trimmed.to_owned(),
            ..sample
        };
        self.svc
            .engine
            .edit(|d| d.set_pad_sample(&id, pad, Some(&renamed)));
    }

    /// `copyPad`.
    pub fn copy_pad(&mut self, pad: usize) {
        let id = self.track();
        self.clipboard = self.svc.engine.doc().get_kit(&id).pads.get(pad).cloned();
    }

    /// `pastePad`: one "Paste pad" undo step (sample, then the other settings).
    pub fn paste_pad(&mut self, pad: usize) {
        let Some(clip) = self.clipboard.clone() else {
            return;
        };
        let id = self.track();
        let mut rest = serde_json::to_value(&clip).unwrap_or(Value::Null);
        if let Value::Object(m) = &mut rest {
            m.remove("sample");
        }
        let rest = patch(rest);
        self.svc.engine.edit(|d| {
            d.begin_undo_group("Paste pad");
            d.set_pad_sample(&id, pad, clip.sample.as_ref());
            d.update_pad(&id, pad, &rest);
            d.end_undo_group();
        });
    }

    /// `chopToPads`: one "Chop to pads" undo step; every pad but the source gets a copy of the
    /// source named "{name} {i+1}"; every pad gets its slice (`reverse: false`).
    pub fn chop_to_pads(&mut self, from: usize, sample: &PadSample, ranges: &[SliceRange]) {
        let plan = plan_chop(from, ranges);
        if plan.is_empty() {
            return;
        }
        let id = self.track();
        self.svc.engine.edit(|d| {
            d.begin_undo_group("Chop to pads");
            for a in &plan {
                if a.pad != from {
                    let copy = PadSample {
                        source: sample.source.clone(),
                        name: format!("{} {}", sample.name, a.pad - from + 1),
                    };
                    d.set_pad_sample(&id, a.pad, Some(&copy));
                }
                let p = patch(
                    json!({ "startSec": a.start_sec, "endSec": a.end_sec, "reverse": false }),
                );
                d.update_pad(&id, a.pad, &p);
            }
            d.end_undo_group();
        });
    }

    /// The chop panel's slices of the selected pad (recomputed while the panel is open).
    pub fn recompute_slices(&mut self) {
        if self.menu != Some(Menu::Chop) {
            self.beat.slices.clear();
            return;
        }
        let pad = self.selected_pad();
        let kit = self.kit();
        let Some(settings) = kit.pads.get(pad).cloned() else {
            return;
        };
        let cache = self.svc.engine.cache().clone();
        let (buf, _) = self
            .buffers
            .borrow_mut()
            .get(&cache, settings.sample.as_ref().map(|s| &s.source));
        let Some(buf) = buf else {
            self.beat.slices.clear();
            return;
        };
        let mode = match self.beat.chop {
            ChopUi::Transients { sensitivity } => ChopMode::Transients { sensitivity },
            ChopUi::Equal { count } => ChopMode::Equal { count },
        };
        self.beat.slices = chop_ranges(
            buf.as_ref(),
            ChopTrim {
                start_sec: settings.start_sec,
                end_sec: settings.end_sec,
            },
            mode,
            pad,
        );
    }

    /// Applies the open chop panel (`applyChop`).
    pub fn apply_chop(&mut self) {
        let pad = self.selected_pad();
        let Some(sample) = self.kit().pads.get(pad).and_then(|p| p.sample.clone()) else {
            return;
        };
        if self.beat.slices.is_empty() {
            return;
        }
        let slices = self.beat.slices.clone();
        self.chop_to_pads(pad, &sample, &slices);
        self.menu = None;
        self.beat.slices.clear();
    }

    /// `onChop` from the context menu: select the pad and open the chop panel.
    pub fn request_chop(&mut self, pad: usize) {
        self.select_pad(pad);
        let has_sample = self.kit().pads.get(pad).is_some_and(|p| p.sample.is_some());
        if has_sample {
            self.menu = Some(Menu::Chop);
            self.recompute_slices();
        }
    }

    /// `fixMissingPads`: "Clear pads" / "Replace pads", one undo step.
    pub fn fix_missing_pads(&mut self, pads: &[usize], fix: MissingPadFix) {
        let Some(id) = self.active_track() else {
            return;
        };
        if pads.is_empty() {
            return;
        }
        let factory = (fix == MissingPadFix::Builtin).then(create_factory_kit);
        self.svc.engine.edit(|d| {
            d.begin_undo_group(if fix == MissingPadFix::Clear {
                "Clear pads"
            } else {
                "Replace pads"
            });
            for &pad in pads {
                let sample = factory
                    .as_ref()
                    .and_then(|k| k.pads.get(pad))
                    .and_then(|p| p.sample.clone());
                d.set_pad_sample(&id, pad, sample.as_ref());
                if sample.is_some() {
                    let p = patch(json!({ "startSec": 0, "endSec": null, "reverse": false }));
                    d.update_pad(&id, pad, &p);
                }
            }
            d.end_undo_group();
        });
    }

    // --- steps ------------------------------------------------------------------------------

    /// `beginStepGesture(label)`.
    pub fn begin_step_gesture(&mut self, label: &str) {
        self.end_step_gesture();
        self.step_gesture = Some(StepGesture::Drag);
        self.svc.engine.edit(|d| d.begin_undo_group(label));
    }

    /// `endStepGesture`.
    pub fn end_step_gesture(&mut self) {
        if self.step_gesture.take().is_some() {
            self.svc.engine.edit(|d| d.end_undo_group());
        }
    }

    /// `toggleStep` (default velocity: the engine's 0.8).
    pub fn toggle_step(&mut self, pad: usize, step: usize, step_beats: f64) {
        let id = self.track();
        self.svc
            .engine
            .edit(|d| d.toggle_step(&id, pad, step as i64, step_beats, None));
    }

    /// `setStepVelocity`.
    pub fn set_step_velocity(&mut self, pad: usize, step: usize, step_beats: f64, v: f64) {
        let id = self.track();
        self.svc
            .engine
            .edit(|d| d.set_step_velocity(&id, pad, step as i64, step_beats, v));
    }

    /// `nudgeStepVelocity`: inside a drag it joins the drag's step; otherwise a "Step velocity"
    /// burst that closes after 400 ms of quiet.
    pub fn nudge_step_velocity(&mut self, pad: usize, step: usize, step_beats: f64, v: f64) {
        if self.step_gesture.is_none() {
            self.svc
                .engine
                .edit(|d| d.begin_undo_group("Step velocity"));
            self.step_gesture = Some(StepGesture::Burst { due: 0.0 });
        }
        if let Some(StepGesture::Burst { due }) = &mut self.step_gesture {
            *due = self.now + STEP_BURST_IDLE;
        }
        self.set_step_velocity(pad, step, step_beats, v);
    }

    /// `clearPattern(pad?)`: only with an active drum track.
    pub fn clear_pattern(&mut self) {
        if let Some(id) = self.active_track() {
            self.svc.engine.edit(|d| d.clear_pattern(&id, None));
        }
    }

    /// `loopPattern`: "Loop pattern" undo step; loop on + markers on the pattern.
    pub fn loop_pattern(&mut self) {
        let start = self.pattern_start();
        let len = self.svc.engine.doc().get_loop_settings().bars as f64 * 4.0;
        self.svc.engine.edit(|d| {
            d.begin_undo_group("Loop pattern");
            if !d.is_loop_enabled() {
                d.set_loop_enabled(true);
            }
            d.set_loop_markers(start, start + len);
            d.end_undo_group();
        });
    }

    // --- transport --------------------------------------------------------------------------

    /// `togglePlay`: play the pattern when stopped (and not recording), else stop.
    pub fn toggle_play(&mut self) {
        if !self.playing() && !self.loop_recording() {
            self.svc.engine.play_pattern();
        } else {
            if self.loop_recording() {
                self.svc.engine.stop_loop_recording();
            }
            self.svc.engine.stop();
        }
    }

    /// `toggleRecord`.
    pub fn toggle_record(&mut self) {
        if self.loop_recording() {
            self.svc.engine.stop_loop_recording();
        } else {
            self.track();
            self.svc.engine.start_loop_recording();
        }
    }

    /// `setBpm` (the BPM field clamps and rounds before calling it).
    pub fn set_bpm(&mut self, bpm: f64) {
        self.svc.engine.edit(|d| d.set_bpm(bpm));
    }

    /// `setLoopSettings(patch)`.
    pub fn set_loop_settings(&mut self, p: LoopSettingsPatch) {
        self.svc.engine.edit(|d| d.set_loop_settings(&p));
    }

    /// The Beat view's M: the loop recorder's metronome.
    pub fn toggle_loop_metronome(&mut self) {
        let on = self.svc.engine.doc().get_loop_settings().metronome;
        self.set_loop_settings(LoopSettingsPatch {
            metronome: Some(!on),
            ..Default::default()
        });
    }

    /// `noteRepeat.toggle()`.
    pub fn toggle_note_repeat(&mut self) {
        self.repeat.enabled = !self.repeat.enabled;
        self.push_note_repeat();
    }

    /// `noteRepeat.setRate`.
    pub fn set_repeat_rate(&mut self, rate: NoteRepeatRate) {
        self.repeat.rate = rate;
        self.push_note_repeat();
    }

    fn push_note_repeat(&mut self) {
        let (on, beats) = (self.repeat.enabled, self.repeat.rate.beats());
        self.svc.engine.handle().set_note_repeat(on, beats);
    }

    /// Undo / redo (`useUndoRedo`).
    pub fn undo(&mut self) {
        self.end_pad_edit();
        self.end_step_gesture();
        self.svc.engine.edit(|d| d.perform_undo());
    }

    pub fn redo(&mut self) {
        self.end_pad_edit();
        self.end_step_gesture();
        self.svc.engine.edit(|d| d.perform_redo());
    }

    // --- kits ---------------------------------------------------------------------------------

    /// `loadKit`: drops a pending Get & load, then `setKit(track, clone, 'user')`.
    pub fn load_kit(&mut self, kit: &KitState) {
        self.svc.packs.cancel_pending_load();
        let id = self.track();
        let k = clone_kit(kit);
        self.svc
            .engine
            .edit(|d| d.set_kit(&id, &k, KitSelectionSource::User));
    }

    /// `loadFactoryKit`.
    pub fn load_factory_kit(&mut self) {
        self.load_kit(&create_factory_kit());
    }

    /// `saveKit(name)`: trimmed or "My kit"; a kit of the same name is overwritten.
    pub fn save_kit(&mut self, name: &str) {
        let id = self.track();
        let mut kit = clone_kit(&self.svc.engine.doc().get_kit(&id));
        let trimmed = match name.trim() {
            "" => "My kit",
            t => t,
        };
        let existing = self
            .user_kits
            .iter()
            .find(|k| k.name == trimmed)
            .map(|k| k.id.clone());
        kit.id = existing.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        kit.name = trimmed.to_owned();
        let mut s = self.svc.settings.borrow_mut();
        let _ = store::save_kit(&mut s, &kit, normalize_stored_kit);
        self.user_kits = store::read_kits(&s, normalize_stored_kit);
    }

    /// `deleteKit`.
    pub fn delete_kit(&mut self, kit_id: &str) {
        let mut s = self.svc.settings.borrow_mut();
        let _ = store::remove_kit(&mut s, kit_id, normalize_stored_kit);
        self.user_kits = store::read_kits(&s, normalize_stored_kit);
    }

    /// `setActiveDrumTrack`.
    pub fn set_active_drum_track(&mut self, id: &str) {
        self.svc.engine.edit(|d| d.set_active_drum_track(id));
    }

    /// `newDrumTrack`: add a drums track and make it active.
    pub fn new_drum_track(&mut self) -> String {
        self.svc.engine.edit(|d| {
            let id = d.add_track(mdaw_model::types::InstrumentType::Drums, 0).id;
            d.set_active_drum_track(&id);
            id
        })
    }

    // --- preferences --------------------------------------------------------------------------

    pub fn set_layout(&mut self, layout: PadLayout) {
        self.beat.layout = layout;
        self.with_settings_mut(|s| {
            let _ = store::write_beat_pad_layout(s, layout.label());
        });
    }

    pub fn set_grid(&mut self, grid: StepGrid) {
        self.beat.grid = grid;
        self.with_settings_mut(|s| {
            let _ = store::write_beat_step_grid(s, grid.label());
        });
    }

    pub fn toggle_key_hints(&mut self) {
        self.beat.show_keys = !self.beat.show_keys;
        let on = self.beat.show_keys;
        self.with_settings_mut(|s| {
            let _ = store::write_beat_key_hints(s, on);
        });
    }

    pub fn toggle_browser(&mut self) {
        self.browser_open = !self.browser_open;
        let open = self.browser_open;
        self.with_settings_mut(|s| {
            let _ = store::write_browser_open(s, open);
        });
    }

    /// The pad's settings for the editor (None past the kit's pads).
    pub fn pad_settings(&self, pad: usize) -> Option<PadSettings> {
        self.kit().pads.get(pad).cloned()
    }
}
