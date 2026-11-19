//! The Samples page: the Sound Designer's state and actions (the view is
//! `ui::p9::designer`).
//!
//! - **Sounds** are [`DesignedSound`]s: a synth recipe, or an imported file with non-destructive
//!   edits. They live in the project (`Arrangement::sounds`, undoable, autosaved with the
//!   project) and in the user's library (`micro-daw:sound-library`, every project sees them).
//!   The list shows both; for a sound in both, the project's copy wins (it is what pads play).
//! - **The draft** is the sound in the editor. Knobs edit the draft; its render (the waveform,
//!   Play / Space) and the MIDI preview synth follow at once. Save writes it to the project and
//!   the library (one undo step); leaving a draft with unsaved edits (another sound, a template,
//!   another page, closing the window) saves it first, so no edit is lost. A template that was
//!   never edited is just discarded.
//! - **Rendering**: every sound of the project and the library is rendered at the context rate
//!   and registered under its id ([`crate::samples::Samples::put_sound`]); when its definition
//!   changes (save, undo, redo, project open) it is rendered again, the pads playing it reload
//!   ([`mdaw_engine::buffers::PadBufferCache::invalidate_sound`]) and so do the Song's audio
//!   regions.
//! - **Using a sound**: Send to pad (the chosen drum track and pad; default: the active drum
//!   track and the selected pad) or Add to song (an audio track with the sound at beat 0).

use std::collections::HashMap;
use std::sync::Arc;

use mdaw_dsp::buffer::{AudioBuffer, SharedBuffer};
use mdaw_dsp::sound_design::render_sound;
use mdaw_model::settings::{
    SAMPLE_LIBRARY_STORAGE_KEY, SOUND_LIBRARY_KEY, SerializedSampleEntry, read_sample_library,
    read_sound_library, write_sound_library,
};
use mdaw_model::sound_design::{DesignedSound, SampleEdit, SoundRecipe, Template};
use mdaw_model::types::SampleRef;
use rune::{KeyEvent, TextEdit, Tx};

use crate::app::{App, AppMode, DawView};

/// The sound in the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub sound: DesignedSound,
    /// Edited since it was opened or saved.
    pub dirty: bool,
}

/// How a library row was created (the list's badge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Synth,
    Sample,
}

pub struct Designer {
    pub draft: Option<Draft>,
    /// The user library (settings), in its stored order.
    pub library: Vec<DesignedSound>,
    pub search: TextEdit,
    pub rename: Option<TextEdit>,
    /// The draft's render (the waveform, Play).
    pub preview: Option<SharedBuffer>,
    preview_of: Option<DesignedSound>,
    /// Definitions whose render is registered in the sample store.
    rendered: HashMap<String, DesignedSound>,
    /// Decoded source files of sample-based sounds (None: unreadable).
    decoded: HashMap<String, Option<SharedBuffer>>,
    /// (started, duration) of the audition (UI clock).
    pub audition: Option<(f64, f64)>,
    pub audition_x: rune::Live<[f32; 2]>,
    pub wave_w: std::cell::Cell<f32>,
    /// The Send-to-pad picker's choice (None: the active drum track / the selected pad).
    pub send_track: Option<String>,
    pub send_pad: Option<usize>,
    /// A short confirmation or error under the action bar, until (UI clock).
    pub notice: Option<(String, bool, f64)>,
    loaded: bool,
    /// (doc state version, library version) the store was last synced at.
    synced: Option<(u64, u64)>,
    lib_version: u64,
    pub version: u64,
}

impl Default for Designer {
    fn default() -> Self {
        Self::new()
    }
}

impl Designer {
    pub fn new() -> Self {
        Self {
            draft: None,
            library: Vec::new(),
            search: TextEdit::new(""),
            rename: None,
            preview: None,
            preview_of: None,
            rendered: HashMap::new(),
            decoded: HashMap::new(),
            audition: None,
            audition_x: rune::Live::new([0.0, 0.0]),
            wave_w: std::cell::Cell::new(600.0),
            send_track: None,
            send_pad: None,
            notice: None,
            loaded: false,
            synced: None,
            lib_version: 0,
            version: 0,
        }
    }

