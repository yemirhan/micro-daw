# Rune port: running parity table

Reference: the Electron app on `main` at `4d98422`. Rust app: [`rune/`](../rune).
Reviews: `docs/rune-port/review-phase-*.md`.
Statuses: ✅ = compared against Electron and matches (evidence linked) · 🟡 = partial (what's missing
is said) · ❌ = missing · N/A = no Rust equivalent needed (reason given).

Detailed per-feature checklists with `file:line` cites: [`docs/rune-port/inventory/`](rune-port/inventory/).
Plan: [`docs/rune-port-plan.md`](rune-port-plan.md). Phase comparisons: `docs/rune-port/phase-*.md`. Reviews: `docs/rune-port/review-phase-*.md`.

## Model, project format, persistence

| Feature | Status | Evidence / notes |
|---|---|---|
| Project JSON (`.mdaw`) read/write, field-compatible | 🟡 | data layer ✅ ([phase 1](rune-port/phase-1-model.md) + [review fixes](rune-port/review-phase-1.md): `open_project` with TS migration + coercion, v1.0/v1.3 files, unknown keys preserved, fuzzed); file I/O + dialogs come with the app (phase 10) |
| Migrations on load (velocity >1, GM drum notes, kits, beat state, loop markers, swingGrid, absolute pack paths) | ✅ | [phase 1](rune-port/phase-1-model.md): `migrate` golden (6 templates + 6 legacy projects, driven through the real TS engine) |
| Templates (all 6+) load identically | ✅ | [phase 1](rune-port/phase-1-model.md): `migrate` golden, `loadFromTemplate` + `loadProject` paths |
| Undo manager (50 steps, groups, flush/rebase, redo rules) | ✅ | [phase 1](rune-port/phase-1-model.md): `undo` golden (4,000 steps) + `doc` golden |
| Arrangement edits (tracks, regions, split, resize, duplicate, copy/paste, groups, markers) with undo labels | ✅ | [phase 1](rune-port/phase-1-model.md): `ArrangementDoc`, `doc` golden (279 steps vs the real TS engine); one deliberate deviation (audio duplicate aliasing; user approved independent copies 2026-11-18) |
| Automation lanes/points helpers | ✅ | [phase 1](rune-port/phase-1-model.md): `automation` golden |
| Piano-roll helpers (snap, move, resize, quantize, velocity, hit testing, zoom) | ✅ | [phase 1](rune-port/phase-1-model.md): 47 vitest cases + `piano_roll` golden (34,793) |
| Beat helpers (pad layout, labels, velocity from pointer, step matrix, chop plan, formatting) | ✅ | [phase 1](rune-port/phase-1-model.md): `beat_helpers` golden (≈22k) |
| Settings / localStorage keys (same keys + formats) | 🟡 | readers/writers ✅ ([phase 1](rune-port/phase-1-model.md): `settings` golden, all 19 keys); wiring into the app comes with each view |
| Import of an existing Electron profile's settings | ❌ | best effort, see plan §5 |

## Engine and audio

