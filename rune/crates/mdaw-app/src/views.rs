//! Phase 9's state and actions: the Samples, Packs, Settings and Developer pages, the command
//! palette, export dialog, template picker, controller setup dialog, synth editor, top-bar
//! instrument selector and the Live FX strip (`App.tsx`'s remaining state and the hooks behind
//! those components). Everything lives in [`Views`] (`App::p9`); the views are in `ui/p9/*`.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use mdaw_engine::fx::live::LiveOp;
use mdaw_midi::monitor::{MidiMonitor, MonitorFilter};
use mdaw_model::controller::{LearnState, PadLearn};
use mdaw_model::doc::KitSelectionSource;
use mdaw_model::packs::model::{
    ClassicMachineInfo, ClassicVoiceInfo as VoiceInfo, PackItem, SoundRef, builtin_item,
    remote_items, sound_key,
};
use mdaw_model::packs::service::{kit_from_installed, pack_file_path};
use mdaw_model::settings as store;
use mdaw_model::types::KnobTarget;
use mdaw_model::types::{AppSettings, Arrangement, KitState, PadSampleSource};
use rune::{KeyEvent, TextEdit, Tx};

use crate::app::{App, AppMode, DawView, Menu};
use crate::kit_preview::{KitPreview, Sounds};
use crate::samples::Samples;

/// A modal dialog (one at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialog {
    Palette,
    Export,
    Templates,
    Controller,
}

/// Phase 9's popovers / selects (`Menu::P9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum P9Menu {
    /// The Sound Designer's "Send to pad" picker.
    SendToPad,
    DevFilter,
    BufferSize,
    LiveFilterType,
    LiveLfoWave,
    PadChannel,
    KnobTarget(usize),
    Instrument,
}

/// `CatalogStatus` of the Packs page (`usePacksController`'s mapping).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogView {
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    Wav,
    Midi,
}

/// The Live FX strip's expandable sections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxSec {
    Lfo,
    Delay,
    Drive,
    Eq,
    Comp,
}

/// A slider being dragged (Settings).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SliderDrag {
    Param(u16),
}

impl ExportUi {
    /// An export that never finishes (the quit test): returns its cancel flag.
    #[cfg(test)]
    pub fn fake_running_for_test(&mut self) -> std::sync::Arc<std::sync::atomic::AtomicBool> {
        let (tx, rx) = channel();
        std::mem::forget(tx);
        let c = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.running = Some(ExportKind::Wav);
        self.rx = Some(rx);
        self.cancel = Some(c.clone());
        c
    }
}

#[derive(Default)]
pub struct ExportUi {
    pub running: Option<ExportKind>,
    pub error: Option<String>,
    /// The last file written (tests, smoke run).
    pub written: Option<PathBuf>,
    rx: Option<Receiver<Result<PathBuf, String>>>,
    /// Set at quit when the export didn't finish in time: the worker writes nothing.
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// The quit cancelled a running export (tests).
    pub cancelled_at_quit: bool,
}

pub struct PacksUi {
    pub query: TextEdit,
    /// `null`, `__installed` or a tag.
    pub tag: Option<String>,
    pub selected: Option<String>,
    /// "Confirm uninstall" armed until (UI clock).
    pub confirm_uninstall: Option<f64>,
    /// `soundErrors` by sound key.
    pub sound_errors: HashMap<String, String>,
    /// Packs whose cancel was asked and whose install has not settled yet ("Cancelling…").
    pub cancelling: HashSet<String>,
    last_preview_error: Option<(String, String)>,
}

/// The command palette's state.
pub struct PaletteUi {
    pub query: TextEdit,
    /// Highlighted row in the shown list (cmdk's selected item).
    pub sel: usize,
    /// The query the order and the selection were last computed for: an edit of the query
    /// resets the selection to the first row (cmdk), a key that is not an edit (arrows) keeps
    /// it.
    pub seen_query: String,
    /// cmdk's DOM order of the items and of the groups (its sort re-appends them on every
    /// search change and keeps that order when the search is cleared).
    pub order: Vec<String>,
    pub group_order: Vec<&'static str>,
    /// The recent projects when the palette opened (its "Recent Projects" group).
    pub recent: Vec<mdaw_model::types::ProjectMeta>,
}

impl PaletteUi {
    pub fn new() -> Self {
        Self::with_recent(&[])
    }

