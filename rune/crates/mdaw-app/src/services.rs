//! The app's services (≙ the Electron singletons `arrangementEngine`, `audioEngine` /
//! `drumEngine`, `midiService` + `useMidiNotes`, `packService`, `kitStore`, localStorage), and
//! the loops the phase-5 / phase-6 docs say the app must run:
//!
//! - **Audio**: the engine core runs in the cpal callback ([`Driver::Device`], through
//!   `mdaw-audio-host`'s `OutputHost`, which falls back to a real-time null pacer when no device
//!   opens), or is rendered by the UI's own clock ([`Driver::Offline`]: headless snapshots and
//!   tests, so a test's virtual seconds are the engine's seconds).
//! - **MIDI** (`docs/rune-port/phase-5-midi.md`): `notify::ensure_started` first (done in
//!   `main`), then `build_with_settings` + `settings::enable_with_settings`; every frame / wake
//!   `MidiUi::pump`, `MidiHost::poll`, `ProfileSaves::apply`. The on-screen and computer-key
//!   pads go through the same router (`drumPress` / `drumRelease` / `drumHit`), so they are heard,
//!   captured, flashed and note-repeated exactly like controller pads.
//! - **Packs** (`docs/rune-port/phase-6-packs.md`): one `PackService`, `init()` at start,
//!   `poll(&mut engine)` every frame, `take_kit_selected` → `on_kit_selected`, `check_target`
//!   after engine edits. `pause_for_quit` runs from the window's close / quit hooks
//!   ([`crate::App::shutdown`]), with the autosave flush.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mdaw_audio_host::{BlockTiming, OutputHost, OutputOptions, shared_render};
use mdaw_engine::buffers::PadBufferCache;
use mdaw_engine::core::{BlockClock, Core};
use mdaw_engine::engine::ArrangementEngine;
use mdaw_midi::{DeferredProfileStore, MidiHost, MidiUi, ProfileSaves, Router};
use mdaw_model::doc::ArrangementDoc;
use mdaw_model::settings::SettingsStore;
use mdaw_packs::http::{Check, Conn, Connector, HttpError};
use mdaw_packs::{PackService, PackStoreOptions};

/// The largest absolute sample the device callback produced (f32 bits; non-negative floats
/// order like their bits). Read by the `MDAW_SMOKE` check.
pub static OUTPUT_PEAK: std::sync::LazyLock<Arc<std::sync::atomic::AtomicU32>> =
    std::sync::LazyLock::new(|| Arc::new(std::sync::atomic::AtomicU32::new(0)));

/// Engine sample rate for offline runs (tests, snapshots).
pub const OFFLINE_RATE: u32 = 48_000;

/// Where the engine core runs.
pub enum Driver {
    /// The cpal output host owns the core (moved into its render callback).
    Device(OutputHost),
    /// The UI renders the core itself up to the UI clock (virtual in headless runs).
    Offline {
        core: Box<Core>,
        /// Seconds of audio rendered.
        rendered: f64,
        /// UI clock at the first poll (renders follow the clock from there).
        origin: Option<f64>,
    },
}

/// How the app was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// A window: real audio device, MIDI, settings and packs on disk.
    Window,
    /// `--snap` / `--tree` / `--script`, or a test: offline audio, no MIDI ports, in-memory
    /// settings, no network.
    Headless,
}

/// The MIDI host + its UI half.
pub struct Midi {
    pub host: MidiHost<DeferredProfileStore>,
    pub ui: MidiUi,
    pub saves: ProfileSaves,
}

pub struct Services {
    pub mode: RunMode,
    pub engine: ArrangementEngine,
    pub driver: Driver,
    pub midi: Midi,
    pub packs: PackService,
    /// localStorage (`micro-daw:*` keys): shared with the MIDI host's last-device persister.
    pub settings: Rc<RefCell<SettingsStore>>,
    started_at: Instant,
    saved_version: u64,
    save_due: Option<f64>,
    /// Headless runs normally keep the in-memory settings untouched; tests turn saving on.
    pub save_headless: bool,
    /// On-screen / computer-key pad presses not released yet (pad → count).
    held: RefCell<std::collections::BTreeMap<usize, u32>>,
    /// The decoded samples the audio regions play (phase 9's sample manager fills it).
    pub sample_store: Arc<crate::samples::SampleStore>,
}

/// A connector that never connects: headless runs use the bundled catalog and never touch the
/// network (a snapshot or a test must not depend on it).
struct OfflineConnector;

impl Connector for OfflineConnector {
    fn connect(
        &self,
        _url: &url::Url,
        _timeout: Duration,
        _check: Check,
    ) -> Result<Box<dyn Conn>, HttpError> {
        Err(HttpError::Network("fetch failed".into()))
    }
}

/// The app's data folder: `$MDAW_DATA_DIR`, else `<platform data dir>/micro-daw-rune` (the Rust
/// app keeps its own folder next to Electron's `micro-daw` userData; importing the Electron
/// profile is a phase-10 item).
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MDAW_DATA_DIR") {
        return PathBuf::from(d);
    }
    dirs::data_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("micro-daw-rune")
}

