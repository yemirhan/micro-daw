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

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_dsp::sound_design::render_sound;
use mdaw_model::settings::{
    SAMPLE_LIBRARY_STORAGE_KEY, SOUND_LIBRARY_KEY, SerializedSampleEntry, read_sample_library,
    read_sound_library, write_sound_library,
};
use mdaw_model::sound_design::{DesignedSound, SampleEdit, SoundRecipe, Template};
use mdaw_model::types::{PadSampleSource, SampleRef};
use rune::{KeyEvent, TextEdit, Tx};

use crate::app::{App, AppMode, DawView};

/// How long a sound whose file couldn't be read waits before the next try (seconds, UI clock).
pub const RENDER_RETRY: f64 = 5.0;
/// The decoded source files kept for sample-based sounds (bytes of f32 audio).
pub const DECODE_BUDGET: usize = 256 << 20;
/// How long the Delete button waits for its confirmation click (seconds, UI clock).
pub const CONFIRM_DELETE: f64 = 6.0;

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

/// What a render job is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// A project sound's audio (registered in the sample store for pads and Song regions).
    Store,
    /// The editor's waveform / Play.
    Preview,
}

/// A finished render (worker → UI thread).
struct Done {
    target: Target,
    rev: u64,
    sound: DesignedSound,
    result: Result<SharedBuffer, String>,
    /// A source file the job decoded (cached by the UI thread): path, audio.
    decoded: Option<(String, SharedBuffer)>,
}

pub struct Designer {
    pub draft: Option<Draft>,
    /// The user library (settings), in its stored order.
    pub library: Vec<DesignedSound>,
    pub search: TextEdit,
    pub rename: Option<TextEdit>,
    /// The draft's render (the waveform, Play).
    pub preview: Option<SharedBuffer>,
    /// The definition [`Designer::preview`] was rendered from.
    preview_of: Option<DesignedSound>,
    /// The preview render in flight (revision, definition). Edits made meanwhile are coalesced:
    /// the next render starts when it lands, from the draft as it is then.
    preview_job: Option<(u64, DesignedSound)>,
    /// The preview's file couldn't be read: tried again at this time (UI clock).
    preview_retry: Option<f64>,
    /// Preview renders started (tests: edits are coalesced).
    pub preview_renders: u64,
    /// Play was asked for while the preview was still rendering: it plays when it lands.
    play_when_ready: bool,
    /// The recipe last handed to the preview synth.
    synth_recipe: Option<SoundRecipe>,
    /// Definitions whose render is registered in the sample store.
    rendered: HashMap<String, DesignedSound>,
    /// Store renders in flight: id → (revision, definition). A result whose revision is not
    /// the latest is dropped.
    pending: HashMap<String, (u64, DesignedSound)>,
    /// Sounds whose render failed (their file is unreadable): id → (definition, retry time).
    failed: HashMap<String, (DesignedSound, f64)>,
    next_rev: u64,
    tx: Sender<Done>,
    rx: Receiver<Done>,
    /// Decoded source files of sample-based sounds (bounded by [`Designer::decode_budget`],
    /// oldest dropped first; a dropped file is decoded again when needed).
    decoded: HashMap<String, SharedBuffer>,
    decoded_order: VecDeque<String>,
    decoded_bytes: usize,
    pub decode_budget: usize,
    /// Length of each source file read so far (seconds; the edit knobs' range).
    source_secs: HashMap<String, f64>,
    /// Length of each sound's last render (seconds; the list rows).
    lengths: HashMap<String, f64>,
    /// Library entries removed by Delete this session: id → (library position, entry). Undo /
    /// redo of that delete puts them back / takes them out again.
    trash: HashMap<String, (usize, DesignedSound)>,
    /// Delete waits for a second click (id, until): the sound is in use or only in the library.
    pub confirm_delete: Option<(String, f64)>,
    /// Pads playing sounds that neither the project nor the library has (names; the shell's
    /// warning card).
    pub missing: Vec<String>,
    warned: HashSet<String>,
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
    /// [`Designer::version`] at the last poll (a change made between frames redraws).
    polled_version: u64,
}

impl Default for Designer {
    fn default() -> Self {
        Self::new()
    }
}