    /// A freshly opened palette: the commands, then the recent projects' group (cmdk's
    /// registration order).
    pub fn with_recent(recent: &[mdaw_model::types::ProjectMeta]) -> Self {
        let cmds: Vec<_> = crate::ui::p9::dialogs::commands()
            .into_iter()
            .chain(crate::ui::p9::dialogs::recent_commands(recent))
            .collect();
        let mut group_order: Vec<&'static str> = Vec::new();
        for c in &cmds {
            if !group_order.contains(&c.group) {
                group_order.push(c.group);
            }
        }
        Self {
            query: TextEdit::new(""),
            sel: 0,
            seen_query: String::new(),
            order: cmds.into_iter().map(|c| c.id).collect(),
            group_order,
            recent: recent.to_vec(),
        }
    }
}

impl Default for PaletteUi {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Views {
    pub samples: Samples,
    /// The Samples page: the Sound Designer.
    pub designer: crate::designer::Designer,
    pub drag: Option<SliderDrag>,
    pub packs: PacksUi,
    pub kit_preview: KitPreview,
    pub monitor: MidiMonitor,
    pub following: bool,
    pub settings: AppSettings,
    pub dialog: Option<Dialog>,
    pub palette: PaletteUi,
    pub export: ExportUi,
    pub template_filter: Option<&'static str>,
    pub synth_editor: bool,
    pub fx_section: Option<FxSec>,
    pub instrument_search: TextEdit,
    /// The selector's highlighted result (`ui::p9::rack::instrument_results`).
    pub instrument_active: usize,
    /// The controller dialog's pad under the pointer (its clear button shows).
    pub ctl_hover: Option<usize>,
    pub vibe: Option<mdaw_engine::synth::presets::PresetVibe>,
    /// The live preset mirror: (preset with edits, its JSON, index, has edits).
    pub preset_cache: std::cell::RefCell<(
        mdaw_engine::synth::presets::SynthPreset,
        serde_json::Value,
        usize,
        bool,
    )>,
    /// The Samples page is mounted (the designer's preview synth and MIDI keys are live).
    samples_mounted: bool,
    /// The Packs page is mounted (`usePacksController`: the catalog is asked for at mount; the
    /// demo groove and sound previews stop at unmount).
    packs_mounted: bool,
    monitor_on: bool,
    pub version: u64,
}

impl Views {
    pub fn new(svc: &mut crate::services::Services, headless: bool) -> Self {
        let preset_cache = {
            let live = svc.engine.live();
            let p = live.current_preset();
            let json = p.to_json();
            (
                p,
                json,
                live.preset_index(),
                !live.synth_overrides().is_empty(),
            )
        };
        // The stored library is restored at the Samples page's first mount (`p9_poll`).
        let samples = Samples::new(
            svc.sample_store.clone(),
            svc.engine.cache().clone(),
            headless,
        );
        let mut kit_preview = KitPreview::default();
        kit_preview.sync = headless;
        Self {
            samples,
            designer: crate::designer::Designer::new(),
            drag: None,
            packs: PacksUi {
                query: TextEdit::new(""),
                tag: None,
                selected: None,
                confirm_uninstall: None,
                sound_errors: HashMap::new(),
                cancelling: HashSet::new(),
                last_preview_error: None,
            },
            kit_preview,
            monitor: MidiMonitor::default(),
            following: true,
            settings: store::read_app_settings(&svc.settings.borrow()),
            dialog: None,
            palette: PaletteUi::new(),
            export: ExportUi::default(),
            template_filter: None,
            synth_editor: false,
            fx_section: None,
            instrument_search: TextEdit::new(""),
            instrument_active: 0,
            ctl_hover: None,
            vibe: None,
            preset_cache: std::cell::RefCell::new(preset_cache),
            samples_mounted: false,
            packs_mounted: false,
            monitor_on: false,
            version: 0,
        }
    }
}

/// The classic machines as `ClassicMachineInfo` (`CLASSIC_MACHINES`).
pub fn machine_infos() -> Vec<ClassicMachineInfo> {
    mdaw_model::classic_meta::CLASSIC_MACHINES
        .iter()
        .map(|m| ClassicMachineInfo {
            id: m.id,
            name: m.name.to_owned(),
            tagline: m.tagline.to_owned(),
            color: m.color.to_owned(),
            description: Some(m.description.to_owned()).filter(|d| !d.is_empty()),
            voices: m
                .voices
                .iter()
                .map(|v| VoiceInfo {
                    id: v.id.to_owned(),
                    label: v.label.to_owned(),
                    gm: Some(v.gm as i64),
                })
                .collect(),
            pads: Some(m.pads.iter().map(|p| (*p).to_owned()).collect()),
        })
        .collect()
}

/// Built-in + downloadable items (`builtins` then `remoteItems(catalog, installed)`).
pub fn pack_items(s: &App) -> (Vec<PackItem>, Vec<PackItem>) {
    let snap = s.svc.packs.snapshot();
    let builtins = machine_infos().iter().map(builtin_item).collect();
    let remotes = remote_items(
        snap.store.catalog.as_ref().map(|c| c.packs.as_slice()),
        &snap.store.installed,
    );
    (builtins, remotes)
}

/// The project templates (`ALL_TEMPLATES`, `src/data/templates/index.ts` order).
pub struct Template {
    pub id: &'static str,
    pub name: String,
    pub description: String,
    pub category: String,
    pub bpm: f64,
    pub track_count: usize,
    pub colors: Vec<String>,
    pub arrangement: Arrangement,
}

pub const TEMPLATE_JSON: [&str; 6] = {
    let f = mdaw_model::templates_data::TEMPLATE_FILES;
    [f[0].1, f[1].1, f[2].1, f[3].1, f[4].1, f[5].1]
};

pub fn templates() -> &'static [Template] {
    static T: std::sync::OnceLock<Vec<Template>> = std::sync::OnceLock::new();
    T.get_or_init(|| {
        TEMPLATE_JSON
            .iter()
            .map(|raw| {
                let v: serde_json::Value = serde_json::from_str(raw).expect("template JSON");
                let arrangement: Arrangement =
                    serde_json::from_value(v["arrangement"].clone()).expect("template");
                Template {
                    id: Box::leak(v["id"].as_str().unwrap_or("").to_owned().into_boxed_str()),
                    name: v["name"].as_str().unwrap_or("").to_owned(),
                    description: v["description"].as_str().unwrap_or("").to_owned(),
                    category: v["category"].as_str().unwrap_or("").to_owned(),
                    bpm: v["bpm"].as_f64().unwrap_or(120.0),
                    track_count: v["trackCount"].as_u64().unwrap_or(0) as usize,
                    colors: arrangement
                        .tracks
                        .iter()
                        .map(|t| t.color.clone())
                        .take(6)
                        .collect(),
                    arrangement,
                }
            })
            .collect()
    })
}