fn temp_packs_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mdaw-app-packs-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

impl Services {
    /// Builds every service. `Window`: the default output device (null pacer fallback), MIDI
    /// enabled, settings and packs in [`data_dir`]. `Headless`: see [`RunMode::Headless`].
    pub fn new(mode: RunMode, settings: SettingsStore) -> Self {
        let (packs_dir, connector): (PathBuf, Arc<dyn Connector>) = match mode {
            RunMode::Window => (
                data_dir().join("packs"),
                Arc::new(mdaw_packs::http::TlsConnector::new()),
            ),
            RunMode::Headless => (temp_packs_dir(), Arc::new(OfflineConnector)),
        };
        let packs_dir_str = packs_dir.to_string_lossy().into_owned();
        let mut packs = PackService::new(
            PackStoreOptions::with_connector(&packs_dir, connector),
            settings.get_item(mdaw_model::settings::PACK_NAMES_KEY),
        );
        let settings = Rc::new(RefCell::new(settings));
        let names = settings.clone();
        packs.set_name_persister(move |raw| {
            let _ = names
                .borrow_mut()
                .set_item(mdaw_model::settings::PACK_NAMES_KEY, raw);
        });

        let sample_store = Arc::new(crate::samples::SampleStore::default());
        let (driver, engine, live) = match mode {
            RunMode::Window => Self::device_engine(&packs_dir_str, sample_store.clone()),
            RunMode::Headless => {
                let cache = PadBufferCache::with_fs(OFFLINE_RATE, Some(packs_dir_str.clone()));
                let (engine, core, live) = ArrangementEngine::with_doc(
                    ArrangementDoc::new(),
                    OFFLINE_RATE,
                    cache,
                    sample_store.clone(),
                );
                let driver = Driver::Offline {
                    core: Box::new(core),
                    rendered: 0.0,
                    origin: None,
                };
                (driver, engine, live)
            }
        };
        let mut engine = engine;
        engine.edit(|d| d.set_packs_dir(Some(packs_dir_str.clone())));
        // `useArrangementPersistence`: the saved project, else the starter (one Drums track).
        let raw = settings
            .borrow()
            .get_item(mdaw_model::settings::ARRANGEMENT_STORAGE_KEY)
            .map(str::to_owned);
        let loaded =
            engine.edit(|d| matches!(d.load_from_local_storage(raw.as_deref()), Ok(Some(_))));
        if !loaded {
            engine.edit(|d| d.reset_to_starter());
        }

        let saved_version = engine.doc().state_version();
        let started_at = Instant::now();
        let clock = move || started_at.elapsed().as_secs_f64() * 1000.0;
        let (host, ui, saves) =
            mdaw_midi::build_with_settings(&engine, live, &settings.borrow(), clock);
        packs.init();
        Self {
            mode,
            engine,
            driver,
            midi: Midi { host, ui, saves },
            packs,
            settings,
            started_at,
            saved_version,
            save_due: None,
            save_headless: false,
            held: RefCell::new(Default::default()),
            sample_store,
        }
    }

    fn device_engine(
        packs_dir: &str,
        samples: Arc<crate::samples::SampleStore>,
    ) -> (Driver, ArrangementEngine, mdaw_engine::core::LiveInput) {
        // The context rate is the default device's rate (like a Web Audio context); the host
        // resamples when the device runs at another rate later.
        let rate = mdaw_audio_host::default_output_device_info()
            .map(|d| d.default_sample_rate)
            .filter(|r| *r > 0)
            .unwrap_or(mdaw_audio_host::DEFAULT_CONTEXT_RATE);
        let cache = PadBufferCache::with_fs(rate, Some(packs_dir.to_owned()));
        let (engine, core, live) =
            ArrangementEngine::with_doc(ArrangementDoc::new(), rate, cache, samples);
        let mut core = core;
        let peak = OUTPUT_PEAK.clone();
        let render = shared_render(move |l: &mut [f32], r: &mut [f32], t: &BlockTiming| {
            core.process_timed(
                l,
                r,
                BlockClock {
                    instant: t.chunk_instant(),
                    output_latency: t.output_latency,
                },
            );
            // Output peak (smoke check): lock- and allocation-free.
            let m = l.iter().chain(r.iter()).fold(0f32, |m, v| m.max(v.abs()));
            peak.fetch_max(m.to_bits(), std::sync::atomic::Ordering::Relaxed);
        });
        let options = OutputOptions {
            context_rate: Some(rate),
            ..OutputOptions::default()
        };
        match OutputHost::start(options, render) {
            Ok(host) => (Driver::Device(host), engine, live),
            Err(e) => panic!("audio output could not start: {e}"),
        }
    }

    /// Milliseconds since start (MIDI activity / knob gesture clock).
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// `handleStart`: `audio.start()`, `drumEngine.start()`, `midi.enable()`. The engine is
    /// already running (the core renders from launch, silent); this enables MIDI input (window
    /// runs only) and the live synth. Returns false when MIDI could not be enabled.
    pub fn start_session(&mut self) -> bool {
        self.engine.live().start();
        if self.mode == RunMode::Headless {
            return true;
        }
        mdaw_midi::settings::enable_with_settings(&mut self.midi.host, self.settings.clone())
    }