impl Designer {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            draft: None,
            library: Vec::new(),
            search: TextEdit::new(""),
            rename: None,
            preview: None,
            preview_of: None,
            preview_job: None,
            preview_retry: None,
            preview_renders: 0,
            play_when_ready: false,
            synth_recipe: None,
            rendered: HashMap::new(),
            pending: HashMap::new(),
            failed: HashMap::new(),
            next_rev: 0,
            tx,
            rx,
            decoded: HashMap::new(),
            decoded_order: VecDeque::new(),
            decoded_bytes: 0,
            decode_budget: DECODE_BUDGET,
            source_secs: HashMap::new(),
            lengths: HashMap::new(),
            trash: HashMap::new(),
            confirm_delete: None,
            missing: Vec::new(),
            warned: HashSet::new(),
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
            polled_version: 0,
        }
    }

    pub fn draft_sound(&self) -> Option<&DesignedSound> {
        self.draft.as_ref().map(|d| &d.sound)
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.draft.as_ref().map(|d| d.sound.id.as_str())
    }

    /// The draft has unsaved edits (they count as unsaved project changes).
    pub fn draft_dirty(&self) -> bool {
        self.draft.as_ref().is_some_and(|d| d.dirty)
    }

    /// The preview shows the draft as it is now (no render pending).
    pub fn preview_current(&self) -> bool {
        self.draft.is_some() && self.preview_of.as_ref() == self.draft_sound()
    }

    /// Bytes of decoded source audio held.
    pub fn decoded_bytes(&self) -> usize {
        self.decoded_bytes
    }

    /// Seconds of the last render of sound `id`, if any.
    pub fn length_of(&self, id: &str) -> Option<f64> {
        self.lengths.get(id).copied()
    }

    /// Keeps a decoded file, dropping the oldest ones past the budget (a file larger than the
    /// whole budget is not kept).
    fn cache_decoded(&mut self, path: String, b: SharedBuffer) {
        let secs = b.frames() as f64 / b.sample_rate.max(1) as f64;
        self.source_secs.insert(path.clone(), secs);
        if self.decoded.contains_key(&path) {
            return;
        }
        let size = b.frames() * b.channel_count() * std::mem::size_of::<f32>();
        if size > self.decode_budget {
            return;
        }
        while self.decoded_bytes + size > self.decode_budget {
            let Some(old) = self.decoded_order.pop_front() else {
                break;
            };
            if let Some(o) = self.decoded.remove(&old) {
                self.decoded_bytes -= o.frames() * o.channel_count() * std::mem::size_of::<f32>();
            }
        }
        self.decoded_bytes += size;
        self.decoded_order.push_back(path.clone());
        self.decoded.insert(path, b);
    }

    /// A new project: the editor, the pending renders and the Delete history start over (the
    /// registered renders stay; the next sync re-renders what differs).
    fn reset_for_project(&mut self) {
        self.draft = None;
        self.rename = None;
        self.preview = None;
        self.preview_of = None;
        self.preview_job = None;
        self.preview_retry = None;
        self.play_when_ready = false;
        self.pending.clear();
        self.failed.clear();
        self.trash.clear();
        self.confirm_delete = None;
        self.send_track = None;
        self.send_pad = None;
        self.notice = None;
        self.missing.clear();
        self.warned.clear();
        self.synced = None;
        self.version += 1;
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

/// The stored version of sound `id` (the project's copy, else the library's).
fn stored(s: &App, id: &str) -> Option<DesignedSound> {
    s.svc
        .engine
        .doc()
        .get_sound(id)
        .cloned()
        .or_else(|| s.p9.designer.library.iter().find(|l| l.id == id).cloned())
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

/// Loads the user library (once per session, at startup: kits and pads brought in from
/// elsewhere copy their sounds from it). Sounds imported with the old Samples page
/// (`micro-daw-sample-library`) become sample-based sounds the first time, with **fresh ids**:
/// the old entry's id may be what Song regions of existing projects play (with the trim applied
/// through the region), so the sound must never be registered under it (review finding 1).
pub fn ensure_library(s: &mut App) {
    if s.p9.designer.loaded {
        return;
    }
    s.p9.designer.loaded = true;
    let st = s.svc.settings.borrow();
    let mut lib = read_sound_library(&st);
    if st.get_item(SOUND_LIBRARY_KEY).is_none() && st.get_item(SAMPLE_LIBRARY_STORAGE_KEY).is_some()
    {
        for v in read_sample_library(&st) {
            let Ok(e) = serde_json::from_value::<SerializedSampleEntry>(v) else {
                continue;
            };
            if e.sample_path.is_empty() {
                continue;
            }
            lib.push(DesignedSound {
                id: uuid::Uuid::new_v4().to_string(),
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

/// The page shows: the first sound (or the Kick 808 template) opens; the preview synth starts.
pub fn mount(s: &mut App) {
    ensure_library(s);
    open_default(s);
    let recipe =
        s.p9.designer
            .draft_sound()
            .map(|d| d.recipe)
            .unwrap_or_default();
    crate::sound_preview::mount(s, recipe);
}

fn open_default(s: &mut App) {
    if s.p9.designer.draft.is_none() {
        match all_sounds(s).into_iter().next() {
            Some(first) => s.open_sound(&first.id, false),
            None => s.pick_template(Template::Kick808, false),
        }
    }
}

/// Leaving the page: the draft's edits are saved, the audition and the preview synth stop.
pub fn unmount(s: &mut App) {
    s.flush_sample_drafts();
    s.stop_sound();
    crate::sound_preview::unmount(s);
}

/// A new project was opened / created (after the old one's draft was flushed): the editor
/// starts over on the new project's sounds.
pub fn reset_for_project(s: &mut App) {
    s.stop_sound();
    s.p9.designer.reset_for_project();
    if s.p9.samples_mounted {
        open_default(s);
        sync_synth(s);
    }
}

/// Undo / redo ran: the library follows the project (review finding 4) — an entry the project
/// also has takes the project's version, and a library entry removed by Delete comes back with
/// the undo (and goes again with the redo).
pub fn after_history(s: &mut App) {
    let doc = s.svc.engine.doc();
    let des = &mut s.p9.designer;
    let mut changed = false;
    for l in des.library.iter_mut() {
        if let Some(p) = doc.get_sound(&l.id)
            && p != l
        {
            *l = p.clone();
            changed = true;
        }
    }
    for (id, (at, _)) in &des.trash {
        let in_lib = des.library.iter().any(|l| &l.id == id);
        match doc.get_sound(id) {
            Some(p) if !in_lib => {
                let at = (*at).min(des.library.len());
                des.library.insert(at, p.clone());
                changed = true;
            }
            None if in_lib => {
                des.library.retain(|l| &l.id != id);
                changed = true;
            }
            _ => {}
        }
    }
    if changed {
        des.lib_version += 1;
        let _ = write_sound_library(&mut s.svc.settings.borrow_mut(), &s.p9.designer.library);
    }
}

/// Per frame: the library loads (first frame), renders follow the definitions, the draft's
/// preview, MIDI notes, the audition.
pub fn poll(s: &mut App, now: f64, shown: bool) -> bool {
    ensure_library(s);
    let mut changed = sync_store(s);
    changed |= apply_results(s);
    if shown {
        if s.p9.designer.draft.is_none() {
            open_default(s);
            changed = true;
        }
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
    if let Some((_, until)) = d.confirm_delete
        && now >= until
    {
        d.confirm_delete = None;
        changed = true;
    }
    if d.polled_version != d.version {
        d.polled_version = d.version;
        changed = true;
    }
    changed
}

/// Ids that Song regions load from a file. A designed sound is never registered under one
/// (it would replace the file's audio those regions play; review finding 1).
fn file_region_ids(s: &App) -> HashSet<String> {
    s.svc
        .engine
        .doc()
        .get_tracks()
        .iter()
        .flat_map(|t| t.regions.iter())
        .filter_map(|r| r.audio.as_ref())
        .filter(|a| !a.sample_path.is_empty())
        .map(|a| a.sample_id.clone())
        .collect()
}

/// Pads playing a sound the project doesn't define get it from the library (a kit loaded or a
/// pad pasted from another project; review finding 7). Sounds the library doesn't have either
/// are reported once (the shell's warning card).
fn adopt_missing(s: &mut App) -> bool {
    let missing = s.svc.engine.doc().missing_sounds();
    if missing.is_empty() {
        return false;
    }
    let mut found = Vec::new();
    for id in missing {
        match s.p9.designer.library.iter().find(|l| l.id == id) {
            Some(l) => found.push(l.clone()),
            None => {
                if s.p9.designer.warned.insert(id.clone()) {
                    let name = pad_name_of(s, &id).unwrap_or_else(|| "A sound".to_owned());
                    s.p9.designer.missing.push(name);
                }
            }
        }
    }
    if found.is_empty() {
        return true;
    }
    s.svc.engine.edit(|d| d.adopt_sounds(&found));
    true
}

fn pad_name_of(s: &App, id: &str) -> Option<String> {
    let doc = s.svc.engine.doc();
    doc.get_tracks()
        .iter()
        .filter_map(|t| t.instrument.kit.as_ref())
        .flat_map(|k| k.pads.iter())
        .filter_map(|p| p.sample.as_ref())
        .find(|smp| matches!(&smp.source, PadSampleSource::Sound { sound_id } if sound_id == id))
        .map(|smp| smp.name.clone())
}

/// Starts a render of `d` on the sample worker pool (inline in headless runs). Sample-based
/// sounds decode their file there too (once: the UI thread caches it).
fn submit(s: &mut App, target: Target, d: DesignedSound) -> u64 {
    let des = &mut s.p9.designer;
    des.next_rev += 1;
    let rev = des.next_rev;
    if target == Target::Preview {
        des.preview_renders += 1;
    }
    let cached = d
        .sample
        .as_ref()
        .and_then(|e| des.decoded.get(&e.path).cloned());
    let tx = des.tx.clone();
    let rate = s.p9.samples.rate();
    let decoder = s.svc.engine.cache().clone();
    let waker = s.p9.samples.waker.clone();
    s.p9.samples.store.pool.run(move || {
        let mut decoded = None;
        let result = match &d.sample {
            Some(e) => {
                let src = match cached {
                    Some(b) => Ok(b),
                    None => decoder
                        .decode_file_direct(&e.path)
                        .inspect(|b| decoded = Some((e.path.clone(), b.clone()))),
                };
                src.map_err(|e| e.to_string())
                    .and_then(|b| render_sound(&d, rate, Some(&b)).ok_or_else(String::new))
            }
            None => render_sound(&d, rate, None).ok_or_else(String::new),
        }
        .map(Arc::new);
        let _ = tx.send(Done {
            target,
            rev,
            sound: d,
            result,
            decoded,
        });
        if let Some(w) = waker {
            w();
        }
    });
    rev
}

/// Registers a project sound's render: the store serves it to pads and Song regions, and the
/// pads playing it reload.
fn register(s: &mut App, d: &DesignedSound, buf: SharedBuffer) {
    let des = &mut s.p9.designer;
    des.lengths.insert(
        d.id.clone(),
        buf.frames() as f64 / buf.sample_rate.max(1) as f64,
    );
    des.rendered.insert(d.id.clone(), d.clone());
    des.failed.remove(&d.id);
    des.pending.remove(&d.id);
    s.p9.samples.put_sound_shared(&d.id, &d.name, buf);
    s.svc.engine.cache().invalidate_sound(&d.id);
}

/// Applies finished renders: only the latest of each sound (and of the preview) is used.
fn apply_results(s: &mut App) -> bool {
    let mut changed = false;
    while let Ok(done) = s.p9.designer.rx.try_recv() {
        if let Some((path, b)) = done.decoded {
            s.p9.designer.cache_decoded(path, b);
        }
        let now = s.now;
        match done.target {
            Target::Store => {
                let id = done.sound.id.clone();
                if s.p9.designer.pending.get(&id).map(|p| p.0) != Some(done.rev) {
                    continue; // superseded or deleted meanwhile
                }
                s.p9.designer.pending.remove(&id);
                match done.result {
                    Ok(buf) if !file_region_ids(s).contains(&id) => {
                        register(s, &done.sound, buf);
                    }
                    Ok(_) => {}
                    Err(_) => {
                        // Not marked rendered: tried again later (a drive reconnected...).
                        s.p9.designer
                            .failed
                            .insert(id, (done.sound, now + RENDER_RETRY));
                    }
                }
                changed = true;
            }
            Target::Preview => {
                let des = &mut s.p9.designer;
                if des.preview_job.as_ref().map(|p| p.0) != Some(done.rev) {
                    continue;
                }
                des.preview_job = None;
                match done.result {
                    Ok(buf) => {
                        des.lengths.insert(
                            done.sound.id.clone(),
                            buf.frames() as f64 / buf.sample_rate.max(1) as f64,
                        );
                        des.preview = Some(buf);
                        des.preview_retry = None;
                    }
                    Err(_) => {
                        des.preview = None;
                        des.preview_retry = Some(now + RENDER_RETRY);
                    }
                }
                des.preview_of = Some(done.sound);
                des.version += 1;
                changed = true;
                if des.play_when_ready && des.preview_current() {
                    des.play_when_ready = false;
                    s.play_sound();
                }
            }
        }
    }
    changed
}

/// Starts the renders of the project sounds whose definition changed (save, undo, redo,
/// project open), registers what is ready, forgets sounds that are gone, and lets the editor
/// follow a change to the open sound. Renders run on the worker pool; the UI never waits.
fn sync_store(s: &mut App) -> bool {
    let now = s.now;
    let retry_due = s.p9.designer.failed.values().any(|(_, at)| now >= *at);
    let key = (
        s.svc.engine.doc().state_version(),
        s.p9.designer.lib_version,
    );
    if s.p9.designer.synced == Some(key) && !retry_due {
        return false;
    }
    let mut changed = adopt_missing(s);
    s.p9.designer.synced = Some((
        s.svc.engine.doc().state_version(),
        s.p9.designer.lib_version,
    ));
    let sounds: Vec<DesignedSound> = s.svc.engine.doc().get_sounds().to_vec();
    let protected = file_region_ids(s);
    for d in &sounds {
        if protected.contains(&d.id) {
            continue;
        }
        let des = &s.p9.designer;
        if des.rendered.get(&d.id) == Some(d) {
            continue;
        }
        if des.pending.get(&d.id).is_some_and(|p| &p.1 == d) {
            continue;
        }
        if let Some((f, at)) = des.failed.get(&d.id)
            && f == d
            && now < *at
        {
            continue;
        }
        // The editor already rendered this exact definition (a save): use it.
        if des.preview_of.as_ref() == Some(d)
            && let Some(b) = des.preview.clone()
        {
            register(s, d, b);
            changed = true;
            continue;
        }
        let rev = submit(s, Target::Store, d.clone());
        s.p9.designer.pending.insert(d.id.clone(), (rev, d.clone()));
    }
    let des = &mut s.p9.designer;
    des.pending
        .retain(|id, _| sounds.iter().any(|d| &d.id == id));
    des.failed
        .retain(|id, _| sounds.iter().any(|d| &d.id == id));
    let gone: Vec<String> = des
        .rendered
        .keys()
        .filter(|id| !sounds.iter().any(|d| &d.id == *id))
        .cloned()
        .collect();
    for id in gone {
        s.p9.designer.rendered.remove(&id);
        if !protected.contains(&id) {
            s.p9.samples.remove_sound(&id);
            s.svc.engine.cache().invalidate_sound(&id);
        }
        changed = true;
    }
    changed |= follow_stored(s);
    changed
}

/// Undo / redo (or a reload) changed the open sound's stored version: an editor without
/// unsaved edits shows it (knobs, waveform, preview synth; review finding 4).
fn follow_stored(s: &mut App) -> bool {
    let Some(d) = s.p9.designer.draft.as_ref() else {
        return false;
    };
    if d.dirty {
        return false;
    }
    let Some(st) = stored(s, &d.sound.id) else {
        return false;
    };
    if st == d.sound {
        return false;
    }
    let des = &mut s.p9.designer;
    if let Some(d) = des.draft.as_mut() {
        d.sound = st;
    }
    des.rename = None;
    des.version += 1;
    refresh_preview(s);
    true
}

/// The preview synth plays the draft's recipe.
fn sync_synth(s: &mut App) {
    let Some(r) = s.p9.designer.draft_sound().map(|d| d.recipe) else {
        return;
    };
    if s.p9.designer.synth_recipe != Some(r) {
        s.p9.designer.synth_recipe = Some(r);
        crate::sound_preview::set_recipe(&r);
    }
}

/// The draft's render follows the draft (and the preview synth its recipe at once). One render
/// at a time: edits made while one runs are rendered together when it lands.
fn refresh_preview(s: &mut App) -> bool {
    let Some(d) = s.p9.designer.draft_sound().cloned() else {
        return false;
    };
    sync_synth(s);
    let des = &mut s.p9.designer;
    let retry = des.preview_retry.is_some_and(|at| s.now >= at);
    if des.preview_of.as_ref() == Some(&d) && !retry {
        return false;
    }
    if des.preview_job.is_some() {
        return false;
    }
    des.preview_retry = None;
    let rev = submit(s, Target::Preview, d.clone());
    s.p9.designer.preview_job = Some((rev, d));
    apply_results(s)
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
        if self.p9.designer.preview_of.as_ref() != Some(&sound) {
            // Not the previous sound's waveform while this one renders.
            self.p9.designer.preview = None;
        }
        self.p9.designer.draft = Some(Draft {
            sound,
            dirty: false,
        });
        self.p9.designer.confirm_delete = None;
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
            // Unsaved draft edits are unsaved project changes: the recovery checkpoint takes
            // them (review finding 2).
            self.svc.arm_autosave(self.now);
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
        apply_results(self);
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
        apply_results(self);
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
            apply_results(self);
        }
        // The render: the editor's (it is this exact sound) or the registered one.
        let b = if self.p9.designer.preview_of.as_ref() == Some(&sound) {
            self.p9.designer.preview.clone()
        } else {
            None
        }
        .or_else(|| self.p9.samples.buffer(&sound.id));
        let Some(b) = b else {
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
        if !self.p9.designer.preview_current() && self.p9.designer.preview_job.is_some() {
            // Still rendering: it plays when it lands.
            self.p9.designer.play_when_ready = true;
            return;
        }
        let Some(buf) = self.p9.designer.preview.clone() else {
            return;
        };
        let dur = buf.frames() as f64 / buf.sample_rate.max(1) as f64;
        self.svc.engine.play_audition(buf, 0.0, dur);
        self.p9.designer.audition = Some((self.now, dur));
        self.p9.designer.audition_x.snap([0.0, 0.0]);
    }

    pub fn stop_sound(&mut self) {
        self.p9.designer.play_when_ready = false;
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

    /// Delete needs a second click when the sound is in use (pads, Song) or only in the user
    /// library (review finding 6).
    pub fn delete_needs_confirm(&self) -> bool {
        let Some(id) = self.p9.designer.selected_id() else {
            return false;
        };
        let doc = self.svc.engine.doc();
        let library_only =
            doc.get_sound(id).is_none() && self.p9.designer.library.iter().any(|l| l.id == id);
        library_only || !doc.sound_users(id).is_empty() || !doc.sound_regions(id).is_empty()
    }

    /// The Delete button: deletes, or asks for the confirmation click first.
    pub fn delete_sound_click(&mut self) {
        let Some(id) = self.p9.designer.selected_id().map(str::to_owned) else {
            return;
        };
        let armed = self
            .p9
            .designer
            .confirm_delete
            .as_ref()
            .is_some_and(|(c, _)| *c == id);
        if self.delete_needs_confirm() && !armed {
            self.p9.designer.confirm_delete = Some((id, self.now + CONFIRM_DELETE));
            return;
        }
        self.delete_sound();
    }

    /// Delete: out of the library and the project; pads that played it are cleared and Song
    /// clips playing it are removed. One undo step, which brings back the project's copy, the
    /// pads, the clips **and the library entry**. The next sound opens.
    pub fn delete_sound(&mut self) {
        self.p9.designer.confirm_delete = None;
        let Some(sound) = self.p9.designer.draft_sound().cloned() else {
            return;
        };
        let id = sound.id.clone();
        let sound = stored(self, &id).unwrap_or(sound);
        let list = listed(self);
        let at = list.iter().position(|x| x.id == id).unwrap_or(0);
        self.stop_sound();
        self.p9.designer.rename = None;
        self.svc.engine.edit(|d| d.delete_sound(&sound));
        let des = &mut self.p9.designer;
        if let Some(pos) = des.library.iter().position(|l| l.id == id) {
            let entry = des.library.remove(pos);
            des.trash.insert(id.clone(), (pos, entry));
            des.lib_version += 1;
            let _ = write_sound_library(
                &mut self.svc.settings.borrow_mut(),
                &self.p9.designer.library,
            );
        }
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
            // Read once here, so an unreadable file is reported at once (an explicit import);
            // the renders reuse the decoded audio.
            match self.svc.engine.cache().decode_file_direct(p) {
                Ok(b) => self.p9.designer.cache_decoded(p.clone(), b),
                Err(_) => {
                    failed.push(crate::samples::sample_name_of(p));
                    continue;
                }
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
            .and_then(|e| self.p9.designer.source_secs.get(&e.path).copied())
            .unwrap_or(0.0)
    }
}