// ------------------------------------------------------------------------------- the app

impl App {
    /// Phase 9's frame work: sample jobs, the Song import, kit previews, the MIDI monitor, the
    /// export worker, pack cancel settling, uninstall confirmation timeout, signal preview.
    /// True when the view must re-run.
    pub fn p9_poll(&mut self, now: f64) -> bool {
        let mut changed = false;
        changed |= self.p9.samples.poll();
        // Audio regions of the project whose samples are not loaded (project restore).
        let regions: Vec<(String, String)> = self
            .svc
            .engine
            .doc()
            .get_tracks()
            .iter()
            .flat_map(|t| t.regions.iter())
            .filter_map(|r| r.audio.as_ref())
            .map(|a| (a.sample_id.clone(), a.sample_path.clone()))
            .collect();
        self.p9.samples.ensure_region_samples(&regions, now);
        let loaded = std::mem::take(&mut self.p9.samples.loaded_ids);
        let failed = std::mem::take(&mut self.p9.samples.region_failures);
        if !failed.is_empty() {
            changed = true;
        }
        self.report_sample_decodes(&loaded, failed);
        if !loaded.is_empty() {
            self.svc.engine.refresh_audio_players(&loaded);
            changed = true;
        }
        for sample in std::mem::take(&mut self.p9.samples.song_imports) {
            self.svc.engine.edit(|d| d.import_audio_sample(&sample));
            changed = true;
        }
        // The Samples page (the Sound Designer): its library loads at the first mount; while
        // it shows, the preview synth plays MIDI keys; leaving it saves a draft being edited.
        let samples_shown = self.started() && self.mode == AppMode::Samples;
        if samples_shown != self.p9.samples_mounted {
            self.p9.samples_mounted = samples_shown;
            self.p9.drag = None;
            if samples_shown {
                crate::designer::mount(self);
            } else {
                crate::designer::unmount(self);
            }
            changed = true;
        }
        changed |= crate::designer::poll(self, now, samples_shown);
        // Slider / trim drags write the settings once, when they end.
        if self.p9.drag.is_none() {
            self.svc.settings.borrow_mut().set_deferred(false);
        }
        // The Packs page mounts: ask for the catalog (`usePacks({ wantCatalog: true })`);
        // unmount: stop the demo groove and any sound preview, dropping pending renders.
        let packs_shown = self.started() && self.mode == AppMode::Packs;
        if packs_shown != self.p9.packs_mounted {
            self.p9.packs_mounted = packs_shown;
            if packs_shown {
                self.svc.packs.load_catalog(false);
            } else {
                self.stop_kit_preview();
                self.p9.kit_preview.stop();
                self.svc.packs.stop_preview(&mut self.svc.engine);
            }
            changed = true;
        }
        // Kit preview.
        if let Some(buf) = self.p9.kit_preview.take_ready(now) {
            self.svc.engine.play_preview(buf);
            changed = true;
        }
        if let Some(at) = self.p9.kit_preview.stop_at
            && now >= at
        {
            self.stop_kit_preview();
            changed = true;
        }
        if self.p9.kit_preview.session.is_some() {
            changed = true;
        }
        // Sound preview errors (`soundErrors`).
        let pe = self.svc.packs.snapshot().preview_error.clone();
        if pe != self.p9.packs.last_preview_error {
            if let Some((k, m)) = &pe {
                self.p9.packs.sound_errors.insert(k.clone(), m.clone());
            }
            self.p9.packs.last_preview_error = pe;
            changed = true;
        }
        // "Cancelling…" until the install settles.
        if !self.p9.packs.cancelling.is_empty() {
            let states = &self.svc.packs.snapshot().store.install_states;
            let before = self.p9.packs.cancelling.len();
            self.p9.packs.cancelling.retain(|id| {
                matches!(
                    states.get(id),
                    Some(mdaw_model::packs::PackInstallState::Downloading { .. })
                )
            });
            changed |= before != self.p9.packs.cancelling.len();
        }
        if let Some(until) = self.p9.packs.confirm_uninstall
            && now >= until
        {
            self.p9.packs.confirm_uninstall = None;
            changed = true;
        }
        // "Get & load" finished: go to the pads while the Packs page is open.
        let loaded = self.svc.packs.take_kit_loaded();
        if !loaded.is_empty() && self.mode == AppMode::Packs {
            self.open_beat();
            changed = true;
        }
        // MIDI monitor (Dev page) and the Generate tab's MIDI keys.
        let want_monitor = self.started() && self.mode == AppMode::Dev;
        if want_monitor != self.p9.monitor_on {
            self.svc.midi.host.set_monitor_active(want_monitor);
            self.p9.monitor_on = want_monitor;
        }
        if want_monitor {
            let batch = self.svc.midi.host.monitor().take();
            if !batch.is_empty() {
                self.p9.monitor.flush(batch);
                changed = true;
            }
        }
        if self.p9.dialog == Some(Dialog::Palette) {
            changed |= crate::ui::p9::dialogs::palette_sync(self);
        }
        // Export worker.
        if let Some(rx) = &self.p9.export.rx
            && let Ok(r) = rx.try_recv()
        {
            match r {
                Ok(p) => self.p9.export.written = Some(p),
                Err(e) => self.p9.export.error = Some(e),
            }
            self.p9.export.running = None;
            self.p9.export.rx = None;
            changed = true;
        }
        changed
    }

