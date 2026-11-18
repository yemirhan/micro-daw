# micro-daw → Rust (Rune) port plan

Status: living document. The parity table is in [`docs/rune-parity.md`](rune-parity.md); per-phase
comparisons are in [`docs/rune-port/`](rune-port/).

The Electron app on `main` stays the reference until parity is signed off. The Rust app lives in
[`rune/`](../rune) as its own Cargo workspace, so `npm start` / `npm test` keep working side by side.

---

## 1. What Rune is (research, rev `307b2cd7`, 2026-11-18)

Source: `github.com/anomalyco/rune` (`crates/rune/README.md`, `crates/rune-kit/README.md`, `examples/*`).

| Question | Answer | Consequence for the port |
|---|---|---|
| UI model | Native GPU UI in plain Rust: `fn view(&State, &mut Cx) -> El`; handlers mutate `&mut State` between frames, then the view re-runs and is diffed. Taffy flexbox layout; springs for motion. | The React tree has to be rewritten as Rust views. No HTML/CSS/Tailwind; no React. |
| Webview? | **No.** wgpu + winit, its own text (Parley/swash) and renderer. | No web content at all. Electron's CSP, navigation / window-open guards and trusted-sender IPC have nothing to guard (see §5). |
| Components | `rune-kit`: Button, IconButton, Switch, Checkbox, SegmentedControl, Select, Menu, context_menu, Dialog, Popover, Tooltip/`help`, Toast, Progress, ProgressCircle, TextInput, Combobox, List, TabStrip, SplitView, Badge, etc. **No slider and no rotary knob** (`rune-knobs` is a dev tuning panel). | Knobs, sliders, step grid, pad grid, timeline and piano roll are custom elements (shapes + `on_drag`/`on_wheel`/`on_step`). |
| Drawing | Vector `Shape` (polylines, fills, paths) per element, `Pixels` RGBA images, `Live<T>` values that redraw without re-running the view. | Waveforms are `Shape` polylines or fills; the playhead, pad flashes and meters use `Live` values fed from the audio side. |
| Input | Pointer capture, drags with per-gesture scratch, wheel, secondary press, key down/up (`on_key`, `on_key_up`, `on_physical_input`), `Chord` shortcuts, focus groups, modals. | Every gesture of the Electron app can be expressed. Internal drag and drop (browser row → pad): macOS has `DragItem::within_app` (files to the app's own drop targets); for all platforms we build it from `on_drag` + `tx.element_at`. |
| Files | `tx.open_file` / `save_file` / `open_folder` (feature `native-file-dialog`; rfd on macOS/Windows, XDG portal on Linux). `on_drop_files` for Finder drops (macOS, Windows, X11; **not Wayland**). | Project open/save, sample import and the folder picker map 1:1. |
| Audio | `Player` (clip playback: AVFoundation on macOS, Media Foundation on Windows; not on Linux) and `Recorder` (microphone to file, **macOS only** at this rev). **No low-latency engine, no scheduling, no mixing.** | Rune can't host the DAW engine. We use **cpal** for output and our own sample-accurate engine. |
| MIDI | **None.** | **midir** (CoreMIDI / WinMM / ALSA). |
| Threads | `AppSpec::setup` gives a `Waker`; `poll` runs before each frame; `AppHandle::update` from any thread; `Live::set` from a bridge thread (never from the audio callback). | The audio thread writes atomics and a lock-free ring. A UI bridge drains them in `poll` and sets `Live` values. |
| Packaging | `scripts/mac-app` (bundling, relaunch), plain cargo builds elsewhere. No installer, **no auto-updater**. | Auto-update is a gap (§5). |
| Testing | Headless runtime with a virtual clock: `--snap out.png --scene x`, `--tree`, `--script`, and in-process `Runtime::new` + `find` / `pointer_*` / `key` / `presented` / `elements`. | Every UI phase gets headless PNG snapshots (side by side with `scripts/ui-snap.mjs` output) and element-tree tests. |
| Platforms | macOS, Windows, Linux, web, iOS (pre-release, pinned `dev` rev). Toolchain 1.99.1. | We pin the same rev and toolchain (`rune/rust-toolchain.toml`). |

## 2. Feature inventory (parity checklist)

The Electron app was inventoried from the code (every view, control, shortcut, IPC handler and
service), not from the docs. The full checklists with `file:line` cites live in:

- [`inventory/beat-ui.md`](rune-port/inventory/beat-ui.md): Beat view, pads, pad editor, step sequencer, beat transport, kit/track menus, missing-pack UI, dock pads, top bar, Song transport bar, `beatHelpers`.
- [`inventory/engine.md`](rune-port/inventory/engine.md): ArrangementEngine API and undo, transport/timing (count-in, loop recording, quantize/swing formulas, gate lengths, note repeat), SamplerKit voice semantics, presets, voices, FX, master bus, automation, factory/classic drum synthesis, export, and where each review-round fix lives.
- [`inventory/views.md`](rune-port/inventory/views.md): App shell, every global shortcut, command palette, start screen, sidebar, Song view (timeline, regions, tracks, groups, markers, automation, track FX), piano roll, on-screen keyboard, mixer, live FX, synth editor, selectors, export dialog, templates, settings, Samples view, sample browser, MIDI monitor.
- [`inventory/main-packs-midi-project.md`](rune-port/inventory/main-packs-midi-project.md): main process (window, close guard, security, updater, all 30 IPC channels), pack store (limits, validation, catalog merge, download, installer state machine, recovery, previews, Get & load intent), MIDI (devices, routing, controller mapping, KnobRouter + soft takeover), project format and migrations, every localStorage key.

The condensed checklist (one row per feature, with status) is the table in
[`docs/rune-parity.md`](rune-parity.md). A row is ✅ only when the Rust behaviour was compared
against the Electron behaviour (test, probe, render or screenshot) and the comparison is recorded.

## 3. Architecture mapping

```
rune/                         Cargo workspace (toolchain 1.99.1, Rune pinned to 307b2cd7)
  crates/mdaw-model           pure data + logic, no audio, no UI   (≙ src/types, src/utils, src/shared, pure services)
  crates/mdaw-dsp             offline DSP: drum synthesis, classic machines, slicer, WAV/decoding, reverb IR
  crates/mdaw-engine          realtime + offline engine (≙ Tone.js graph + ArrangementEngine/LoopRecorder/NoteRepeat/
                              DrumEngine/AudioEngine/ExportService/SamplerKit/voices/FX)
  crates/mdaw-midi            midir input, device list, routing  (≙ MidiService, useMidiNotes routing core)
  crates/mdaw-packs           catalog, installer, recovery, previews (≙ src/main/packs, PackService core)
  crates/mdaw-app             Rune UI + app wiring  (≙ src/components, src/hooks, App.tsx, main.ts window/menus)
```

| Electron / web | Rust |
|---|---|
| React components + hooks | Rune `view` functions over one `AppState`; hooks become state structs + methods; `useSyncExternalStore` slices become versions checked in `poll` |
| Tailwind tokens / shadcn | A `theme` module with the same OKLCH tokens (`src/index.css`) as `rune::oklch` colours; rune-kit `Kit` overridden with them |
| lucide-react icons | The lucide SVG bodies we use, copied into `Icon` constants (Lucide is ISC) |
| Manrope / JetBrains Mono / Syne (@fontsource) | Same OFL fonts embedded as TTF through `AppSpec::fonts` |
| Tone.js context, Transport, Part, lookahead | **One engine core** `Core::process(out, frames)` that owns a sample clock, a tick transport (PPQ 192), per-track event lists in **beats**, and fires events at exact sample offsets inside each buffer. Runs inside the cpal callback live and in a loop offline (export, tests). There is no 100 ms lookahead: control-thread commands are timestamped in samples and applied at the next buffer. |
| `Tone.now()/immediate()` | `Clock` shared atomically: frames rendered + the wall-clock instant of that callback, so the control thread can read "now" in audio time, exactly or extrapolated |
| Transport ticks (`transportBeatAt`) | Computed by the core from the tempo map it applied; a scheduled start remembers `{time, beat}` so lookups before it return the start beat (R4-B1 by construction) |
| `Tone.Offline` (export, factory voice renders) | The same `Core` run offline at a fixed rate; drum voices rendered by pure DSP functions |
| SamplerKit (Tone BufferSource + Gain + Filter + Panner) | `SamplerKit` voices in Rust: per-voice start/stop sample, rate, linear ramps, 2-pole LP (−12 dB/oct, Q 0.707), equal-power pan, choke and steal rules identical |
| PolyInstrument / Subtractive / FM / Pluck / SampledPiano | Rust voices with the same parameters and envelopes; preset table ported verbatim |
| Track FX chain (Filter, Distortion, EQ3, Compressor, Chorus, FeedbackDelay, RoomReverb, Panner, Volume, Meter), master bus | Rust DSP blocks with the same order, defaults, ranges and bypass rules |
| Web Worker (classic synthesis) | `std::thread` render pool + cache; never on the audio thread |
| webmidi | midir (input ports, hot-plug by polling the port list) |
| Electron IPC (`ipcMain.handle` / preload) | Direct Rust calls: no process boundary. Long work (downloads, decoding, file IO) runs on worker threads that report back through channels and `AppHandle::update` |
| Electron dialogs | `tx.open_file` / `save_file` / `open_folder`; the unsaved-changes box becomes a rune-kit `Dialog` |
| `net` / fetch for packs | `ureq` over rustls with the same host allowlist, redirect checks, size/sha256 checks, timeouts and retries |
| localStorage | A JSON settings store in the platform config dir with the **same keys and value formats**, plus a one-time import from the Electron profile's localStorage (LevelDB) where it can be read (§5) |
| `.mdaw` project JSON | `serde` structs with the same field names (camelCase) and the same migrations, so the Rust app opens Electron projects and vice versa |
| vitest (Tone mock) | `cargo test` ports of each suite; timing tests run on the **real** engine core offline (no mock), which is stricter than the Tone mock |
| `npm run test:audio` probes (real Tone) | Offline-render regression tests of the engine core (sample-accurate assertions on onset positions) plus a short real-device smoke test |
| `scripts/ui-snap.mjs` | Rune headless `--snap` / `--tree` with scenes; screenshots stored next to the Electron ones under `docs/parity/` |

### Threading and realtime ownership (revised after the phase 0+1 reviews)

Anything that must be sample-accurate lives in the **core**, not on the UI thread:
- transport and loop
- metronome and count-in clicks
- pattern playback
- note-repeat scheduling and its cancellation on release
- gate releases
- loop-take hit **timestamping**

- **UI thread** (Rune). `AppState` owns the `ArrangementDoc` (data, undo and loop-take merging). It turns doc changes into commands and track schedules for the core. It receives **capture events** from the core: `{pad, velocity, audio_time, transport_beat, loop_wraps_since_on, generation}` for hits, releases and repeat hits. It merges them into the pattern with the phase-1 helpers (`quantize_beat`, `loop_gate_beats`, `merge_drum_hit`). Musical positions are computed by the core at the sample the hit sounded, so a slow UI frame only delays the merge, never the timing.
- **Audio thread** (cpal callback): `Core` only, with no allocation and no locks.
  - **One SPSC ring per producer**: UI → core and MIDI → core. Each command carries a target sample time. At the start of every buffer the core drains both rings and merges them by `(time, producer, seq)`, so ordering is deterministic.
  - Immutable track schedules are swapped in through a lock-free slot. The old `Arc` goes back to the UI thread through a return ring and is freed there.
  - Core → UI: an SPSC ring of capture and state events, plus atomics for the playhead and meters.
- **MIDI thread** (midir callback):
  - Timestamps each event in audio time.
  - Resolves pads through a **routing snapshot**: the controller pad map, the active drum track's sampler, routing mode and split point, and the note-repeat state. The UI publishes the snapshot as an `Arc` whenever any of these change.
  - Sends pad hits straight to the core so a pad sounds without a UI frame.
  - Pushes the raw event to the UI's MIDI queue (monitor, learn, keys).
- **Generations**:
  - Every transport start, stop, seek and count-in carries a **transport generation**. Stop or seek bumps it, so a pending count-in start or an older start is invalid by construction (R4-B1, R5-N1, R5-N4).
  - Capture events from an old generation are dropped.
  - **Session generations** do the same for project loads and Get & load (R5-N2).
- **Workers**: decoding, classic-kit rendering, pack downloads, WAV export and sample processing.

## 4. Phases

Each phase ends with: tests green, a parity comparison doc `docs/rune-port/phase-N-*.md` (same
inputs → same outputs, with evidence), the table in `docs/rune-parity.md` updated, two reviews
(GPT-6.1 Sol and Fable 5.2) with findings fixed, and a commit on `rune-port`.

| # | Phase | Contents | Parity evidence |
|---|---|---|---|
| 0 | Research + plan + skeleton | This document, the inventories, a Rune window building with cpal + midir | The build itself; the Electron screenshots |
| 1 | Model | Types with serde (project file compatible), normalise/migrate, arrangement and beat helpers (swing, quantize, gate length, pattern steps, GM map), piano-roll and automation helpers, UndoManager, ControllerMapping + KnobRouter + soft takeover maths, pack validation and source resolution, settings store keys | Ported vitest suites; golden JSON produced by the TS code (`vite-node` dumps) compared field by field; every template and a set of saved projects round-trip |
| 2 | DSP | Classic machines (literal port of `dsp.ts` / `voices.ts`), factory drum voices, slicer, WAV encode/decode (+ mp3/flac/ogg/m4a decode), RoomReverb IR, soft clip | Classic renders compared sample-by-sample against the TS output; factory voices compared against Tone renders captured in Chromium (envelope, spectrum, peak); slicer vectors |
| 3 | Engine core | Clock, transport, scheduler, SamplerKit, metronome, count-in, loop recorder, note repeat, DrumEngine sink rules, ArrangementEngine API, offline render, WAV + MIDI export | Ports of the engine vitest suites; ports of every `scripts/probes` timing probe as offline-render tests; export of the same project compared against the Electron export |
| 4 | Instruments + FX | Presets 0–40 + fallback piano, subtractive/FM/pluck/sampled piano voices, voice allocator, character chain, track FX chain, live FX chain, master bus, automation playback | Allocator/preset tests ported; renders of each preset's middle C compared with Chromium renders (pitch, envelope, level) |
| 5 | MIDI + controller | midir devices, routing modes (auto/keys/split), sustain, all-notes-off, controller learn, knob routing | Routing tests ported; virtual MIDI port end-to-end test; real MPK test is the user's |
| 6 | Packs | Catalog fetch/merge/validate, installer (staging, resume, retries, sha/size, atomic swap, crash recovery, pause on quit), uninstall, previews, Get & load intent | Installer tests + fault injection (ported from the Electron harness); live catalog install of one pack, compared with the Electron result on disk |
| 7 | UI shell + Beat view | Theme, fonts, icons, start screen, top bar, sidebar, Beat view (pads, pad editor, step sequencer, transport, kit/track menus, missing packs, chips), computer-keyboard pads, sample browser | Side-by-side screenshots per state; element-tree tests for every interaction listed in `inventory/beat-ui.md` |
| 8 | Song view | Timeline, tracks, regions (move/resize/split/duplicate/copy/paste), groups, markers, loop, automation lanes, track FX, mixer, transport bar, piano roll, on-screen keyboard, dock | Screenshots + interaction tests per `inventory/views.md` §5–8 |
| 9 | Remaining views | Packs page (inventory: `packs-ui-and-sample-dsp.md` §A), Samples view (edit/process/generate, with the SampleProcessor DSP of §B: reverse → filter → pitch shifter → feedback delay → noise-IR reverb → normalize, 3 s tail cap, compared against Chromium renders), Settings, Dev MIDI monitor, command palette, export dialog, controller setup dialog, template picker, synth editor, live FX panel | Screenshots + tests per the inventories |
| 10 | App shell parity | Window min size, full screen, close guard + unsaved dialog, title, recent projects, autosave, settings import, packaging | Manual checklist + headless tests |

## 5. Risks, gaps and decisions

**Things Rune cannot deliver (recorded here and in the report, not dropped):**

1. **Low-latency audio and MIDI**: Rune has neither. We use cpal and midir. This is a dependency choice, not a parity loss.
2. **Auto-updater** (`electron-updater` against GitHub releases, download + install on quit): Rune has no updater. Plan: the Settings "Check for updates" checks the GitHub releases API and offers to open the release page; in-place download and install is **🟡 partial**. **Decided 2026-11-18 (user left it to us):** ship the check + "open release page" fallback in phase 10 (Settings button, same version comparison as electron-updater's semver check); a native in-place updater (Sparkle on macOS) stays out of scope until requested.
3. **File drops on Linux Wayland** don't arrive (Rune limitation). macOS/Windows/X11 work.
4. **Electron security model** (CSP, navigation and window-open guards, trusted-sender IPC) protects web content that the Rust app does not have. Marked **N/A by construction** in the table, with the remaining guarantees kept: only https downloads from the allowlisted host, path-traversal checks, no remote code, external links only through `open_url` to `https://`.
5. **Existing localStorage settings** (`micro-daw:*` keys) live in Chromium's LevelDB inside the Electron profile. The Rust app keeps the same keys and formats in its own JSON store; importing an existing Electron profile is best effort (reading LevelDB) and listed as a separate row.
6. **Fonts**: `@fontsource` ships WOFF/WOFF2 only; Rune needs TTF/OTF bytes. We embed the upstream OFL TTFs of the same families.

**Engineering risks:**

- **Sample-accurate scheduling.** Tone schedules 100 ms ahead on the JS thread; past bugs (R4-B1 stale position, R5-N1 pending start, R5-N4 seek before start) came from its stop/start/lookahead semantics. Our core owns the clock and fires events inside the buffer, so these become simple invariants. Each one gets an offline regression test that replays the probe's sequence.
- **Same inputs, same outputs with a different engine.** Pattern data, quantize/swing, gate lengths, MIDI export and project JSON must be **identical** (golden tests against TS output). Audio can only be compared within tolerances: the classic machines are pure DSP and can match sample for sample; Tone-graph sounds (factory kit, synths, FX) are compared by onset, envelope, peak/RMS and spectral centroid against renders captured from the Electron app's own engine in Chromium.
- **Live latency.** Tone's live pads play at `immediate` (one render quantum + output latency). cpal with a 128–256 frame buffer is at least as fast; the buffer size will be configurable.
- **Sample rate.** Factory voices render at the device rate; classic voices at 44.1 kHz then resample (Tone's BufferSource resamples implicitly). The Rust buffer cache resamples once at load.
- **UI scale.** Rune re-runs the view per event; the step grid (up to 16 × 256 cells) and the timeline must use `memo`, `Shape`-drawn cells and `Live` values for the playhead, so a hit doesn't rebuild the screen (the same goal as R3-E4 / R4-L-a).
- **Packaging and signing.** Electron Forge gives us dmg/zip/deb/rpm/Squirrel, code signing and notarization today. The Rust app needs its own: `scripts/mac-app` for the bundle, plus codesign/notarytool, a Windows installer and Linux packages. This is phase 10 work, and signing needs the user's certificates.
- **Rune is pre-release.** We pin an unreleased `dev` revision plus five `[patch.crates-io]` forks (winit, wgpu-hal, parley_engine, accesskit_macos, accesskit_windows). Bumping Rune means bumping every patch to the same rev. A breaking change upstream could cost a phase.
- **Decoders.** symphonia and Chromium differ on MP3/AAC encoder priming samples, so `startSec` trims and slice points on compressed files can shift by a few ms against the Electron app. WAV/AIFF/FLAC are exact. Decoded lengths get compared on the pack catalog's files in phase 2/6.
- **Saved-project corpus.** Phase 1 verified templates, synthetic legacy files and hand-written v1.0/v1.3 files. We have no corpus of real user projects; the user should open a few of their own `.mdaw` files in the Rust app when the UI lands.
- **Scope.** About 43k lines of TS. Phases are ordered so the core (model, engine, Beat view) reaches parity first. Every unfinished row stays visible as 🟡 or ❌ in `docs/rune-parity.md`.

**Quirks** (listed in `inventory/engine.md` §8 and `inventory/views.md` intro): the port **keeps** them by default for 1:1 parity (e.g. `play()` starts at 0, solo ignored in WAV export, no automation in export). Where one is a clear bug, it's listed for the user to decide, not changed silently.

**Electron bug fixed in both apps (user decision 2026-11-18)**, so parity stays 1:1 and measured against the fixed TS (details in `rune-port/phase-4-instruments.md`, "Resolved decisions"):
- `SubtractiveVoice` applies oscA/oscB `semi`/`fine` at build (warm-pad, saw-lead sound as designed);
- turning a filter LFO off keeps the static cutoff (it silenced lowpass / bandpass tracks and the live synth);
- a volume change keeps a muted track / the live synth muted (mute is a gate after the volume in the TS, a flag in Rust).
