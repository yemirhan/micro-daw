//! Track I/O actions of the Song view: arming, live inputs, hosted plugins, recorded takes.
//!
//! - **Arm** ([`App::toggle_arm`]): each track has its own arm button; an armed track is heard
//!   live (an audio track's input through its plugins, the keys on a synth track's instrument)
//!   and recorded by the Song view's Record.
//! - **Plugins**: insert effects per track and a plugin instrument per synth track, from the
//!   installed Audio Units (`crate::plugins`); each has an editor window.
//! - **Takes**: when a recording stops, every armed audio track's take
//!   (`mdaw_engine::engine::RecordedTake`) is written as a 32-bit float WAV into
//!   `<data dir>/recordings`, registered as a sample, and added to the track as an audio region
//!   at the beat the recording started.

use mdaw_dsp::buffer::AudioBuffer;
use mdaw_engine::engine::RecordedTake;
use mdaw_engine::plugin::{PluginFactory, PluginProcessor};
use mdaw_model::{PluginSlot, TrackInput};
use mdaw_plugin_host::PluginKind;

use crate::app::App;
use crate::services::RunMode;

/// How often an open plugin editor's state is read back into the project (seconds).
const STATE_POLL: f64 = 1.5;

/// The offline processors of an export, made on the UI thread and rendered on the worker.
pub struct PreparedPlugins(pub std::collections::HashMap<String, Box<dyn PluginProcessor>>);

impl PluginFactory for PreparedPlugins {
    fn processor(&mut self, p: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        self.0.remove(&p.id)
    }
    fn offline_processor(&mut self, p: &PluginSlot, _: u32) -> Option<Box<dyn PluginProcessor>> {
        self.0.remove(&p.id)
    }
}

/// A file name made of `name`'s letters, digits, spaces, dashes and underscores.
pub fn safe_file_stem(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = s.trim();
    if s.is_empty() {
        "Take".to_owned()
    } else {
        s.chars().take(48).collect()
    }
}

impl App {
    // ---------------------------------------------------------------- arming and inputs

    /// The arm button: arm or disarm this track; the other tracks keep their state.
    pub fn toggle_arm(&mut self, id: &str) {
        let armed = self.svc.engine.doc().is_track_armed(id);
        self.svc.engine.edit(|d| d.set_track_armed(id, !armed));
    }

    /// An audio track's input (first channel 0-based, mono or a stereo pair).
    pub fn set_track_input(&mut self, id: &str, channel: u32, stereo: bool) {
        self.svc
            .engine
            .edit(|d| d.set_track_input(id, TrackInput { channel, stereo }));
    }

    /// Channels of the open input device; while it is closed, what the track menus offer
    /// (the settings' device, else 2).
    pub fn input_channel_count(&self) -> usize {
        match self.svc.engine.input_channels() {
            0 => self
                .p9
                .audio_devices
                .input_channels(&self.p9.settings.audio),
            n => n,
        }
    }

    // ---------------------------------------------------------------- plugins

    fn plugin_slot(&self, plugin_id: &str) -> Option<PluginSlot> {
        self.svc
            .engine
            .doc()
            .plugin_slots()
            .into_iter()
            .find(|(_, p)| p.id == plugin_id)
            .map(|(_, p)| p.clone())
    }

    /// Add the installed effect `index` (of the effect list) to a track's inserts and show its
    /// editor.
    pub fn add_track_plugin(&mut self, track_id: &str, index: usize) {
        let slot = {
            let m = self.svc.plugins.0.borrow();
            m.of_kind(PluginKind::Effect)
                .get(index)
                .map(|i| crate::plugins::slot_for(i))
        };
        let Some(slot) = slot else { return };
        let id = self.svc.engine.edit(|d| d.add_track_plugin(track_id, slot));
        if let Some(id) = id {
            self.open_plugin_editor(&id);
        }
    }

    pub fn remove_track_plugin(&mut self, track_id: &str, plugin_id: &str) {
        // An undo brings the plugin back as it is now, not as it was last saved.
        self.sync_plugin_states();
        self.svc
            .engine
            .edit(|d| d.remove_track_plugin(track_id, plugin_id));
        self.retain_plugins();
    }

    pub fn toggle_plugin_bypass(&mut self, track_id: &str, plugin_id: &str) {
        let on = self.plugin_slot(plugin_id).is_some_and(|p| p.bypassed);
        self.svc
            .engine
            .edit(|d| d.set_track_plugin_bypassed(track_id, plugin_id, !on));
    }

    /// A synth track plays the installed instrument `index` (None: its built-in preset again).
    pub fn set_track_plugin_instrument(&mut self, track_id: &str, index: Option<usize>) {
        let slot = index.and_then(|i| {
            let m = self.svc.plugins.0.borrow();
            m.of_kind(PluginKind::Instrument)
                .get(i)
                .map(|i| crate::plugins::slot_for(i))
        });
        if index.is_some() && slot.is_none() {
            return;
        }
        self.sync_plugin_states();
        let opened = slot.is_some();
        self.svc
            .engine
            .edit(|d| d.set_track_plugin_instrument(track_id, slot));
        self.retain_plugins();
        if opened
            && let Some(p) = self
                .find_track(track_id)
                .and_then(|t| t.instrument.plugin.clone())
        {
            self.open_plugin_editor(&p.id);
        }
    }

    /// Show a plugin's editor window.
    pub fn open_plugin_editor(&mut self, plugin_id: &str) {
        let Some(slot) = self.plugin_slot(plugin_id) else {
            return;
        };
        let rate = self.svc.engine.sample_rate();
        if let Err(e) = self.svc.plugins.0.borrow_mut().open_editor(&slot, rate) {
            eprintln!("[plugins] {}: {e}", slot.name);
        }
    }