    /// Something of phase 9 keeps frames coming.
    pub fn p9_busy(&self) -> bool {
        self.p9.designer.audition.is_some()
            || self.p9.kit_preview.playing.is_some()
            || self.p9.export.running.is_some()
            || self.p9.monitor_on
            || !self.p9.packs.cancelling.is_empty()
    }

    pub fn open_beat(&mut self) {
        self.switch_mode(AppMode::Daw);
        self.view = DawView::Beat;
    }

    // ---------------------------------------------------------------- dialogs

    /// Opens `d`. A different dialog already open is closed first through
    /// [`App::leave_dialog`] (its cleanup runs: the controller dialog cancels a learn); when it
    /// can't close (an export running), nothing opens.
    pub fn open_dialog(&mut self, d: Dialog) {
        if self.p9.dialog.is_some_and(|c| c != d) && !self.leave_dialog() {
            return;
        }
        self.menu = None;
        if d == Dialog::Palette {
            self.p9.palette = PaletteUi::with_recent(&self.project.recent);
        }
        if d == Dialog::Export {
            self.p9.export.error = None;
        }
        self.p9.dialog = Some(d);
    }

    /// The one way a dialog goes away (closed, replaced by another, or a command run from the
    /// palette): the export dialog stays while exporting (false), the controller dialog cancels
    /// a running learn.
    pub fn leave_dialog(&mut self) -> bool {
        match self.p9.dialog {
            Some(Dialog::Export) if self.p9.export.running.is_some() => return false,
            Some(Dialog::Controller) => {
                self.svc.with_router(|r, _| r.mapping.cancel_learn());
            }
            _ => {}
        }
        self.p9.dialog = None;
        true
    }