    /// Renders the offline core up to UI time `now` (seconds), then runs the engine's frame.
    /// The device driver renders on its own thread; this only ticks the engine.
    pub fn advance(&mut self, now: f64) {
        if let Driver::Offline {
            core,
            rendered,
            origin,
        } = &mut self.driver
        {
            let origin = *origin.get_or_insert(now);
            let target = (now - origin).max(0.0);
            let quantum = 128.0 / OFFLINE_RATE as f64;
            let (mut l, mut r) = ([0f32; 128], [0f32; 128]);
            // At most 2 s per frame (a long virtual jump renders in pieces across frames).
            let mut budget = (2.0 / quantum) as usize;
            while *rendered + quantum <= target + 1e-9 && budget > 0 {
                l.fill(0.0);
                r.fill(0.0);
                core.process(&mut l, &mut r);
                *rendered += quantum;
                budget -= 1;
                // Captures are read per quantum, as the UI would between device callbacks.
                self.engine.tick();
            }
        }
        self.engine.tick();
    }

    /// The UI-thread MIDI work of one frame (pump, raw listeners, profile saves).
    pub fn pump_midi(&mut self) -> bool {
        let actions = self.midi.ui.pump(&mut self.engine);
        let polled = self.midi.host.poll();
        let _ = self.midi.saves.apply(&mut self.settings.borrow_mut());
        !actions.is_empty() || polled
    }

    /// The packs work of one frame (worker results, Get & load, kit-selected events).
    pub fn pump_packs(&mut self) -> bool {
        let v = self.packs.version();
        for e in self.engine.take_kit_selected() {
            self.packs.on_kit_selected(&e);
        }
        self.packs.poll(&mut self.engine);
        self.packs.version() != v
    }

    /// Runs `f` on the MIDI router with the engine link as its output (on-screen pads, routing
    /// mode, panic), as the TS hook's callbacks do.
    pub fn with_router<R>(
        &self,
        f: impl FnOnce(&mut Router<DeferredProfileStore>, &mut mdaw_midi::EngineLink) -> R,
    ) -> R {
        self.midi.host.shared().lock().with_router(f)
    }

    /// `drumPress`: on-screen / computer-key pad pressed and held.
    pub fn drum_press(&self, pad: usize, velocity: f64) {
        *self.held.borrow_mut().entry(pad).or_insert(0) += 1;
        self.with_router(|r, out| r.drum_press(pad as f64, velocity, out));
    }

    /// `drumRelease`.
    pub fn drum_release(&self, pad: usize) {
        {
            let mut held = self.held.borrow_mut();
            if let Some(n) = held.get_mut(&pad) {
                *n -= 1;
                if *n == 0 {
                    held.remove(&pad);
                }
            }
        }
        self.with_router(|r, out| r.drum_release(pad as f64, out));
    }

    /// Pads the UI pressed through the router and has not released yet (every press is
    /// released exactly once: a stuck entry is a stuck gate-mode pad / note-repeat).
    pub fn held_pads(&self) -> Vec<usize> {
        self.held.borrow().keys().copied().collect()
    }

    /// `drumHit`: one-shot (the pad editor's audition).
    pub fn drum_hit(&self, pad: usize, velocity: f64) {
        self.with_router(|r, out| r.drum_hit(pad as f64, velocity, out));
    }
}

/// `AUTOSAVE_DEBOUNCE_MS`.
pub const AUTOSAVE_DEBOUNCE: f64 = 0.3;

impl Services {
    /// Autosave (`useArrangementPersistence`): the arrangement is written to
    /// `micro-daw-arrangement`, debounced, after each change (window runs only).
    pub fn autosave(&mut self, now: f64) {
        let v = self.engine.doc().state_version();
        if v != self.saved_version {
            self.saved_version = v;
            self.save_due = Some(now + AUTOSAVE_DEBOUNCE);
        }
        if let Some(due) = self.save_due
            && now >= due
        {
            self.save_due = None;
            self.save_now();
        }
    }

    /// A save is scheduled (the 300 ms debounce runs).
    pub fn save_pending(&self) -> bool {
        self.save_due.is_some() || self.engine.doc().state_version() != self.saved_version
    }

    /// `flush` of `useArrangementPersistence` (unload / unmount): a pending save is written now.
    pub fn flush_autosave(&mut self) {
        if self.save_pending() {
            self.saved_version = self.engine.doc().state_version();
            self.save_due = None;
            self.save_now();
        }
    }

    fn save_now(&mut self) {
        if self.mode == RunMode::Window || self.save_headless {
            let json = self.engine.doc().save_to_local_storage();
            let _ = self
                .settings
                .borrow_mut()
                .set_item(mdaw_model::settings::ARRANGEMENT_STORAGE_KEY, json);
        }
    }
}