    pub fn draft_sound(&self) -> Option<&DesignedSound> {
        self.draft.as_ref().map(|d| &d.sound)
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.draft.as_ref().map(|d| d.sound.id.as_str())
    }
}

pub fn kind_of(s: &DesignedSound) -> Kind {
    if s.is_sample() {
        Kind::Sample
    } else {
        Kind::Synth
    }
}

/// Every sound of the library and the project (the project's copy wins), library order first.
pub fn all_sounds(s: &App) -> Vec<DesignedSound> {
    let doc = s.svc.engine.doc();
    let mut out: Vec<DesignedSound> =
        s.p9.designer
            .library
            .iter()
            .map(|l| doc.get_sound(&l.id).cloned().unwrap_or_else(|| l.clone()))
            .collect();
    for p in doc.get_sounds() {
        if !out.iter().any(|o| o.id == p.id) {
            out.push(p.clone());
        }
    }
    out
}

/// The library list, filtered by the search field (case-insensitive substring).
pub fn listed(s: &App) -> Vec<DesignedSound> {
    let q = s.p9.designer.search.text.trim().to_lowercase();
    all_sounds(s)
        .into_iter()
        .filter(|x| q.is_empty() || x.name.to_lowercase().contains(&q))
        .collect()
}

/// `name`, or `name 2`, `name 3`... when another sound has it.
fn unique_name(s: &App, name: &str, except: &str) -> String {
    let taken: Vec<String> = all_sounds(s)
        .into_iter()
        .filter(|x| x.id != except)
        .map(|x| x.name)
        .collect();
    if !taken.iter().any(|t| t == name) {
        return name.to_owned();
    }
    (2..)
        .map(|n| format!("{name} {n}"))
        .find(|c| !taken.contains(c))
        .unwrap_or_else(|| name.to_owned())
}

/// First mount: the library loads (once per session). Sounds imported with the old Samples
/// page (`micro-daw-sample-library`) become sample-based sounds the first time.
pub fn mount(s: &mut App) {
    if !s.p9.designer.loaded {
        s.p9.designer.loaded = true;
        let st = s.svc.settings.borrow();
        let mut lib = read_sound_library(&st);
        if st.get_item(SOUND_LIBRARY_KEY).is_none()
            && st.get_item(SAMPLE_LIBRARY_STORAGE_KEY).is_some()
        {
            for v in read_sample_library(&st) {
                let Ok(e) = serde_json::from_value::<SerializedSampleEntry>(v) else {
                    continue;
                };
                if e.sample_path.is_empty() || lib.iter().any(|l| l.id == e.sample_id) {
                    continue;
                }
                lib.push(DesignedSound {
                    id: e.sample_id,
                    name: e.sample_name,
                    recipe: SoundRecipe::default(),
                    sample: Some(SampleEdit {
                        start: e.trim.map_or(0.0, |t| t.start_seconds),
                        end: e.trim.map(|t| t.end_seconds),
                        ..SampleEdit::new(&e.sample_path)
                    }),
                });
            }
            drop(st);
            let _ = write_sound_library(&mut s.svc.settings.borrow_mut(), &lib);
        } else {
            drop(st);
        }
        s.p9.designer.library = lib;
        s.p9.designer.lib_version += 1;
    }
    if s.p9.designer.draft.is_none() {
        match all_sounds(s).into_iter().next() {
            Some(first) => s.open_sound(&first.id, false),
            None => s.pick_template(Template::Kick808, false),
        }
    }
    let recipe =
        s.p9.designer
            .draft_sound()
            .map(|d| d.recipe)
            .unwrap_or_default();
    crate::sound_preview::mount(s, recipe);
}

/// Leaving the page: the draft's edits are saved, the audition and the preview synth stop.
pub fn unmount(s: &mut App) {
    s.flush_sample_drafts();
    s.stop_sound();
    crate::sound_preview::unmount(s);
}

