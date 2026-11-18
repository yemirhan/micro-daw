# Phase 5: MIDI + controller (`rune/crates/mdaw-midi`) vs the Electron app

**Scope** (plan §4 row 5, inventory [§3 MIDI](inventory/main-packs-midi-project.md#3-midi)):
- `MidiService.ts`: enable, device list, auto-select, `selectDevice`, hot-plug, held-note release, raw listeners.
- `useMidiNotes.ts`: the routing core (auto / keys / split, pad vs key notes, channel rules, velocity, sustain, all-notes-off, learn consumption, CC dispatch, legacy CC fallbacks).
- `installKnobRouting` (`KnobRouter.ts`): knob dispatches wired to the engine (pad params, swing, tempo, track volume, master volume) with soft takeover and undo grouping.
- `useMidiMonitor.ts`: the raw-message formatter and buffers of the Dev view's MIDI monitor.
- One user decision: **`autoConnectLastDevice` is now wired and the preferred device persisted** (Electron gap fixed in Rust, below).

The pure controller logic (`ControllerMapping`, `KnobRouter`, `KnobPickup`) was ported and golden-tested in phase 1 (`mdaw_model::{controller, knobs}`).

## Method

**Design.** The Rust crate keeps the TS state machines as pure code and puts the I/O around them:

| Module | Port of | Notes |
|---|---|---|
| `message.rs` | webmidi 3.3.1 parsing + `velocityToNormalized` | note-on velocity 0 → note-off; velocity `clamp(raw/127)`; channels 1..16; every CC (120–127 included) |
| `router.rs` | `useMidiNotes` | `Router`: one method per hook callback. Every effect is an `Action` emitted in the TS call order |
| `devices.rs` | `MidiService` device logic | `DeviceSelector` returns attach / detach ops; `HeldNotes` = `heldNotes` |
| `timing.rs` | (new) | midir timestamps → `Instant` → engine audio time |
| `engine_link.rs` | `installKnobRouting` + the engine calls of the hook | `EngineLink` (MIDI thread → core), `MidiUi` (UI thread: captures, knobs, legacy CCs) |
| `host.rs` | `MidiService` I/O | midir enable / enumerate / connect, hot-plug watcher |
| `monitor.rs` | `useMidiMonitor` | `formatEvent`, buffer, 500 cap, filters, pause |
| `settings.rs` | (new) | persisted last device |

**Threads** (plan §3 "Threading and realtime ownership"):
- **MIDI thread** (midir's callback). For each message it:
  1. locks `MidiShared` (router + held notes + timestamp mapper + raw listeners);
  2. maps the timestamp to audio time;
  3. runs the router;
  4. sends engine actions through the MIDI producer's `LiveInput` (a lock-free SPSC ring), then calls `LiveInput::flush()`.
- **Mutex.** It is shared only by the MIDI thread and the UI thread (on-screen keyboard and pads, panic, mode, mute, learn). The audio thread never takes it, so MIDI cannot block audio.
- **UI thread.** Everything that edits the doc goes through an unbounded channel to the UI thread. `MidiUi::pump`, called every frame, applies it:
  - Song-recording captures of synth notes;
  - knob dispatches (`KnobRouter`);
  - legacy CCs (the engine's `handle_legacy_cc`, which owns that pickup).
- **Hot-plug.** A watcher thread enumerates the ports every **500 ms** (`HOTPLUG_POLL_INTERVAL`; midir has no change notifications). When the list changed, it wakes the UI, which runs `MidiHost::poll` → `handleDeviceChange`. Detection latency is ≤ 500 ms plus one UI poll.

**Engine additions** (`mdaw-engine/src/core`, realtime-safe; covered by the allocation guard):
- `Command::PadHit`: `drumHit`, a one-shot hit with no note repeat.
- `Command::SetLivePadMute`: the hook's `muted`. Pad hits and repeats are still captured, flashed and repeated, but not played.
- `Command::RepeatReleaseAll`: `noteRepeat.releaseAll()` in the panic.
- `LiveInput::synth_note_at`: live synth notes at a MIDI audio time, plus the 4 ms `LIVE_OFFSET`.
- `ControllerMapping`'s clock is now `Send`, so the mapping can live on the MIDI thread.

**Timestamps.** midir stamps messages in µs from a backend-specific origin. `TimestampMapper` learns `arrival − timestamp` with a minimum filter (a message never arrives before it happened). It re-anchors when the offset jumps by more than 250 ms, and it is reset on every new connection. The resulting instant goes through the core's published anchor (`ClockSnapshot::audio_time_for_instant`): one device block plus one render quantum after the event. Every event therefore gets the same latency and keeps its relative timing (no jitter), even when the OS delivers a burst late.
- Pads sound at that time (≙ `drumEngine.hit` at `Tone.immediate()`).
- Synth notes sound 4 ms later (≙ `audioEngine.noteOn` at `immediate() + LIVE_OFFSET`).
- Without an anchor (no device callback yet), a message plays at the next quantum.

## Parity evidence

| Area | Comparison | Result |
|---|---|---|
| Routing core (`useMidiNotes`) | `rune/parity/ts/midi_routing.golden.ts`: the real hook, the real `ControllerMapping` singleton and the real `NoteEventBus`, with audio / drum / arrangement engines and note repeat replaced by call loggers. 36 scenarios: 12 written by hand (pads + keys, mode changes while held, sustain + pedal up, CC 120/121/123, recording captures, mute, re-hit without note-off, channel-filtered pad map, pad learn + wizard, knob learn, mapped + legacy CCs with pickup, volume CC without a handler) and 24 seeded random streams (60–200 mixed inputs each). Each Rust action is expanded into the TS calls the core makes for it (`PadDown` = `drum.hit` unless muted, capture while recording, flash, `drum` event, `repeat.press`, …). Legacy CCs go through a real `ArrangementEngine::handle_legacy_cc` | **3,486 steps, 4,845 calls identical, in order.** After every step the state also matches: held notes (order and velocity), sustain, sounding notes, mode, learn state, profile, last activity, effect params. The unmount panic matches too |
| Device handling (`MidiService`) | `midi_devices.golden.ts`: the real `MidiService` over a fake webmidi whose input list a script changes. 6 hand-written and 20 random sessions: plug / unplug / replug, select (unknown ids too), notes held across switches and unplugs, note-on velocity 0, CCs, dispose. Rust feeds raw bytes through `parse` + `HeldNotes` + `DeviceSelector` | **881 steps identical**: active and preferred device, device list, 195 note callbacks (synthesised note-offs included, in order), 286 change notifications |
| MIDI monitor (`useMidiMonitor`) | `midi_monitor.golden.ts`: the real hook fed 80 raw messages of every kind (malformed ones too: `undefinedNaN (undefined)`, `CC#undefined`, …), the 50 ms flush, the 6 type filters, the 500-event cap over two flushes, pause | identical (78 events; clock and active sensing dropped) |
| Ported vitest suites | `useMidiNotes.test.ts`, `useMidiNotes.routing.test.ts` (the routing and learn cases), `stuckNotes.test.tsx` (the routing cases), `MidiService.test.ts` (the device cases) as `router_tests.rs` / `devices.rs` tests. The fake synth stacks voices like `fakeAudioEngine.ts`, so a leaked voice fails | 39 unit tests pass |
| End to end through the engine (`tests/engine_e2e.rs`) | Raw MIDI → `MidiShared` → router → `LiveInput` → the real core, rendered offline | Pad on ch 10 audible (fallback factory kit), one `PadHit`, decays to silence. A key plays the live synth and its note-off releases it. Sustain + CC 123 silence everything. Pad mute is silent but still captured |
| Timestamps / latency | Core driven with `process_timed` (256-frame blocks, synthetic instants). One calibration message, then three pads stamped 10 ms apart but delivered together 40 ms late | Each pad lands at its event time + (256 + 128) / 48000 s within one frame; the spacing is 10 ms within one frame |
| Knobs on the real engine | CC 75 / 77 / 74 / 70 through `MidiUi` (`KnobRouter` + `EngineKnobs` over `ArrangementEngine`) | Soft takeover holds a far knob back; BPM 180, master 0 dB, swing 0.75, pad pitch +24; one undo restores the pitch (gesture grouping). Legacy CC 3 (reverb, after pickup) and CC 5 (master volume) with an empty knob map |
| Virtual MIDI port (midir, CoreMIDI on this Mac; ALSA on Linux) | `virtual_port_end_to_end`: a virtual source created in the test, listed and selected through `MidiHost`, a real midir connection | A note on the port → audio from the core. A held key, then the port closes: the watcher wakes the UI, `poll` drops the device, the held note is released (no sounding notes) |
| Realtime | `core_alloc_guard.rs` extended with `PadHit`, pad mute, repeat release-all and timestamped synth notes from the MIDI producer | no allocation or free inside `Core::process` |

Golden fixtures are regenerated with `npx vitest run --config rune/parity/vitest.golden.config.mts` (`midi_routing.json`, `midi_devices.json`, `midi_monitor.json`).

## Electron gap fixed in Rust: `autoConnectLastDevice` (user decision 2026-11-18)

In the Electron app `settings.midi.autoConnectLastDevice` is shown but never read, and the preferred device lives in memory only (inventory §3.1, §5 gap 2). In Rust:
- **Persisting.** Every change of the preferred device (a manual `select_device`, or the first automatic pick) is reported to `MidiHost::set_persist`. The app writes it with `settings::write_last_device` under the **Rust-only key** `micro-daw:midi-last-device` (`{"id", "name"}`), so `micro-daw-settings` keeps exactly the Electron format.
- **Restoring.** At `enable`, `settings::device_to_restore` returns the stored device when the setting is on (default on), and the selector starts with it as the preferred device. It is attached if present. If absent, the usual MPK / first-input fallback runs and the stored device stays preferred, so it is re-attached when it is plugged in.
- **Matching.** By id first. A device restored from settings can also match by exact name: ALSA port ids change across replugs, CoreMIDI unique ids do not.
- **Setting off.** The selector starts empty: the TS behaviour exactly.

`src/` is unchanged; the fix exists only in Rust.

## Deviations (what the user notices)

- **Timing is better than Electron.** The TS ignores MIDI timestamps: notes play at `Tone.immediate()` when the JS callback runs, so they inherit event-loop jitter. Rust schedules each note at its timestamp + one device block + one render quantum: the latency is constant and there is no jitter. Absolute latency is about the same as Electron's (≈ 8 ms at 256 frames / 48 kHz, plus output latency).
- **Hot-plug** is detected by polling every 500 ms (webmidi gets OS events). Unplugging and replugging within 500 ms can go unnoticed. Then the connection may stay pointed at a stale port until the next change.
- **`enable`** has no 5 s timeout race. midir's client creation is synchronous; failure returns false. A wedged CoreMIDI server would block the call instead of timing out.
- **Device `manufacturer` is empty.** midir exposes only the port name.
- **SysEx is ignored** (Web MIDI without the sysex permission does the same). The monitor never shows SysEx in either app.
- **Song-recording captures of synth notes** are applied on the UI thread (`MidiUi::pump`) with the event's audio time. The TS captures synchronously at `Tone` now. The musical position is computed at the note's audio time, so a slow frame only delays the merge.
- **Knob dispatches and legacy CCs** are applied on the next UI frame instead of synchronously in the MIDI callback (same order, same values).
- **Kept TS behaviour (not changed):**
  - `split` = `auto` (inventory §5 gap 1; the selector tooltip still claims "left half drums");
  - channel 10 is not special;
  - the routing mode is not persisted;
  - CC 121 only lifts the pedal;
  - a key pressed while muted is tracked but never sounds, even after unmute.

## Verified / not verified

**Verified here (this Mac, CoreMIDI):**
- Everything in the evidence table.
- A real midir connection through a virtual port: device listing, select, note → audio, port removal detected by the watcher, held note released.

**Not verified (needs the user's hardware or other platforms):**
- **Real Akai MPK mini Play mk3:**
  - its port names and the auto-select by `/mpk\s*mini/i`;
  - pad / key channels and notes against the default profile;
  - knob CCs 70–77, the sustain pedal;
  - unplug / replug and sleep / wake reconnection;
  - the persisted-device restore after a restart.
- **Felt latency and jitter on real hardware.** The timestamp maths is tested with synthetic clocks and a virtual port. A key-to-sound measurement needs the device.
- **Linux (ALSA) and Windows (WinMM).** The code is platform-neutral midir, and the virtual-port test is compiled for Linux, but neither platform was run. Windows has no virtual ports, so its end-to-end test is the hardware test.
- **CoreMIDI change notifications.** The hot-plug watcher relies on CoreMIDI's port list updating without a run loop on the watcher thread. It did in this test process. Inside the Rune app the main thread runs a run loop, which should only help.
- **UI wiring.** The device selector, routing selector, controller dialog and MIDI monitor views come in phases 7–9. The APIs they call are the ones tested here.