    /// Unload the plugins the document no longer has.
    pub fn retain_plugins(&mut self) {
        let doc = self.svc.engine.doc();
        let ids: Vec<&str> = doc
            .plugin_slots()
            .into_iter()
            .map(|(_, p)| p.id.as_str())
            .collect();
        self.svc.plugins.0.borrow_mut().retain(&ids);
    }

    /// Read every loaded plugin's state into the project (no undo entry). True when one
    /// changed (the project is then unsaved).
    pub fn sync_plugin_states(&mut self) -> bool {
        self.capture_plugin_states(false)
    }

    /// [`Self::sync_plugin_states`]; `force` also reads the plugins whose state never reads
    /// the same twice (an explicit save wants them too).
    pub fn capture_plugin_states(&mut self, force: bool) -> bool {
        let changes = {
            let doc = self.svc.engine.doc();
            let slots = doc.plugin_slots();
            self.svc.plugins.0.borrow_mut().capture_states(force, |id| {
                slots
                    .iter()
                    .find(|(_, p)| p.id == id)
                    .map(|(_, p)| p.state.clone())
            })
        };
        let changed = !changes.is_empty();
        for (id, state) in changes {
            self.svc
                .engine
                .edit(|d| d.set_plugin_state(&id, Some(state)));
        }
        changed
    }

    /// Run an undo / redo with the plugins' states outside of it: what the plugins are set to
    /// now is in the document before (so the snapshot the step leaves behind has it) and in
    /// the restored document after (a plugin's knobs are not turned back by an unrelated
    /// undo); plugins the step removed are unloaded.
    pub fn with_plugin_states_kept(&mut self, step: impl FnOnce(&mut Self)) {
        self.sync_plugin_states();
        step(self);
        self.retain_plugins();
        self.sync_plugin_states();
    }

    /// The plugins' frame: while an editor is open its plugin's state is read back every
    /// [`STATE_POLL`] seconds (the autosave follows). True when the view must re-run.
    pub fn plugins_poll(&mut self, now: f64) -> bool {
        let (version, editing) = {
            let m = self.svc.plugins.0.borrow();
            (m.version(), m.any_editor_open())
        };
        if editing && now >= self.plugin_state_due {
            self.plugin_state_due = now + STATE_POLL;
            self.sync_plugin_states();
        }
        std::mem::replace(&mut self.plugins_seen, version) != version
    }

    /// The offline processors of the project's plugins for an export (UI thread: plugins load
    /// here), keyed by slot id.
    pub fn prepare_export_plugins(&mut self) -> PreparedPlugins {
        let rate = self.svc.engine.sample_rate();
        let slots: Vec<PluginSlot> = self
            .svc
            .engine
            .doc()
            .get_tracks()
            .iter()
            .filter(|t| !t.muted)
            .flat_map(|t| {
                t.instrument
                    .plugin
                    .iter()
                    .chain(t.plugins.iter().filter(|p| !p.bypassed))
                    .cloned()
            })
            .collect();
        let mut plugins = self.svc.plugins.clone();
        PreparedPlugins(
            slots
                .iter()
                .filter_map(|p| Some((p.id.clone(), plugins.offline_processor(p, rate)?)))
                .collect(),
        )
    }

    // ---------------------------------------------------------------- takes

    /// Turn the audio takes a finished recording left into files and regions. True when one
    /// was added.
    pub fn collect_takes(&mut self) -> bool {
        let takes = self.svc.engine.take_recorded_audio();
        if takes.is_empty() {
            return false;
        }
        for take in takes {
            self.add_take(take);
        }
        true
    }

    fn add_take(&mut self, take: RecordedTake) {
        let track_name = self
            .find_track(&take.track_id)
            .map_or_else(|| "Audio".to_owned(), |t| t.name.clone());
        self.take_counter += 1;
        let name = format!("{track_name} take {}", self.take_counter);
        let buffer = AudioBuffer {
            sample_rate: take.sample_rate,
            channels: take.channels,
        };
        // Window runs keep the audio in a file (the project refers to it by path); headless
        // runs keep it in memory.
        let path = if self.svc.mode == RunMode::Window {
            let dir = crate::services::data_dir().join("recordings");
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis());
            let file = dir.join(format!("{}-{stamp}.wav", safe_file_stem(&name)));
            let written = std::fs::create_dir_all(&dir).and_then(|_| {
                mdaw_model::fsio::write_atomic(&file, &mdaw_dsp::audio_io::encode_wav_f32(&buffer))
            });
            match written {
                Ok(_) => file.to_string_lossy().into_owned(),
                Err(e) => {
                    eprintln!("[recording] could not write {}: {e}", file.display());
                    self.svc.autosave_error =
                        Some(format!("The recording could not be saved to disk: {e}"));
                    String::new()
                }
            }
        } else {
            String::new()
        };
        let sample = self.p9.samples.register_file(&name, &path, buffer);
        self.svc
            .engine
            .edit(|d| d.add_audio_region(&take.track_id, Some(&sample), take.start_beat));
    }
}

#[cfg(test)]
mod tests {
    use super::safe_file_stem;

    #[test]
    fn take_file_names_are_plain() {
        assert_eq!(safe_file_stem("Guitar take 3"), "Guitar take 3");
        assert_eq!(safe_file_stem("a/b:c*?"), "a_b_c__");
        assert_eq!(safe_file_stem("  "), "Take");
        assert_eq!(safe_file_stem(&"x".repeat(80)).len(), 48);
    }
}