/// Per frame: renders follow the definitions, the draft's preview, MIDI notes, the audition.
pub fn poll(s: &mut App, now: f64, shown: bool) -> bool {
    let mut changed = sync_store(s);
    if shown {
        changed |= refresh_preview(s);
        for (note, vel) in crate::sound_preview::take_note_ons() {
            if s.p9.designer.draft_sound().is_some_and(|d| d.is_sample()) {
                s.play_sound();
            } else {
                crate::sound_preview::play(s, note, vel);
            }
        }
    }
    let d = &mut s.p9.designer;
    if let Some((start, dur)) = d.audition {
        if now - start >= dur {
            d.audition = None;
            changed = true;
        } else {
            let p = ((now - start) / dur.max(1e-9)).clamp(0.0, 1.0);
            d.audition_x.snap([p as f32 * d.wave_w.get(), 0.0]);
        }
    }
    if let Some((_, _, until)) = d.notice
        && now >= until
    {
        d.notice = None;
        changed = true;
    }
    changed
}

/// Renders the sounds whose definition changed since their last render (project and library),
/// registers them, and lets the pads and audio regions playing them reload.
fn sync_store(s: &mut App) -> bool {
    let key = (
        s.svc.engine.doc().state_version(),
        s.p9.designer.lib_version,
    );
    if s.p9.designer.synced == Some(key) {
        return false;
    }
    s.p9.designer.synced = Some(key);
    let sounds = all_sounds(s);
    let mut changed = false;
    for d in &sounds {
        if s.p9.designer.rendered.get(&d.id) == Some(d) {
            continue;
        }
        s.p9.designer.rendered.insert(d.id.clone(), d.clone());
        if let Some(buf) = render_with_files(s, d) {
            s.p9.samples.put_sound(&d.id, &d.name, buf);
            s.svc.engine.cache().invalidate_sound(&d.id);
            changed = true;
        }
    }
    let gone: Vec<String> =
        s.p9.designer
            .rendered
            .keys()
            .filter(|id| !sounds.iter().any(|d| &d.id == *id))
            .cloned()
            .collect();
    for id in gone {
        s.p9.designer.rendered.remove(&id);
        s.p9.samples.remove_sound(&id);
        s.svc.engine.cache().invalidate_sound(&id);
        changed = true;
    }
    changed
}

/// Renders `d` at the context rate (decoding a sample-based sound's file once).
fn render_with_files(s: &mut App, d: &DesignedSound) -> Option<AudioBuffer> {
    let rate = s.p9.samples.rate();
    let decoded = match &d.sample {
        Some(e) => {
            let path = e.path.clone();
            if !s.p9.designer.decoded.contains_key(&path) {
                let r = s.svc.engine.cache().decode_file_direct(&path).ok();
                s.p9.designer.decoded.insert(path.clone(), r);
            }
            s.p9.designer.decoded.get(&path).cloned().flatten()
        }
        None => None,
    };
    render_sound(d, rate, decoded.as_deref())
}

/// The draft's render follows the draft (and the preview synth its recipe).
fn refresh_preview(s: &mut App) -> bool {
    let Some(d) = s.p9.designer.draft_sound().cloned() else {
        return false;
    };
    if s.p9.designer.preview_of.as_ref() == Some(&d) {
        return false;
    }
    crate::sound_preview::set_recipe(&d.recipe);
    let buf = render_with_files(s, &d).map(Arc::new);
    let des = &mut s.p9.designer;
    des.preview = buf;
    des.preview_of = Some(d);
    des.version += 1;
    true
}

/// The Samples page's keys (no text field focused): Space plays, ↑ / ↓ move through the
/// library (and play), Escape closes the pad picker.
pub fn keys(s: &mut App, ev: &KeyEvent, tx: &mut Tx, focused: Option<&str>) -> bool {
    if ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
        return false;
    }
    let on_control = focused.is_some_and(|f| {
        let leaf = f.rsplit('/').next().unwrap_or(f);
        leaf.starts_with("sd-k-") || leaf.starts_with("sd-seg")
    });
    match &ev.key {
        rune::Key::Text(t) if t == " " => {
            if !tx.key_repeat() {
                s.toggle_sound_play();
            }
            true
        }
        rune::Key::Up | rune::Key::Down if !on_control => {
            let list = listed(s);
            if list.is_empty() {
                return true;
            }
            let cur =
                s.p9.designer
                    .selected_id()
                    .and_then(|id| list.iter().position(|x| x.id == id));
            let next = match (cur, matches!(ev.key, rune::Key::Down)) {
                (None, _) => 0,
                (Some(i), true) => (i + 1).min(list.len() - 1),
                (Some(i), false) => i.saturating_sub(1),
            };
            if Some(next) != cur {
                let id = list[next].id.clone();
                s.open_sound(&id, true);
            }
            true
        }
        _ => false,
    }
}

