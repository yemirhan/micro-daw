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
//! - [`host`]: midir I/O: enable, enumerate, hot-plug watcher, connect.
//! - [`monitor`]: the Dev view's MIDI monitor formatting and buffers.
//! - [`settings`]: the persisted last device (`micro-daw:midi-last-device`).

pub mod devices;
pub mod engine_link;
pub mod host;
pub mod message;
pub mod monitor;
pub mod router;
pub mod settings;
pub mod timing;

use std::sync::Arc;

use mdaw_engine::core::LiveInput;
use mdaw_engine::engine::ArrangementEngine;
use mdaw_model::controller::{ControllerMapping, ProfileStore};
use parking_lot::Mutex;

pub use devices::{DeviceInfo, LastDevice};
pub use engine_link::{EngineLink, MidiUi};
pub use host::{MidiHost, MidiShared};
pub use router::{Action, Router, RoutingMode};

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
    let (link, rx) = EngineLink::new(live, engine.live_synth_slot());
    let shared = Arc::new(Mutex::new(MidiShared::new(Router::new(mapping), link)));
    (MidiHost::new(shared), MidiUi::new(rx, Box::new(clock_ms)))
}
