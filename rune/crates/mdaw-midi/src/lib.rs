//! micro-daw MIDI input (phase 5 of the Rune port; `docs/rune-port/phase-5-midi.md`).
//!
//! Port of `src/services/MidiService.ts` (devices, hot-plug, held notes, raw listeners),
//! `src/hooks/useMidiNotes.ts` (the routing core), the MIDI half of `useMidiMonitor.ts` and the
//! controller wiring of `installKnobRouting` (`src/services/KnobRouter.ts`). The pure controller
//! profile / learn / knob maths are in `mdaw_model::{controller, knobs}` (phase 1).
//!
//! - [`message`]: raw bytes → note / CC events (webmidi semantics).
//! - [`router`]: the routing core ([`router::Router`]), a pure state machine emitting actions.
//! - [`devices`]: the device-selection state machine and held notes (pure).
//! - [`timing`]: midir timestamps → instants → engine audio time.
//! - [`engine_link`]: router actions → the core (`LiveInput`, MIDI thread) and the UI thread
//!   (captures, knobs, legacy CCs) ([`engine_link::MidiUi`]).
//! - [`host`]: midir I/O: enable, enumerate, hot-plug watcher, connect, connection health.
//! - [`notify`]: CoreMIDI setup notifications on a dedicated run-loop thread.
//! - [`monitor`]: the Dev view's MIDI monitor formatting and buffers.
//! - [`settings`]: the persisted last device, split point and controller profile (written by
//!   the UI thread only).

pub mod devices;
pub mod engine_link;
pub mod host;
pub mod message;
pub mod monitor;
pub mod notify;
pub mod router;
pub mod settings;
pub mod timing;

use std::sync::Arc;

use mdaw_engine::core::LiveInput;
use mdaw_engine::engine::ArrangementEngine;
use mdaw_model::controller::{ControllerMapping, ProfileStore};
use parking_lot::Mutex;

pub use devices::{DeviceInfo, LastDevice};
pub use engine_link::{EngineLink, MidiUi, Waker};
pub use host::{Connection, MidiHost, MidiShared, MidiStats};
pub use router::{Action, Router, RoutingMode};
pub use settings::{DeferredProfileStore, ProfileSaves};

/// Everything the app needs: the shared MIDI state (router + engine link), the UI half, and the
/// host. `live`: the MIDI producer the engine returned; `store`: the controller profile store;
/// `clock_ms`: a monotonic millisecond clock (activity timestamps, knob gestures).
pub fn build<St: ProfileStore + Send + 'static>(
    engine: &ArrangementEngine,
    live: LiveInput,
    store: St,
    clock_ms: impl Fn() -> f64 + Send + Clone + 'static,
) -> (MidiHost<St>, MidiUi) {
    let mapping = ControllerMapping::new(store, Box::new(clock_ms.clone()));
    let (link, rx) = EngineLink::new(live, engine.synth_targets());
    let waker = link.waker().clone();
    let shared = Arc::new(Mutex::new(MidiShared::new(Router::new(mapping), link)));
    (
        MidiHost::new(shared),
        MidiUi::new(rx, waker, Box::new(clock_ms)),
    )
}

/// [`build`] from the app's settings: the controller profile is read from them (and its saves
/// queued for the UI thread: apply them with [`ProfileSaves::apply`]) and the router starts with
/// the stored split point. Enable with [`settings::enable_with_settings`].
pub fn build_with_settings(
    engine: &ArrangementEngine,
    live: LiveInput,
    settings: &mdaw_model::settings::SettingsStore,
    clock_ms: impl Fn() -> f64 + Send + Clone + 'static,
) -> (MidiHost<DeferredProfileStore>, MidiUi, ProfileSaves) {
    let (store, saves) = settings::profile_store(settings);
    let (host, ui) = build(engine, live, store, clock_ms);
    let split = settings::read_split_point(settings);
    host.shared().lock().router.set_split_point(split);
    (host, ui, saves)
}