impl App {
    fn notice(&mut self, msg: impl Into<String>, error: bool) {
        self.p9.designer.notice = Some((msg.into(), error, self.now + 3.0));
    }

    /// Saves the draft when it has unsaved edits (another sound opens, the page closes, a
    /// draft field blurs, the window closes); also commits a rename being typed.
    pub fn flush_sample_drafts(&mut self) {
        if self.p9.designer.rename.is_some() {
            self.commit_sound_rename();
        }
        if self.p9.designer.draft.as_ref().is_some_and(|d| d.dirty) {
            self.save_sound();
        }
    }

    /// Focus left a field: the name commits.
    pub fn p9_blur(&mut self, was: &str, now: &str) -> bool {
        if was != now && was == "designer-rename" && self.p9.designer.rename.is_some() {
            self.commit_sound_rename();
            return true;
        }
        false
    }

    fn set_draft(&mut self, sound: DesignedSound, play: bool) {
        self.stop_sound();
        self.p9.designer.rename = None;
        self.p9.designer.draft = Some(Draft {
            sound,
            dirty: false,
        });
        self.p9.designer.version += 1;
        refresh_preview(self);
        if play {
            self.play_sound();
        }
    }

    /// Opens a library sound in the editor (the current draft's edits are saved first).
    pub fn open_sound(&mut self, id: &str, play: bool) {
        if self.p9.designer.selected_id() == Some(id) {
            if play {
                self.play_sound();
            }
            return;
        }
        self.flush_sample_drafts();
        let Some(sound) = all_sounds(self).into_iter().find(|x| x.id == id) else {
            return;
        };
        self.set_draft(sound, play);
    }

    /// Starts a new, unsaved sound from a template.
    pub fn pick_template(&mut self, t: Template, play: bool) {
        self.flush_sample_drafts();
        let id = uuid::Uuid::new_v4().to_string();
        let name = unique_name(self, t.name(), &id);
        self.set_draft(
            DesignedSound {
                id,
                name,
                recipe: t.recipe(),
                sample: None,
            },
            play,
        );
    }

    /// Edits the draft's recipe (a knob, a switch...).
    pub fn edit_recipe(&mut self, f: impl FnOnce(&mut SoundRecipe)) {
        self.edit_sound(|d| f(&mut d.recipe));
    }

    /// Edits the draft.
    pub fn edit_sound(&mut self, f: impl FnOnce(&mut DesignedSound)) {
        let Some(d) = self.p9.designer.draft.as_mut() else {
            return;
        };
        let before = d.sound.clone();
        f(&mut d.sound);
        d.sound.recipe = d.sound.recipe.sanitized();
        if d.sound != before {
            d.dirty = true;
            self.p9.designer.version += 1;
            refresh_preview(self);
        }
    }

    /// The draft is in the project or the library.
    pub fn draft_is_stored(&self) -> bool {
        self.p9.designer.draft_sound().is_some_and(|d| {
            self.svc.engine.doc().get_sound(&d.id).is_some()
                || self.p9.designer.library.iter().any(|l| l.id == d.id)
        })
    }

    fn put_in_library(&mut self, sound: &DesignedSound) {
        let lib = &mut self.p9.designer.library;
        match lib.iter_mut().find(|l| l.id == sound.id) {
            Some(l) => *l = sound.clone(),
            None => lib.push(sound.clone()),
        }
        self.p9.designer.lib_version += 1;
        let _ = write_sound_library(
            &mut self.svc.settings.borrow_mut(),
            &self.p9.designer.library,
        );
    }

    /// Save: the draft goes to the project (one undo step) and the user library; pads playing
    /// it are re-rendered.
    pub fn save_sound(&mut self) {
        let Some(d) = self.p9.designer.draft.as_mut() else {
            return;
        };
        d.dirty = false;
        let sound = d.sound.clone();
        self.svc.engine.edit(|doc| doc.save_sound(&sound));
        self.put_in_library(&sound);
        sync_store(self);
        self.p9.designer.version += 1;
    }