| Feature | Status | Evidence / notes |
|---|---|---|
| Sample-accurate transport (play/stop/seek, loop, BPM change) | ✅ | [phase 3](rune-port/phase-3-engine.md): offline-render tests on the real core; deviation: no 100 ms look-ahead (starts sooner) |
| R4-B1 / R5-N1 / R5-N4 transport regressions | ✅ | [phase 3](rune-port/phase-3-engine.md): every probe scenario ported, impossible by construction |
| Pattern playback (transient loop, R2-1 / R3-U6) | ✅ | [phase 3](rune-port/phase-3-engine.md) |
| Metronome (+ count-in clicks, −15 dB, accents) | ✅ | [phase 3](rune-port/phase-3-engine.md): click times/accents and Tone envelope formulas; the sine is computed directly, not from Chromium's wavetable |
| Loop recording: count-in, quantize, swing, overdub/replace, undo grouping | ✅ | [phase 3](rune-port/phase-3-engine.md): ported beat/round5 cases on the real core |
| Gate lengths in musical time across loop wraps and tempo changes | ✅ | [phase 3](rune-port/phase-3-engine.md) |
| Note repeat (transport-locked + free-running, release cancellation) | ✅ | [phase 3](rune-port/phase-3-engine.md): in the core, exact grid (no pump/look-ahead, documented) |
| SamplerKit (trim, pitch, reverse, gain, pan, LP filter, decay, choke, gate/one-shot, voice limit) | ✅ | [phase 3](rune-port/phase-3-engine.md): 19 Chromium/Tone cases, 13 bit-exact, rest ≤ −141 dB |
| DrumEngine sink rules (release on every target, sink switch) | ✅ | [phase 3](rune-port/phase-3-engine.md); mute + sample preview come with the UI |
| Factory kit voices (16) | ✅ | [phase 2](rune-port/phase-2-dsp.md): seeded Chromium capture with real Tone, 16 voices × 2 rates, correlation 1.000000; random-noise statistics within 3σ |
| Classic machines (808/909/707/606/Linn/CR-78-style) | ✅ | [phase 2](rune-port/phase-2-dsp.md): 97 voices bit-exact vs TS |
| Slicer (transients, equal) + chop to pads | 🟡 | maths ✅ ([phase 1](rune-port/phase-1-model.md): slicer.test.ts ported, chop golden); UI in phase 7 |
| Synth presets 0–40 + fallback piano | ✅ | [phase 4](rune-port/phase-4-instruments.md): bank verbatim, `mergePresetParams` (legacy keys), `presetAt`; all 39 non-sampler presets + `FALLBACK_PIANO` vs the app's `PolyInstrument` in Chromium at −103.6 to −146.6 dB (worst pluck-bass −103.6, worst block −88.9 dB); live `set()` −119.9 to −135.8 dB, applied from patches prepared on the control side (no audio-thread allocation, [review](rune-port/review-phase-4.md)). **User decision pending:** the kept bug that `SubtractiveVoice` never applies oscA/oscB semi/fine at build (warm-pad oscB −12 st, saw-lead +7 ct silent in the shipping app) |
| Voices: subtractive, FM, pluck, sampled piano; allocator, mono/legato | ✅ | [phase 4](rune-port/phase-4-instruments.md): voices in the preset captures above; `voiceAllocator` / `instruments` tests ported; sampled piano −126 to −141 dB (59/60 decoded channels bit-identical), FM fallback until decoded. Deviations: non-integer notes rounded, deterministic noise seed, fixed 160-source piano pool, polyphony capped at 128, pluck dampening clamped to the stable range (the app goes non-finite above `sr/π`), up to 16 ringing retirees per slot on rapid preset switches (then reclaim / 5 ms fade), Stop releases at the Stop time (TS: +100 ms) |
| Character chain | ✅ | [phase 4](rune-port/phase-4-instruments.md): the app's `CharacterChain` in Chromium, −80 to −198 dB (chorus presets −80 to −95 dB); one `CharacterParams` type shared with the presets; wired after every track / live instrument (`SynthPlayer`) |
| Track FX chain (filter+LFO, distortion, EQ3, compressor, chorus, delay, reverb, pan, volume, meter) | ✅ | [phase 4](rune-port/phase-4-instruments.md) (effects): the app's `createTrackAudioNodes` + `applyTrackEffect` / `resetTrackEffects` in Chromium vs the engine core, 6 cases −88 to −102 dB; DynamicsCompressor 18 cases (16 at −196 dB), ConvolverNode 5 cases −106 to −122 dB; `Tone.Meter` semantics. Deviation: mute stays a flag (`setTrackVolume` un-mutes in Tone) |
| Live FX chain + master bus (soft clip, master volume) | ✅ | [phase 4](rune-port/phase-4-instruments.md): a fresh `AudioEngine` + its setters (rewire, frozen stages) in Chromium, 5 cases −74 to −118 dB; master fader 30 ms ramp; legacy CC map + pickup |
| Automation playback | ✅ | [phase 4](rune-port/phase-4-instruments.md): the app's `scheduleAutomation` (every parameter, hidden lane, reset mid-way) in Chromium vs the core's transport, −101 dB; export has no automation (TS quirk kept) |
| Linear recording (Song), punch mode | 🟡 | engine side ✅ ([phase 3](rune-port/phase-3-engine.md) drums; [phase 4](rune-port/phase-4-instruments.md): captured synth notes play back through the track synth, the live synth plays while recording); the keyboard → capture wiring comes with MIDI (phase 5) |
| Audio tracks / audio regions playback | 🟡 | [phase 3](rune-port/phase-3-engine.md): players via BufferPlayer, B3 in-place reload; UI in phase 8 |
| WAV export | 🟡 | vs Electron's real export ([phase 3](rune-port/phase-3-engine.md), [phase 4](rune-port/phase-4-instruments.md)): drum projects −86.5 dB; Ambient Pad and EDM Drop templates −88.4 / −90.9 dB; keys + sampled piano + mono bass −84.8 dB; synth + drum tracks with every track effect −70.9 dB (`Tone.Offline` clock modelled; Tone double-fire bug not replicated). Known deviations (asserted ceilings −11 / −24 dB; investigation in progress): pluck tracks (−12 dB; the app's own pluck export is nondeterministic) and saw-lead (−26 dB, cause not found). Every case also has a worst-block ceiling and must be finite and non-silent |
| MIDI export | ✅ | [phase 3](rune-port/phase-3-engine.md): byte-identical golden (17 cases) |