    /// Closes the open dialog (see [`App::leave_dialog`]).
    pub fn close_dialog(&mut self) {
        let _ = self.leave_dialog();
    }

    pub fn toggle_dialog(&mut self, d: Dialog) {
        if self.p9.dialog == Some(d) {
            self.close_dialog();
        } else {
            self.open_dialog(d);
        }
    }

    // ---------------------------------------------------------------- export

    /// Writes the export of `kind` to `path` on a worker thread (`exportWav` / `exportMidi`).
    /// The file is replaced durably ([`mdaw_model::fsio::write_atomic`]: temp + fsync +
    /// rename), so a failed or interrupted export never truncates an existing file (Electron's
    /// `fs.writeFile` writes in place). A quit waits for it, or cancels it
    /// ([`App::finish_export_for_quit`]).
    pub fn export_to(&mut self, kind: ExportKind, path: PathBuf) {
        let arr = self.svc.engine.doc().get_arrangement().clone();
        let rate = self.svc.engine.sample_rate();
        let cache = self.svc.engine.cache().clone();
        let samples = self.svc.sample_store.clone();
        let (tx, rx) = channel();
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.p9.export.running = Some(kind);
        self.p9.export.error = None;
        self.p9.export.rx = Some(rx);
        self.p9.export.cancel = Some(cancel.clone());
        let waker = self.waker.clone();
        let job = move || {
            let bytes = match kind {
                ExportKind::Wav => mdaw_engine::engine::export_wav(&arr, rate, &cache, &*samples)
                    .map_err(|e| e.to_string()),
                ExportKind::Midi => {
                    mdaw_engine::export::midi_export(&arr).map_err(|e| e.to_string())
                }
            };
            let r = bytes.and_then(|b| {
                if cancel.load(std::sync::atomic::Ordering::Acquire) {
                    return Err("cancelled".to_owned());
                }
                mdaw_model::fsio::write_atomic(&path, &b)
                    .map(|_| path.clone())
                    .map_err(|e| e.to_string())
            });
            let _ = tx.send(r);
            if let Some(w) = waker {
                w();
            }
        };
        if self.svc.mode == crate::services::RunMode::Headless {
            job();
        } else {
            std::thread::spawn(job);
        }
    }

    /// The quit's export handling: a running export gets up to `wait` to finish (its file is
    /// then complete); after that it is cancelled (the render's result is dropped, nothing is
    /// written, the destination keeps its old content).
    pub fn finish_export_for_quit(&mut self, wait: std::time::Duration) {
        let Some(rx) = self.p9.export.rx.take() else {
            return;
        };
        match rx.recv_timeout(wait) {
            Ok(Ok(p)) => self.p9.export.written = Some(p),
            Ok(Err(e)) => self.p9.export.error = Some(e),
            Err(_) => {
                if let Some(c) = &self.p9.export.cancel {
                    c.store(true, std::sync::atomic::Ordering::Release);
                }
                self.p9.export.cancelled_at_quit = true;
            }
        }
        self.p9.export.running = None;
    }

    /// The dialog's buttons: a save dialog named `{name||'untitled'}.{ext}`, then the export.
    pub fn export_pick(&mut self, kind: ExportKind, tx: &mut Tx) {
        let name = {
            let n = self.svc.engine.doc().get_arrangement().name.clone();
            if n.is_empty() {
                "untitled".to_owned()
            } else {
                n
            }
        };
        let (ext, label) = match kind {
            ExportKind::Wav => ("wav", "WAV audio"),
            ExportKind::Midi => ("mid", "MIDI file"),
        };
        let options = rune::FileDialogOptions {
            title: "Export".into(),
            directory: dirs::download_dir(),
            file_name: Some(format!("{name}.{ext}")),
            filters: vec![(label.into(), vec![ext.into()])],
        };
        tx.save_file(
            options,
            move |s: &mut App, r: rune::FileDialogResult, _tx: &mut Tx| {
                if let Ok(Some(p)) = r {
                    s.export_to(kind, p);
                }
            },
        );
    }