    /// The drum track and pad "Send to pad" targets.
    pub fn send_target(&self) -> Option<(String, usize)> {
        let doc = self.svc.engine.doc();
        let track = self
            .p9
            .designer
            .send_track
            .clone()
            .filter(|t| doc.kit(t).is_some())
            .or_else(|| doc.get_active_drum_track_id())?;
        let pad = self
            .p9
            .designer
            .send_pad
            .unwrap_or_else(|| doc.get_selected_pad());
        Some((track, pad))
    }

    /// Send to pad: saves the draft and assigns it to the target pad (one undo step).
    pub fn send_sound_to_pad(&mut self) {
        self.flush_sample_drafts();
        let Some(sound) = self.p9.designer.draft_sound().cloned() else {
            return;
        };
        let (track, pad) = match self.send_target() {
            Some(t) => t,
            None => (self.track(), self.selected_pad()),
        };
        self.svc
            .engine
            .edit(|d| d.send_sound_to_pad(&track, pad, &sound));
        self.put_in_library(&sound);
        sync_store(self);
        self.menu = None;
        let kit = self.svc.engine.doc().get_kit(&track).name;
        self.notice(
            format!("“{}” is on pad {} of {kit}", sound.name, pad + 1),
            false,
        );
    }

    /// Add to song: saves the draft and puts it on a new audio track at beat 0.
    pub fn add_sound_to_song(&mut self) {
        self.flush_sample_drafts();
        let Some(sound) = self.p9.designer.draft_sound().cloned() else {
            return;
        };
        if !self.draft_is_stored() || self.p9.designer.draft.as_ref().is_some_and(|d| d.dirty) {
            self.save_sound();
        } else {
            self.svc.engine.edit(|d| d.save_sound(&sound));
            sync_store(self);
        }
        let Some(b) = self.p9.samples.buffer(&sound.id) else {
            self.notice("This sound has no audio (its file can't be read)", true);
            return;
        };
        let sample = SampleRef {
            id: sound.id.clone(),
            name: sound.name.clone(),
            path: String::new(),
            duration_seconds: b.frames() as f64 / b.sample_rate.max(1) as f64,
            sample_rate: b.sample_rate as f64,
            channels: b.channel_count() as u32,
        };
        self.svc
            .engine
            .edit(|d| d.send_sample_to_arrangement(&sample, None));
        self.switch_mode(AppMode::Daw);
        self.view = DawView::Song;
    }

    pub fn play_sound(&mut self) {
        let Some(buf) = self.p9.designer.preview.clone() else {
            return;
        };
        let dur = buf.frames() as f64 / buf.sample_rate.max(1) as f64;
        self.svc.engine.play_audition(buf, 0.0, dur);
        self.p9.designer.audition = Some((self.now, dur));
        self.p9.designer.audition_x.snap([0.0, 0.0]);
    }

    pub fn stop_sound(&mut self) {
        if self.p9.designer.audition.take().is_some() {
            self.svc.engine.stop_audition();
        }
    }

    pub fn toggle_sound_play(&mut self) {
        if self.p9.designer.audition.is_some() {
            self.stop_sound();
        } else {
            self.play_sound();
        }
    }

    pub fn begin_sound_rename(&mut self) {
        if let Some(d) = self.p9.designer.draft_sound() {
            let mut e = TextEdit::new(&d.name);
            e.focused = true;
            self.p9.designer.rename = Some(e);
        }
    }

    pub fn commit_sound_rename(&mut self) {
        let Some(edit) = self.p9.designer.rename.take() else {
            return;
        };
        let name = edit.text.trim().to_owned();
        let Some(d) = self.p9.designer.draft.as_mut() else {
            return;
        };
        if name.is_empty() || name == d.sound.name {
            return;
        }
        d.sound.name = name.clone();
        let id = d.sound.id.clone();
        let stored_in_doc = self.svc.engine.doc().get_sound(&id).is_some();
        if stored_in_doc {
            self.svc.engine.edit(|doc| doc.rename_sound(&id, &name));
        }
        if let Some(l) = self.p9.designer.library.iter_mut().find(|l| l.id == id) {
            l.name = name;
            self.p9.designer.lib_version += 1;
            let _ = write_sound_library(
                &mut self.svc.settings.borrow_mut(),
                &self.p9.designer.library,
            );
        }
        self.p9.designer.version += 1;
    }