| Audio file decoding + resampling (wav/aif/flac/mp3/ogg/m4a) | 🟡 | [phase 2](rune-port/phase-2-dsp.md) + [review fixes](rune-port/review-phase-2.md): WAV/FLAC/MP3 bit-identical to Chromium on every channel, AAC (incl. fragmented) −130 dB, Vorbis −103 dB or better, damaged files handled like Chromium, fuzz-hardened. Opus unsupported; AIFF decodes (Chromium can't) |
| Pad buffer playback at any device rate (AudioBufferSourceNode semantics) | ✅ | BufferPlayer max error 0 vs Chromium (71 cases), wired into SamplerKit + audio regions ([phase 3](rune-port/phase-3-engine.md)) |
| bufferCache contract (pack generations, invalidation, in-flight dedup, reversed cache, preview LRU, missing-pack errors) | ✅ | [phase 3](rune-port/phase-3-engine.md): `PadBufferCache`, ported loader/packSources runtime cases |

| Audio output device (cpal), buffer size, device change/loss | 🟡 | [phase 3](rune-port/phase-3-engine.md) + [review fixes](rune-port/review-phase-3.md): probe on this Mac (MiniFuse 4, 5.8 ms); fixed context rate + resampling when the device differs (tempo exact at 44.1/48/96k in tests); null fallback when no device. Real-hardware device switching not yet verified; settings UI in phase 9 |

## MIDI and controller

| Feature | Status | Evidence / notes |
|---|---|---|
| Device list, selection, hot-plug | ❌ | |
| Routing modes (auto / keys / split), sustain, all-notes-off | ❌ | |
| Controller profile (pad map + CC map), default MPK Mini profile | 🟡 | logic ✅ ([phase 1](rune-port/phase-1-model.md): `controller` golden, byte-identical storage JSON); MIDI I/O in phase 5 |
| Pad learn (single + sequence wizard), knob learn | 🟡 | state machine ✅ ([phase 1](rune-port/phase-1-model.md): 36 learn sessions golden); dialog in phase 9 |
| KnobRouter + soft takeover, value curves | 🟡 | logic ✅ ([phase 1](rune-port/phase-1-model.md): `knobs` golden ≈47k); live wiring in phase 5 |
| MIDI monitor (Dev view) | ❌ | |

## Packs

| Feature | Status | Evidence / notes |
|---|---|---|
| Built-in classic kits page | ❌ | |
| Catalog fetch, validation, merge, bundled fallback | 🟡 | validation + merge ✅ ([phase 1](rune-port/phase-1-model.md): `packs_validate` golden incl. real catalog); fetch in phase 6 |
| Installer: staging, resume, retries, size+sha256, atomic swap | ❌ | |
| Crash recovery (.old-*, .tmp-*), pause on quit | ❌ | |
| Uninstall, in-use warning, buffer invalidation + in-place reload | ❌ | |
| Sound previews (remote + local), kit preview | ❌ | |
| Get & load intent (bound to track/kit/session; cancel/defer rules) | ❌ | |
| Missing / withdrawn pack UI | ❌ | |

## UI

| Feature | Status | Evidence / notes |
|---|---|---|
| Start screen | ❌ | |
| Top bar (project, undo/redo, Beat/Song, live readout, MIDI device, instrument, volume, full screen) | ❌ | |
| Sidebar (modes, new/open/save, settings) | ❌ | |
| Beat view: pad grid (4×4 / 2×8, flashes, drag/drop, context menu) | ❌ | |
| Beat view: pad editor (trim waveform, knobs, mode, reverse, choke, chop) | ❌ | |
| Beat view: step sequencer (paint, velocity, keyboard, playhead) | ❌ | |
| Beat view: transport (play, record, BPM, bars, quantize, swing, count-in, overdub, metronome, repeat, export) | ❌ | |
| Beat view: drum track menu, kit menu, missing packs, pending load chip | ❌ | |
| Computer-keyboard pads | ❌ | |
| Sample browser | ❌ | |
| Song view: timeline, tracks, regions, markers, loop, groups, automation, track FX | ❌ | |
| Piano roll editor | ❌ | |
| Mixer | ❌ | |
| Instrument dock, on-screen keyboard, routing selector | ❌ | engine side: the live instrument API (`engine.live()`, [phase 4](rune-port/phase-4-instruments.md)) |
| Synth editor, live FX panel, instrument selector | ❌ | |
| Export dialog | ❌ | |
| Template picker | ❌ | |
| Packs page + pack detail | ❌ | |
| Samples view (edit, process, generate) | ❌ | |
| Settings view | ❌ | |
| Dev view (MIDI monitor) | ❌ | |
| Command palette + all global shortcuts | ❌ | |
| Controller setup dialog | ❌ | |

## App shell / platform

| Feature | Status | Evidence / notes |
|---|---|---|
| Window (min size 1360, title, full screen) | ❌ | |
| Close guard + unsaved-changes dialog | ❌ | |
| Recent projects, autosave/localStorage restore | ❌ | |
| Auto-updater | ❌ | Rune has no updater. Decided: phase 10 ships check + open-release-page (🟡 target); see plan §5 |
| CSP, navigation/window-open guards, trusted-sender IPC | N/A | no web content in a native Rune app; remaining guarantees listed in plan §5 |
| Packaging (dmg/zip/deb/rpm/squirrel) | ❌ | |
