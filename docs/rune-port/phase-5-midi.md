# Phase 5: MIDI + controller (`rune/crates/mdaw-midi`) vs the Electron app

**Scope** (plan §4 row 5, inventory [§3 MIDI](inventory/main-packs-midi-project.md#3-midi)):
- `MidiService.ts`: enable, device list, auto-select, `selectDevice`, hot-plug, held-note release, raw listeners.
- `useMidiNotes.ts`: the routing core (auto / keys / split, pad vs key notes, channel rules, velocity, sustain, all-notes-off, learn consumption, CC dispatch, legacy CC fallbacks).
- `installKnobRouting` (`KnobRouter.ts`): knob dispatches wired to the engine (pad params, swing, tempo, track volume, master volume) with soft takeover and undo grouping.
- `useMidiMonitor.ts`: the raw-message formatter and buffers of the Dev view's MIDI monitor.
- One user decision: **`autoConnectLastDevice` is now wired and the preferred device persisted** (Electron gap fixed in Rust, below).
- Review fixes: [`review-phase-5.md`](review-phase-5.md). Two of them are **Electron bugs fixed in both apps (user decision 2026-11-18)**: Split mode, and the sustain pedal on a lost input.

The pure controller logic (`ControllerMapping`, `KnobRouter`, `KnobPickup`) was ported and golden-tested in phase 1 (`mdaw_model::{controller, knobs}`).

## Method

**Design.** The Rust crate keeps the TS state machines as pure code and puts the I/O around them:

| Module | Port of | Notes |
|---|---|---|
| `message.rs` | webmidi 3.3.1 parsing + `velocityToNormalized` | note-on velocity 0 → note-off; velocity `clamp(raw/127)`; channels 1..16; every CC (120–127 included) |
| `router.rs` | `useMidiNotes` | `Router`: one method per hook callback. Every effect is an `Action` emitted in the TS call order |
| `devices.rs` | `MidiService` device logic | `DeviceSelector` returns attach / detach ops; `HeldNotes` = `heldNotes` |
| `timing.rs` | (new) | midir timestamps → `Instant` (windowed minimum) → engine audio time |
| `engine_link.rs` | `installKnobRouting` + the engine calls of the hook | `EngineLink` (MIDI thread → core, outbox retry thread, `Waker`), `MidiUi` (UI thread: knobs, legacy CCs) |
| `host.rs` | `MidiService` I/O | midir enable / enumerate / connect, connection health, hot-plug watcher, raw queue + monitor |
| `notify.rs` | (new) | CoreMIDI setup notifications on a dedicated run-loop thread |
| `monitor.rs` | `useMidiMonitor` | `formatEvent`, buffer, 500 cap, filters, pause |
| `settings.rs` | (new) | persisted last device, split point, controller profile (written by the UI thread only) |

**Threads** (plan §3 "Threading and realtime ownership"; the deviation is below):
- **MIDI thread** (midir's callback). For each message it does a bounded amount of work:
  1. locks `MidiShared` (router + held notes + timestamp mapper);
  2. maps the timestamp to audio time;
  3. runs the router (learn included; a profile save is only queued);
  4. sends engine actions through the MIDI producer's `LiveInput` (a lock-free SPSC ring);
  5. after the lock, copies the raw bytes into a bounded queue (only while a monitor or raw listener wants them) and wakes the UI.

  No disk I/O, no formatting, no user callbacks.
- **Mutex: deliberate deviation from plan §3.** The plan describes a published routing snapshot. Here the router stays behind one mutex shared by the MIDI thread and the UI thread (on-screen keyboard and pads, panic, mode, split point, mute, learn). This keeps the TS's exact interleaving of UI and MIDI calls (learn consumption, held-note routes, panic against a note in flight), which the routing golden checks call by call.
  - Under the lock, the UI only makes router calls. Measured worst case: 71–78 µs (debug build).
  - A UI hold delays MIDI by its length; it never loses an event.
  - The audio thread never takes it.
- **Outbox retry.** `LiveInput` sits behind its own small lock. A `midi-outbox` thread pushes commands left over by a full ring every 1 ms until none are left, so the note-offs of a burst reach the core without further input.
- **Song-recording captures of synth notes** go to the core (`Command::CaptureSynthNote`). The core stamps them at the note's frame with the transport beat and generation, in order with the pad captures and punch closes.
- **UI thread.** The rest goes through a channel to the UI thread. `MidiUi::pump`, called every frame and whenever the coalesced `Waker` fires, applies it:
  - knob dispatches (`KnobRouter`);
  - legacy CCs (the engine's `handle_legacy_cc`, which owns that pickup).

  `MidiHost::poll` runs the raw listeners and the monitor formatting. `ProfileSaves::apply` writes queued profile saves: the UI thread is the single owner of the settings file.
- **Hot-plug.**
  - On macOS, the process's first CoreMIDI client is created on a dedicated `midi-runloop` thread that runs its `CFRunLoop` (`notify.rs`; `MidiHost::enable` and `mdaw-app` start it first). Without it, ports created by other processes never showed up. Its setup notifications flag the watcher and wake the UI at once.
  - A watcher thread also enumerates every **500 ms** (`HOTPLUG_POLL_INTERVAL`). On Linux and Windows that is the only source.
  - When the list changed, `MidiHost::poll` → `handleDeviceChange`.
  - A source removed and re-added between two polls (same list) is reconnected.
- **Connection health.** `MidiHost::connection()` is `Disconnected`, `Connected` or `Failed { error }`, separate from the selection, and is reported to `on_connection_change` listeners. A failed or dead connection is retried on the next hot-plug change or on a reselect. `reconnect()` forces one.

**Engine additions** (`mdaw-engine/src/core`, realtime-safe; covered by the allocation guard):
- `Command::PadHit`: `drumHit`, a one-shot hit with no note repeat.
- `Command::SetLivePadMute`: the hook's `muted`. Pad hits and repeats are still captured, flashed and repeated, but not played.
- `Command::RepeatReleaseAll`: `noteRepeat.releaseAll()` in the panic.
- `LiveInput::synth_note_at`: live synth notes at a MIDI audio time, plus the 4 ms `LIVE_OFFSET`.
- `ControllerMapping`'s clock is now `Send`, so the mapping can live on the MIDI thread.

**Timestamps.** midir stamps messages in µs from a backend-specific origin.
- `TimestampMapper` learns `arrival − timestamp` with a **windowed minimum**: two 2 s buckets, so it follows device-clock drift both ways (−100 ppm over an hour: 0.51 ms error; the lifetime minimum it replaced was 359.7 ms off).
- It re-anchors when the offset jumps by more than 250 ms or after two silent windows, and it is reset on every new connection.
- The resulting instant goes through the core's published anchor (`ClockSnapshot::map_event`): one device block (the largest seen) plus one render quantum after the event.

The **late-event policy**:
- **Within that budget:** constant latency, relative timing kept, no jitter.
- **Delivered later** (a burst the OS held back): played as soon as possible, at the next quantum. The burst therefore loses its spacing. It is counted in `MidiStats` (`late_events`, `max_late_ms`).
- **Device stalled:** the mapping is capped at two periods past the last callback, so nothing is stamped into the future. On a device, the core also applies a MIDI-producer command no later than 0.5 s after it is drained (`MAX_LIVE_LOOKAHEAD`), and the producer rebases a stamp beyond that.

What plays when:
- Pads sound at that time (≙ `drumEngine.hit` at `Tone.immediate()`).
- Synth notes sound 4 ms later (≙ `audioEngine.noteOn` at `immediate() + LIVE_OFFSET`).
- Without an anchor (no device callback yet, offline), a message plays at the next quantum.

## Parity evidence

| Area | Comparison | Result |
|---|---|---|
| Routing core (`useMidiNotes`) | `rune/parity/ts/midi_routing.golden.ts`: the real hook, the real `ControllerMapping` singleton and the real `NoteEventBus`, with audio / drum / arrangement engines and note repeat replaced by call loggers. 47 scenarios: 15 written by hand (pads + keys, mode changes while held, sustain + pedal up, CC 120/121/123, recording captures, mute, re-hit without note-off, channel-filtered pad map, pad learn + wizard, knob learn, mapped + legacy CCs with pickup, volume CC without a handler, and 3 for Split: the chromatic map, moving the split point with held notes and clamping, learn / mute / recording / panic in Split), 24 seeded random streams (60–200 mixed inputs each) and 8 seeded Split streams. Each Rust action is expanded into the TS calls the core makes for it (`PadDown` = `drum.hit` unless muted, capture while recording, flash, `drum` event, `repeat.press`, …). Legacy CCs go through a real `ArrangementEngine::handle_legacy_cc` | **4,253 steps, 6,011 calls identical, in order.** After every step the state also matches: held notes (order and velocity), sustain, sounding notes, mode, split point, learn state, profile, last activity, effect params. The unmount panic matches too |
| Device handling (`MidiService`) | `midi_devices.golden.ts`: the real `MidiService` over a fake webmidi whose input list a script changes. 7 hand-written and 20 random sessions: plug / unplug / replug, select (unknown ids too), notes held across switches and unplugs, the sustain pedal lifted on a lost input, note-on velocity 0, CCs, dispose. Rust feeds raw bytes through `parse` + `HeldNotes` + `DeviceSelector` | **894 steps identical**: active and preferred device, device list, 209 note / pedal callbacks (synthesised note-offs and pedal-ups included, in order), 287 change notifications |
| MIDI monitor (`useMidiMonitor`) | `midi_monitor.golden.ts`: the real hook fed 80 raw messages of every kind (malformed ones too: `undefinedNaN (undefined)`, `CC#undefined`, …), the 50 ms flush, the 6 type filters, the 500-event cap over two flushes, pause | identical (78 events; clock and active sensing dropped) |
| Ported vitest suites | `useMidiNotes.test.ts`, `useMidiNotes.routing.test.ts` (the routing and learn cases), `stuckNotes.test.tsx` (the routing cases), `MidiService.test.ts` (the device cases) as `router_tests.rs` / `devices.rs` tests. The fake synth stacks voices like `fakeAudioEngine.ts`, so a leaked voice fails | 39 unit tests pass |
| End to end through the engine (`tests/engine_e2e.rs`) | Raw MIDI → `MidiShared` → router → `LiveInput` → the real core, rendered offline | Pad on ch 10 audible (fallback factory kit), one `PadHit`, decays to silence. A key plays the live synth and its note-off releases it. Sustain + CC 123 silence everything. Pad mute is silent but still captured |
| Timestamps / latency (`tests/live_timing.rs`) | Audio advancing continuously: a `process_timed` callback every 256 frames at its wall instant, messages delivered between callbacks | On time: event + 8.00 ms (block + quantum), spacing exact (≤ 1.5 frames). Burst delivered 40 ms late: all 3 counted late (max 40.3 ms), played together at the next quantum. Burst inside the budget: 2 ms spacing kept. 2 s stall / 5 s outage: an event during it plays 13.3 ms after resume; the next one is not held back (28.0 ms = 20 + 8). A 10 s stamp on a device applies at 0.505 s |
| Robustness (`tests/live_timing.rs`) | Saturating burst then silence; Song recording across a punch wrap with the UI stalled; a lost input with the pedal down; lock hold | 976 queued commands → 0 without further input, synth silent. Punch capture at beat 7.8933 → startBeat 3.8933. A pre-record key is not recorded. Sustained voices released, sustain off. UI work under the lock ≤ 78 µs; MIDI thread per message median 1.1 µs, p99 ≤ 8 µs |
| Knobs on the real engine | CC 75 / 77 / 74 / 70 through `MidiUi` (`KnobRouter` + `EngineKnobs` over `ArrangementEngine`) | Soft takeover holds a far knob back; BPM 180, master 0 dB, swing 0.75, pad pitch +24; one undo restores the pitch (gesture grouping). Legacy CC 3 (reverb, after pickup) and CC 5 (master volume) with an empty knob map |
| Virtual MIDI ports (midir, CoreMIDI on this Mac; ALSA on Linux; `#[ignore]`d, run with `--ignored`, fail without MIDI) | `virtual_port_end_to_end` (in-process source); `hotplug_external.rs` (a **child process** owns the port); `host_virtual.rs` (restart on disk, connection health) | In-process: note → audio, port closes → device dropped, held note released. External process: listed after 257 ms, removal seen after 96 ms, held note released. Without the run-loop thread: never listed. Last device restored from disk after a restart (setting on), TS choice (setting off), reattached by name when it comes back with a new id. A failed open is reported (`Connection::Failed`) and retried; a source replaced under the same name is heard again |
| Realtime | `core_alloc_guard.rs` extended with `PadHit`, pad mute, repeat release-all and timestamped synth notes from the MIDI producer | no allocation or free inside `Core::process` |

Golden fixtures are regenerated with `npx vitest run --config rune/parity/vitest.golden.config.mts` (`midi_routing.json`, `midi_devices.json`, `midi_monitor.json`).

## Electron gap fixed in Rust: `autoConnectLastDevice` (user decision 2026-11-18)

In the Electron app `settings.midi.autoConnectLastDevice` is shown but never read, and the preferred device lives in memory only (inventory §3.1, §5 gap 2). In Rust:
- **Persisting.** A change worth persisting is reported to `MidiHost::set_persist`: an explicit `select_device`, an automatic pick of an MPK mini, or a remembered device re-keyed to a new id. `settings::enable_with_settings` wires it to `settings::write_last_device`, which writes the Rust-only key `micro-daw:midi-last-device` (`{"id", "name"}`, name without the ALSA suffix), so `micro-daw-settings` keeps exactly the Electron format. Any other automatic pick (an IAC bus, a virtual port) is the preferred device in memory only, as in the TS. It is never persisted, so a fallback cannot become sticky over the MPK.
- **Restoring.** At `enable`, `settings::device_to_restore` returns the stored device when the setting is on (default on), and the selector starts with it as the preferred device. It is attached if present. If absent, the usual MPK / first-input fallback runs and the stored device stays preferred, so it is re-attached when it is plugged in.
- **Matching.** By id first, else by port name. The name is normalised (`normalize_port_name` strips ALSA's ` client:port`). Among same-named ports, the one at the same position wins. The name is kept for the whole session, so recovery works on every replug. ALSA port ids change across replugs; CoreMIDI unique ids do not.
- **Setting off.** The selector starts empty: the TS behaviour exactly.

`src/` is unchanged for this gap.

## Electron bugs fixed in both apps (user decision 2026-11-18)

- **Split mode.** `split` behaved exactly like `auto` while its tooltip promised "left half drums, right half synth".
  - Now notes below the split point trigger pads chromatically, ignoring the profile's pad map: `pad = (note − split + 16) mod 16`. The 16 notes just below the split are pads 0–15, and lower notes repeat that layout.
  - Notes at or above the split play the synth. Learn still consumes first.
  - The split point defaults to 60 (C4) and is stored under `micro-daw:midi-split-point` (a JSON number) in both apps (`src/services/midiSplit.ts`; `settings::{read,write}_split_point`). The setter is `setSplitPoint` / `Router::set_split_point`.
  - The Auto tooltip now reads "notes mapped to pads in your controller profile trigger pads; other notes play the synth".
  - A split-point control in the UI comes with phase 7 / 9.
- **Sustain on a lost input.** A switch, unplug or dispose released the input's held keys but left its pedal down, so the notes it sustained rang on. Now the input's pedal is lifted too (CC 64 = 0 on each channel it held down, after the note-offs): `MidiService.sustainChannels` / `HeldNotes::take_sustain`.

## Deviations (what the user notices)

- **Timing is better than Electron.** The TS ignores MIDI timestamps: notes play at `Tone.immediate()` when the JS callback runs, so they inherit event-loop jitter. Rust schedules each note at its timestamp + one device block + one render quantum: the latency is constant and there is no jitter. Absolute latency is about the same as Electron's (≈ 8 ms at 256 frames / 48 kHz, plus output latency).
- **Hot-plug.** On macOS, CoreMIDI notifications are immediate, and a source replaced between two polls is reconnected. On Linux / Windows, ports are polled every 500 ms (webmidi gets OS events), so a port replaced under the same id within 500 ms can go unnoticed there.
- **Late MIDI bursts** (delivered after their latency budget) play at once and lose their spacing, as in Electron. On-time events are better than Electron (no jitter).
- **`enable`** has no 5 s timeout race. midir's client creation is synchronous; failure returns false. A wedged CoreMIDI server would block the call instead of timing out.
- **Device `manufacturer` is empty.** midir exposes only the port name.
- **SysEx is ignored** (Web MIDI without the sysex permission does the same). The monitor never shows SysEx in either app.
- **Song-recording captures of synth notes** are stamped by the core at the note's frame (transport beat, generation). The TS captures synchronously at `Tone` now. The position is the note's, never the UI frame's.
- **Knob dispatches, legacy CCs, raw listeners and the MIDI monitor** are applied on the next UI frame (woken at once) instead of synchronously in the MIDI callback (same order, same values).
- **Kept TS behaviour (not changed):**
  - channel 10 is not special;
  - the routing mode is not persisted;
  - CC 121 only lifts the pedal;
  - a key pressed while muted is tracked but never sounds, even after unmute.

## Verified / not verified

**Verified here (this Mac, CoreMIDI):**
- Everything in the evidence table.
- A real midir connection through virtual ports, in-process and owned by a child process: device listing, select, note → audio, removal detected, held note released. Restart with the last device on disk. Failed open reported and retried, replaced source reconnected.

**Not verified (needs the user's hardware or other platforms):**
- **Real Akai MPK mini Play mk3:**
  - its port names and the auto-select by `/mpk\s*mini/i`;
  - pad / key channels and notes against the default profile;
  - knob CCs 70–77, the sustain pedal;
  - unplug / replug and sleep / wake reconnection;
  - the persisted-device restore after a restart.
- **Felt latency and jitter on real hardware.** The timestamp maths is tested with synthetic clocks and a virtual port. A key-to-sound measurement needs the device.
- **Linux (ALSA) and Windows (WinMM).** The code is platform-neutral midir, and the virtual-port test is compiled for Linux, but neither platform was run. Windows has no virtual ports, so its end-to-end test is the hardware test.
- **Same-id replacement on CoreMIDI.** In the runs here, the replacement source got a new id (name recovery handled it). The "same id, list unchanged" path, which reconnects through the removed-source counter, is implemented but was not hit.
- **Release-build timings.** The lock and per-message timings are from debug builds.
- **UI wiring.** The device selector, routing selector (with the new split-point control), controller dialog and MIDI monitor views come in phases 7–9. The APIs they call are the ones tested here. The app loop must call `notify::ensure_started` first (done in `mdaw-app`), then `build_with_settings` + `settings::enable_with_settings`, and on every frame / wake `MidiUi::pump`, `MidiHost::poll` and `ProfileSaves::apply`.