    /// Duplicate: a stored copy ("… copy") opens.
    pub fn duplicate_sound(&mut self) {
        self.flush_sample_drafts();
        let Some(mut copy) = self.p9.designer.draft_sound().cloned() else {
            return;
        };
        copy.id = uuid::Uuid::new_v4().to_string();
        copy.name = unique_name(self, &format!("{} copy", copy.name), &copy.id);
        self.put_in_library(&copy);
        self.set_draft(copy, false);
        self.save_sound();
    }

    /// Delete: out of the library and the project; pads that played it are cleared (undo
    /// brings the project's copy and the pads back). The next sound opens.
    pub fn delete_sound(&mut self) {
        let Some(id) = self.p9.designer.selected_id().map(str::to_owned) else {
            return;
        };
        let list = listed(self);
        let at = list.iter().position(|x| x.id == id).unwrap_or(0);
        self.stop_sound();
        self.svc.engine.edit(|d| d.remove_sound(&id));
        self.p9.designer.library.retain(|l| l.id != id);
        self.p9.designer.lib_version += 1;
        let _ = write_sound_library(
            &mut self.svc.settings.borrow_mut(),
            &self.p9.designer.library,
        );
        self.p9.designer.draft = None;
        sync_store(self);
        let rest = listed(self);
        match rest.get(at.min(rest.len().saturating_sub(1))) {
            Some(n) => {
                let nid = n.id.clone();
                self.open_sound(&nid, false);
            }
            None => self.pick_template(Template::Kick808, false),
        }
    }

    /// Imports audio files as sample-based sounds (library); the first one opens and plays.
    pub fn import_sound_files(&mut self, paths: &[String]) {
        let mut first = None;
        let mut failed = Vec::new();
        for p in paths {
            if let Some(existing) = self.p9.designer.library.iter().find(|l| {
                l.sample
                    .as_ref()
                    .is_some_and(|e| e.path == *p && e.is_identity())
            }) {
                first.get_or_insert(existing.id.clone());
                continue;
            }
            let decoded = self.svc.engine.cache().decode_file_direct(p).ok();
            self.p9.designer.decoded.insert(p.clone(), decoded.clone());
            if decoded.is_none() {
                failed.push(crate::samples::sample_name_of(p));
                continue;
            }
            let sound = DesignedSound {
                id: uuid::Uuid::new_v4().to_string(),
                name: unique_name(self, &crate::samples::sample_name_of(p), ""),
                recipe: SoundRecipe::default(),
                sample: Some(SampleEdit::new(p)),
            };
            first.get_or_insert(sound.id.clone());
            self.put_in_library(&sound);
        }
        sync_store(self);
        if let Some(id) = first {
            self.open_sound(&id, true);
        }
        if !failed.is_empty() {
            self.notice(format!("Couldn't read {}", failed.join(", ")), true);
        }
    }

    /// The page's "Import audio": a file picker.
    pub fn sample_import_pick(&mut self, tx: &mut Tx) {
        let options = rune::FileDialogOptions {
            title: "Import Audio File".into(),
            directory: None,
            file_name: None,
            filters: vec![(
                "Audio".into(),
                ["wav", "mp3", "ogg", "flac", "aiff", "aif", "m4a"]
                    .iter()
                    .map(|s| (*s).to_owned())
                    .collect(),
            )],
        };
        tx.open_file(
            options,
            |s: &mut App, r: rune::FileDialogResult, _tx: &mut Tx| {
                if let Ok(Some(p)) = r {
                    s.import_sound_files(&[p.to_string_lossy().into_owned()]);
                }
            },
        );
    }

    /// The file length of the draft's sample (seconds), for the edit knobs.
    pub fn draft_file_seconds(&self) -> f64 {
        self.p9
            .designer
            .draft_sound()
            .and_then(|d| d.sample.as_ref())
            .and_then(|e| self.p9.designer.decoded.get(&e.path).cloned().flatten())
            .map_or(0.0, |b| b.frames() as f64 / b.sample_rate.max(1) as f64)
    }
}