    // ---------------------------------------------------------------- templates

    /// `project.newProjectFromTemplate(template.arrangement)`: the unsaved prompt, the save
    /// dialog, then the template (phase 10's project service, `crate::project`).
    pub fn new_from_template(&mut self, index: usize, tx: &mut Tx) {
        if templates().get(index).is_none() {
            return;
        }
        self.close_dialog();
        self.request_project_op(crate::project::ProjectOp::Template(index), tx);
    }

    // ---------------------------------------------------------------- audio import

    /// `importAudioFile()` (⌘I, the Song toolbar's Audio): pick a file, then a track per file.
    pub fn import_audio_pick(&mut self, tx: &mut Tx) {
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
                    s.p9.samples
                        .import_for_song(&[p.to_string_lossy().into_owned()]);
                }
            },
        );
    }

    // ---------------------------------------------------------------- packs

    pub fn pack_item(&self, id: &str) -> Option<PackItem> {
        let (b, r) = pack_items(self);
        b.into_iter().chain(r).find(|i| i.id() == id)
    }

    /// `kitFor(item, kitId)`.
    pub fn kit_for(&self, item: &PackItem, kit_id: &str) -> Option<KitState> {
        match item {
            PackItem::Builtin { machine, .. } => {
                Some(mdaw_model::kit::create_classic_kit(machine.id))
            }
            PackItem::Remote { id, .. } => {
                let snap = self.svc.packs.snapshot();
                let inst = snap.store.installed.iter().find(|p| &p.id == id)?;
                kit_from_installed(inst, kit_id)
            }
        }
    }

    pub fn select_pack(&mut self, id: &str) {
        if let Some(p) = &self.p9.kit_preview.playing
            && !p.starts_with(&format!("{id}/"))
        {
            self.stop_kit_preview();
        }
        self.p9.packs.confirm_uninstall = None;
        self.p9.packs.selected = if self.p9.packs.selected.as_deref() == Some(id) {
            None
        } else {
            Some(id.to_owned())
        };
    }

    pub fn close_pack_detail(&mut self) {
        self.stop_kit_preview();
        self.p9.packs.selected = None;
        self.p9.packs.confirm_uninstall = None;
    }

    pub fn stop_kit_preview(&mut self) {
        if self.p9.kit_preview.stop() {
            self.svc.engine.stop_preview();
        }
    }

    pub fn pack_cancel(&mut self, id: &str) {
        self.svc.packs.cancel(id);
        self.p9.packs.cancelling.insert(id.to_owned());
    }

    /// `previewKit(item, kitId)`.
    pub fn preview_kit(&mut self, item_id: &str, kit_id: &str) {
        let Some(item) = self.pack_item(item_id) else {
            return;
        };
        let key = format!("{item_id}/{kit_id}");
        self.svc.packs.stop_preview(&mut self.svc.engine);
        let (kit, sounds) = match self.kit_for(&item, kit_id) {
            Some(k) => (k, Sounds::Kit),
            None => match &item {
                PackItem::Remote { manifest, .. } => {
                    let Some(plan) =
                        mdaw_model::packs::kit_preview::remote_kit_plan(manifest, kit_id)
                    else {
                        return;
                    };
                    (plan.kit, Sounds::Remote(plan.urls))
                }
                PackItem::Builtin { .. } => return,
            },
        };
        let cache = self.svc.engine.cache().clone();
        let store = self.svc.packs.store().clone();
        self.p9.kit_preview.play(&key, kit, sounds, cache, store);
    }

    /// `playSound(item, sound)`.
    pub fn play_pack_sound(&mut self, item_id: &str, voice_or_path: &str) {
        let Some(item) = self.pack_item(item_id) else {
            return;
        };
        self.stop_kit_preview();
        match &item {
            PackItem::Builtin { machine, .. } => {
                let key = sound_key(item_id, &SoundRef::Voice(voice_or_path));
                self.p9.packs.sound_errors.remove(&key);
                self.svc.packs.preview_sound(
                    PadSampleSource::Classic {
                        machine: machine.id,
                        voice: voice_or_path.to_owned(),
                    },
                    &key,
                    &mut self.svc.engine,
                );
            }
            PackItem::Remote { manifest, .. } => {
                let Some(file) = manifest.files.iter().find(|f| f.path == voice_or_path) else {
                    return;
                };
                let key = sound_key(item_id, &SoundRef::File(file));
                self.p9.packs.sound_errors.remove(&key);
                let inst = self
                    .svc
                    .packs
                    .snapshot()
                    .store
                    .installed
                    .iter()
                    .find(|p| p.id == item_id)
                    .map(|p| p.dir.clone());
                match inst {
                    Some(dir) => {
                        let path = pack_file_path(&dir, &file.path);
                        self.svc
                            .packs
                            .preview_file(&path, &key, &mut self.svc.engine);
                    }
                    None => {
                        let url = file.url.clone();
                        self.svc
                            .packs
                            .preview_remote(&url, &key, &mut self.svc.engine);
                    }
                }
            }
        }
    }

    /// "Load on pads": stop the preview, drop a pending Get & load, put the kit on the active
    /// drum track (created when missing), go to the Beat view.
    pub fn load_pack_kit(&mut self, item_id: &str, kit_id: &str) {
        let Some(item) = self.pack_item(item_id) else {
            return;
        };
        let Some(kit) = self.kit_for(&item, kit_id) else {
            return;
        };
        self.stop_kit_preview();
        self.svc.packs.cancel_pending_load();
        self.svc.engine.edit(|d| {
            let t = d.ensure_drum_track();
            d.set_kit(&t, &kit, KitSelectionSource::User);
        });
        self.open_beat();
    }

    pub fn get_and_load(&mut self, item_id: &str, kit_id: &str) {
        self.stop_kit_preview();
        self.svc
            .packs
            .get_and_load(item_id, kit_id, &mut self.svc.engine);
    }

    /// The Uninstall button: first click arms "Confirm uninstall" (4 s, 9 s when the project
    /// uses the pack), the second uninstalls.
    pub fn uninstall_click(&mut self, id: &str, used: bool) {
        if self.p9.packs.confirm_uninstall.is_none() {
            self.svc.packs.dismiss_error(id);
            self.p9.packs.confirm_uninstall = Some(self.now + if used { 9.0 } else { 4.0 });
            return;
        }
        self.p9.packs.confirm_uninstall = None;
        self.stop_kit_preview();
        self.svc.packs.uninstall(id);
    }

    /// `usedByProject(id)`.
    pub fn pack_used_by_project(&self, id: &str) -> bool {
        let doc = self.svc.engine.doc();
        let kits: Vec<Option<&KitState>> = doc
            .get_tracks()
            .iter()
            .map(|t| t.instrument.kit.as_ref())
            .collect();
        let dir = self
            .svc
            .packs
            .snapshot()
            .store
            .installed
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.dir.clone());
        mdaw_model::packs::model::kits_use_pack(&kits, id, dir.as_deref())
    }

    /// Escape on the Packs page closes the detail (not from the search field).
    pub fn packs_escape(&mut self, focused: Option<&str>) -> bool {
        if self.mode != AppMode::Packs || self.p9.packs.selected.is_none() {
            return false;
        }
        if focused.is_some_and(|f| f.ends_with("packs-search")) || self.p9.dialog.is_some() {
            return false;
        }
        self.close_pack_detail();
        true
    }

    // ---------------------------------------------------------------- settings

    /// `updateSettings(partial)`.
    pub fn update_settings(&mut self, partial: serde_json::Value) {
        let mut st = self.svc.settings.borrow_mut();
        let _ = store::update_app_settings(&mut st, &partial);
        self.p9.settings = store::read_app_settings(&st);
    }

    // ---------------------------------------------------------------- live FX / synth

    pub fn live_fx_op(&mut self, op: LiveOp) {
        self.svc.engine.live_fx(op);
    }

    /// `audio.changePreset(i)` from the instrument selector / synth editor / palette.
    pub fn change_preset(&mut self, index: usize) {
        self.svc.engine.live().set_preset(index as f64);
        self.preset = crate::app::preset_label(&mut self.svc);
        self.refresh_preset_cache();
    }

    /// `audio.setSynthParam(path, value)`.
    pub fn set_synth_param(&mut self, path: &str, value: serde_json::Value) {
        self.svc.engine.live().set_synth_param(path, value);
        self.preset = crate::app::preset_label(&mut self.svc);
        self.refresh_preset_cache();
    }

    pub fn palette_toggle_play(&mut self) {
        if self.view == DawView::Beat {
            self.toggle_play();
        } else {
            self.song_toggle_play();
        }
    }

    pub fn palette_toggle_metronome(&mut self) {
        if self.view == DawView::Beat {
            self.toggle_loop_metronome();
        } else {
            self.toggle_song_metronome();
        }
    }

    /// "Record to Track (Song)": stop a running Song recording, else start one.
    pub fn palette_song_record(&mut self) {
        self.song_r_key();
    }

    pub fn palette_marker(&mut self, next: bool) {
        if next {
            self.seek_next_marker();
        } else {
            self.seek_previous_marker();
        }
    }

    // ---------------------------------------------------------------- controller

    pub fn controller_learn_pads(&mut self) {
        self.svc
            .with_router(|r, _| r.mapping.start_pad_learn(PadLearn::Sequence));
    }

    pub fn controller_learn_pad(&mut self, pad: usize) {
        self.svc
            .with_router(|r, _| r.mapping.start_pad_learn(PadLearn::Pad(pad as i64)));
    }

    pub fn controller_learn_knob(&mut self, target: KnobTarget, index: Option<i64>) {
        self.svc
            .with_router(|r, _| r.mapping.start_knob_learn(target, index));
    }

    pub fn controller_learn_state(&self) -> LearnState {
        self.svc.with_router(|r, _| r.mapping.get_learn_state())
    }

    /// The controller dialog's keys: Esc cancels a learn first, then closes; Backspace goes
    /// back one pad in the wizard.
    pub fn controller_key(&mut self, ev: &KeyEvent) -> bool {
        match ev.key {
            rune::Key::Escape => {
                if self.svc.with_router(|r, _| r.mapping.is_learning()) {
                    self.svc.with_router(|r, _| r.mapping.cancel_learn());
                } else {
                    self.close_dialog();
                }
                true
            }
            rune::Key::Backspace => {
                if let LearnState::Pad { sequence: true, .. } = self.controller_learn_state() {
                    self.svc.with_router(|r, _| r.mapping.previous_pad());
                    return true;
                }
                false
            }
            _ => false,
        }
    }

    // ---------------------------------------------------------------- dev

    pub fn monitor_set_filter(&mut self, f: MonitorFilter) {
        self.p9.monitor.filter = f;
        self.menu = None;
    }

    pub fn monitor_toggle_pause(&mut self) {
        let p = !self.p9.monitor.paused;
        self.p9.monitor.paused = p;
        self.svc.midi.host.monitor().set_paused(p);
    }

    pub fn monitor_clear(&mut self) {
        self.p9.monitor.clear();
        self.svc.midi.host.monitor().clear();
    }

    pub fn toggle_p9_menu(&mut self, m: P9Menu) {
        self.toggle_menu(Menu::P9(m));
    }
}

/// Phase 9's text fields (they keep their keys).
pub fn text_field(leaf: &str) -> bool {
    matches!(
        leaf,
        "designer-rename" | "designer-search" | "packs-search" | "palette-input" | "sound-search"
    )
}

/// Phase 9's keys, before the app's shortcuts: an open dialog owns the keyboard (Escape closes
/// it; the controller dialog's Escape cancels a learn first and Backspace steps back), ⌘K
/// toggles the palette, ⌘E the export dialog, ⌘I imports audio; Escape closes the Packs page's
/// detail panel.
pub fn keys(s: &mut App, ev: &KeyEvent, tx: &mut Tx, focused: Option<&str>) -> bool {
    let m = ev.mods.cmd;
    if m {
        match ev.key {
            rune::Key::Command('k') => {
                s.toggle_dialog(Dialog::Palette);
                return true;
            }
            rune::Key::Command('e') if s.p9.dialog.is_none_or(|d| d == Dialog::Export) => {
                s.toggle_dialog(Dialog::Export);
                return true;
            }
            rune::Key::Command('i') if s.p9.dialog.is_none() => {
                s.import_audio_pick(tx);
                return true;
            }
            _ => {}
        }
    }
    if let Some(d) = s.p9.dialog {
        if d == Dialog::Controller && s.controller_key(ev) {
            return true;
        }
        if matches!(ev.key, rune::Key::Escape) {
            s.close_dialog();
            return true;
        }
        // A modal is open: the DAW shortcuts are skipped (`[role=dialog][data-state=open]`).
        return !m;
    }
    if matches!(ev.key, rune::Key::Escape) && s.menu.is_none() && s.packs_escape(focused) {
        return true;
    }
    // The sound name field: Escape cancels the rename.
    if matches!(ev.key, rune::Key::Escape)
        && s.mode == AppMode::Samples
        && s.p9.designer.rename.is_some()
    {
        s.p9.designer.rename = None;
        return true;
    }
    false
}
