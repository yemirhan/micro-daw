# Phase 4: instruments, effects, live synth and WAV export vs the Electron app

This is the phase-4 comparison document. It merges the FX report (formerly `phase-4-fx.md`) with
the instrument work (presets, voices, sampled piano, character chain) and their integration into
the engine (synth tracks, the live synth, the WAV export).

**Scope** (`rune/crates/mdaw-engine/src/`):
- `synth/`: the preset bank (0–40 + `FALLBACK_PIANO`), override merging (`mergePresetParams`,
  legacy keys), the voice allocator and mono note stack, `PolyInstrument` with the Subtractive /
  FM / Pluck voices rendered like Chromium renders their Tone.js graphs.
- `piano/`: `SampledPiano` (Tone.Sampler over the bundled Salamander set, decoded like Chromium),
  its allocator and the FM fallback until the samples are decoded (`PianoSlot`).
- `character/`: `CharacterChain` (drive, presence, tone, tremolo, chorus, gain).
- `player.rs`: **`SynthPlayer`**, the complete instrument a track or the live keyboard plays
  (`createSynthFromPreset`): voices → `CharacterChain(character, gain)`, or for sampler presets
  `SampledPiano` (sources + fallback → its own chain → ×2 → the preset's chain). One type for
  the character parameters (`character::CharacterParams`, re-exported by `synth::presets`);
  `PianoParams` is a view of `SamplerPreset` (`From`, same serde form).
- `fx/`: the per-track strip, the live synth bus, automation, master bus, meters (see "Effects").
- Wiring: `core/` (synth notes with `triggerAttackRelease` at the tempo at event time; live
  synth commands; retiring instruments), `engine/tracks.rs` (synth tracks), `engine/live.rs`
  (the live instrument API), `engine/export_wav.rs` (synths in the export).

## Wiring summary

- **Synth tracks.** `TrackInstrument::Synth` = `SynthPlayer` built from
  `resolve_preset(presetIndex, synthOverrides)` (an unknown index is preset 0, like `presetAt`),
  then the track strip. Sampler presets read the engine's shared `PianoSlot`, which starts
  decoding the first time a sampler preset is built (or at `live().start()`); until then the FM
  fallback plays, note-offs routed back to it. `setTrackInstrument` / `setTrackSynthOverrides`
  emit `TrackAudioRebuilt`, which rebuilds the instrument inside `withPlaybackRestart`. Pattern
  notes call `trigger_attack_release(note, durationBeats × secPerBeat(at event time), time,
  velocity)` (gate ≥ 24 ms), Stop releases every synth.
- **Live instrument** (`engine.live()`, `LiveSynthApi`, ≙ `AudioEngine`): `start()` (idempotent,
  builds preset 0 and starts the piano decode), `note_on` / `note_off` / `release_all` at the
  next render quantum + 4 ms (`LIVE_OFFSET`; note-off honoured while muted; ignored before
  `start`, like the TS), `set_preset(i)` (integers in the bank only; resets the overrides; the old
  instrument gets `releaseAll` at Tone's `now()` = +100 ms and keeps ringing on the bus until it
  is retired 1200 ms later, `RETIRE_MS`, through the garbage ring), `set_synth_param('a.b' | key,
  value)` (merged into the overrides and `set` on the running instrument, no rebuild),
  `synth_overrides`, `current_preset`, `preset_index`, `active_voice_count` / `held_notes`
  (published by the instrument after every quantum), `is_piano_ready`, `mute` / `unmute` /
  `set_volume` (30 ms ramp, default −12 dB). It plays through the live FX bus (`fx/live.rs`) and
  the master bus. The MIDI thread can send the same notes (`LiveInput::synth_note`).
- **Realtime.** Note events, pattern playback, preset switches (`ReplaceSynth`) and rendering
  never allocate on the audio thread (alloc guard: synth tracks of every engine, the sampled
  piano, the live synth with notes from both producers, `releaseAll`, a preset switch and its
  retirement). **One exception:** `setSynthParam` (`Command::SynthSet`) re-derives voice
  parameters on the audio thread (preset clones, wave tables, noise buffers) and may allocate
  there; it only comes from a user edit, never from playback.
- **WAV export.** Unmuted synth tracks render through the same `SynthPlayer` + strip as live,
  after the piano samples are decoded when an unmuted track uses a sampler preset (the TS
  `loadPianoSamples`; a failed decode leaves the FM fallback). No automation (TS quirk kept);
  master bus at 0 dB. The instruments run in `Tone.Offline`'s clock model
  (`SynthPlayer::new_offline`, below).

## Tests

- `cargo test --workspace`: 906 tests (engine: 365 lib + 3 allocation-guard + 1 host-rate), all
  green; `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all --check`
  clean.
- New: `player_tests.rs` (chains, the sampled-piano paths, `PianoParams` view, live `set`,
  status, allocation-free note events incl. the sampled piano); `engine/tests_synth.rs` (synth
  tracks through the engine: notes, tempo at event time, rebuilds while playing, Stop, mute, a
  Song recording played back; the live instrument: the non-FX cases of
  `src/services/AudioEngine.test.ts` and the engine side of `src/test/stuckNotes.test.tsx`, checked
  on the rendered audio); the allocation guard's synth case
  (`tests/core_alloc_guard.rs::synth_tracks_and_the_live_synth_do_not_allocate`).
- Ported earlier in the phase: `voiceAllocator.test.ts`, `presets.test.ts`, the PolyInstrument /
  SampledPiano parts of `instruments.test.ts`.

## Parity evidence: instruments

Captures are offline renders in headless Chromium 153 with the app's real Tone.js code
(`node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/<script>`), compared over the
whole render (error-to-signal energy) and per 1024-frame block (`stats.mjs`).

| Area | Comparison | Result |
|---|---|---|
| Synth presets + voices (`synth/parity_tests.rs`) | The app's `PolyInstrument` tapped before its CharacterChain, every non-sampler preset (39) + `FALLBACK_PIANO`, scripted notes / re-strikes / steals / releases (`capture-synth.steps.mjs`) | −103.6 to −146.6 dB whole render; worst pluck-bass −103.6 dB (worst block −88.9 dB), wobble-bass −107.9 / −76.9 dB, acid-bass −106.0 dB; every block's energy within 0.001 dB |
| Live `set()` | 4 cases of parameter changes on sounding voices | −119.9 to −135.8 dB |
| Sampled piano (`piano/parity_tests.rs`) | The app's `SampledPiano` with the real Salamander samples: roots, between roots, velocity curve, sustain, re-strike, overflow, presets | −126 to −141 dB; 59 of 60 decoded channels bit-identical (Fs7 R: last 7 frames −136 dB) |
| Character chain (`character/parity_tests.rs`) | The app's `CharacterChain` on a test signal, every stage and the preset characters | −80 to −198 dB (chorus presets −80 to −95 dB) |
| Assembled instrument (`player_tests.rs`) | `SynthPlayer` = voices then the chain, sample-identical to running the two parts; sampler presets = fallback → its chain → ×2 → preset chain | exact |

## Parity evidence: WAV export with synth tracks

`ExportService.exportWav` in Chromium (Tone.Offline, real track chains, master bus) vs
`export_wav`, 48 kHz, 16-bit, full-render error over block evidence
(`capture-export.steps.mjs`, `engine/tests_export.rs`). Limit −60 dB. Tone's doubled Part
events are removed in the reference the documented way (synth `triggerAttackRelease` calls are
logged and de-duplicated like the SamplerKit triggers). Synth noise: Tone's noise buffers from
the synth capture's seeds and `Math.random` seeded with the Rust `DEFAULT_NOISE_SEED` before each
export (exact when one instrument draws noise).

| Project | Content | Full render (worst block) |
|---|---|---|
| factory_basic / classic_swing / two_tracks | the phase-3 drum projects | −86.5 / −86.5 / −86.5 dB |
| tpl_ambient_pad | "Ambient Pad" template as `loadFromTemplate` installs it: supersaw-pad chords, warm-pad, drums, 70 BPM, 30 s | **−88.4 dB** (−53.8) |
| tpl_edm_drop | "EDM Drop" template: drums + analog-bass, 128 BPM | **−90.9 dB** (−55.1) |
| keys_piano | fm-keys chords with `synthOverrides` (index, gain, character), rhodes, the **sampled grand piano** (samples decoded, re-strike, overlaps), reese-bass mono legato | **−84.8 dB** (−54.1) |
| fx_tracks | warm-pad (lowpass + sine LFO, chorus, reverb), square-lead (bandpass + triangle LFO, distortion, EQ, delay, compressor), factory drums (highpass + square LFO, chorus, delay, reverb, compressor); 13 doubled Tone events removed | **−70.9 dB** (−47.8) |
| pluck_harp *(known deviation)* | harp (pluck), 16 notes | −12.0 dB (−1.7): see below |
| saw_lead *(known deviation)* | saw-lead, 7 notes | −25.9 dB (−13.8): see below |

Every compared project's WAV length is exact.

### `Tone.Offline` behaviour found and modelled (export only)

`Tone.Offline` runs Tone's whole clock (every Part callback, every scheduling call) **before**
Chromium renders a frame. The live app never sees this (its look-ahead keeps scheduling ahead of
the audio), so the export needs its own model (`PolyInstrument::set_offline`, `SynthEngine::
prepare_offline`):
- **Tone's `now()` while scheduling** is the clock's block time, accumulated in f64
  (`_currentTime += 128/sr`): an event at `t` is handed out at the first block time after `t`, so
  Tone clamps the voices' source starts / stops to it (oscillators start up to 128 frames after
  the note; an event at 0 starts at frame 128). The sampled piano is not affected
  (`ToneBufferSource` starts at the event time). Without this model the templates were at +2 dB.
- **Every node exists when rendering starts:** the instrument LFO runs from frame 0 (live, it
  starts when the first voice connects it); a new voice's signals start in the block of that
  clock time, late first frame by the usual Chromium rule of that block (one-frame offset on some
  blocks, e.g. frame 905216 at 48 kHz). Ambient Pad went from −40.7 to −88.4 dB with this.
- **Pluck voices:** the comb's lowpass is re-created at every attack and the old one is
  disconnected on the next clock tick, i.e. before rendering: each voice plays every note through
  the lowpass of its LAST attack; an attack's `resonance.cancelScheduledValues(t)` removes the
  previous note's release ramp (stored at its end time) when it ends after `t`, so that note rings
  at full feedback until the attack (same for a steal's 15 ms fade); the comb worklet has run
  since frame 0 (its write head position, which decides where its one-frame wrap glitch falls).
  These are modelled with a pre-pass over the track's notes on a scratch instrument.
- **Tone bug neutralized in the capture** (like the doubled Part events): Tone creates
  AudioWorklet nodes (the pluck comb) in a promise callback that only runs when the clock loop
  yields (every second of audio, and only once the module has loaded, which takes real time).
  Pluck notes handed out before that keep their comb on the placeholder params (delay 0.1 s,
  wrong feedback): the app's own export of the first seconds of a pluck track is wrong and
  **varies from run to run**. The capture loads the module first and creates each node right
  after the attack that built the voice (what the realtime app does).

### Known export deviations (investigated, not closed)

- **Pluck (`pluck_harp`, −12.0 dB).** Up to 2.7 s the harp matches at −74 to −85 dB; from then on
  two effects remain. (1) Tone hands the last note (beat 7.5) out one block EARLY (Tone's
  `currentTime` 4.016 s for an event at 4.0179 s, so its sources are not clamped): Tone's
  transport tick iteration (`TickSource.forEachTickBetween`, accumulated tick durations, rounded
  tick numbers) does not always agree with the block in which the event's time falls; this is
  not modelled. (2) Note 11 (a fresh voice at 2.946 s) differs from its first echo on. The
  realtime pluck matches Chromium at −103.6 to −146.6 dB; this is the export path only, and the
  app's own pluck export is nondeterministic (above).
- **saw-lead (`saw_lead`, −25.9 dB).** One note of preset 1 is −18 dB off from its onset, the
  error lying at 4–8 kHz; not the delayed vibrato, not the oscB detune, not the track FX (probes
  with each effect alone and dry give the same error), not a time offset. Other subtractive
  presets in the export (supersaw-pad, warm-pad, analog-bass, square-lead with all effects) match
  at −71 to −91 dB, and saw-lead matches live at < −100 dB. Cause not found; the fx project uses
  square-lead.

## Effects (merged from the FX report)

**Scope** (`fx/`, wired in `core/` and `engine/`):
- The per-track strip of `createTrackAudioNodes`: Filter (+LFO) → Distortion (2× oversampled) →
  EQ3 → [Compressor] → Chorus → FeedbackDelay → RoomReverb → Panner → Volume → Meter. It follows
  `applyTrackEffect` (setTrackEffect), `resetTrackEffects` (every play start,
  removeAutomationLane), `setTrackVolume` / `setTrackPan` / `setTrackMute`.
- Track automation (`scheduleAutomation`): every point becomes a callback in the slot's schedule
  and fires on the core's transport, loop passes included.
- The live synth bus of `AudioEngine`: its setters, `rewire()`, volume and mute, and the legacy
  MIDI CC map with pickup (`handleCC`).
- The master bus fader (`setMasterVolumeDb`, 30 ms ramp) and `Tone.Meter` (`getTrackLevel`,
  `getMasterLevel`).
- The Chromium nodes these are built from: `DynamicsCompressorNode` and `ConvolverNode`
  (partitioned, zero latency, allocation-free), `DelayNode` (k-rate and a-rate paths),
  `BiquadFilterNode`, oversampled `WaveShaperNode`, `OscillatorNode` LFOs; Tone's `Param` /
  `Signal` semantics (units, `rampTo`, the `overridden` flag).

### Parity evidence: effects

Captures are offline renders in headless Chromium 153 (`node scripts/ui-snap.mjs --no-start --steps rune/parity/chromium/<script>`), compared over the whole render with the block evidence of `stats.mjs`. Limits: full-render error ≤ −60 dB, and per 1024-frame block above −90 dBFS, energy within 0.5 dB.

| Area | Comparison | Result |
|---|---|---|
| DynamicsCompressor (`fx/compressor.rs`) | Native node, 18 cases: drums / sines / steps at 48 and 44.1 kHz, every parameter and its clamps, 4 cases with k-rate changes mid-render (`capture-compressor.steps.mjs`, `fx_compressor.json`) | 16 cases at −196 dB (bit-exact apart from f32 rounding); `t60_r20_k0_a0` −131.5 dB, `changes` −128.3 dB; reduction reads ≤ 8e-7 off. Note: `sin`, `asin`, `exp` and the attack `powf` are computed in f64 then rounded |
| ConvolverNode (`fx/convolver.rs`, `fx/fft.rs`) | Native node with the app's RoomReverb IR (`normalize`), 5 cases: 48 / 44.1 / 96 kHz, decay 0.5 with no pre-delay, a mono IR; impulses, noise, sines and a gap longer than the IR (`capture-convolver.steps.mjs`, `fx_convolver.json`) | −111.1 / −111.9 / −106.4 / −122.5 / −111.2 dB. Cost: 0.26 % of a core per track at 48 kHz (mean 6.8 µs per quantum, p99.9 19 µs); 16 ns when idle |
| Track strip (`fx/track.rs`) | The app's `createTrackAudioNodes` + `applyTrackEffect` / `resetTrackEffects` / node writes, replayed on the engine core (`capture-fx.steps.mjs`, `fx_chains.json`): defaults; every effect on; effects switched on one by one (LFO on, retuned, compressor bypassed, LFO off); 44.1 kHz; mute / volume | −100.7, −100.6, −101.7, −88.2 (44.1 kHz), −94.8 dB (up to the volume-while-muted deviation) |
| Automation (`fx/automation.rs`) | The app's `scheduleAutomation`: all 10 parameters, a hidden lane, ramps, `distortionWet` / `delayWet` pre-pass, a `resetTrackEffects` in the middle; callbacks from the core's transport | −100.9 dB |
| Live bus (`fx/live.rs`) | A fresh `AudioEngine` (`start()`, master bus included) and its setters: defaults; everything on, then each stage out and back in, LFO off, filter type, comp and volume; stages out and in one at a time; late chorus / ping-pong / EQ with filter ramps at 48 and 44.1 kHz | −117.9, −114.8, −114.9, −106.0, −73.5 dB |
| WAV export | The phase-3 Electron export captures (3 drum projects) | −86.5 dB each, up from −63 to −66 dB: now the real strips, with Chromium's convolver normalization |
| Engine wiring (`engine/tests_fx.rs`) | Volume automation through `play()` (ramp, then reset on the next play), a hidden lane, `setTrackEffect` reaching the strip, track volume / mute, master fader ramp, meters (mono, smoothing 0.8 per read, master = track + 2 dB of drive), legacy CCs with pickup | pass |

### Chromium / Tone behaviour found and modelled

The captures pinned these down. Each was measured before it was modelled.

- **ConvolverNode normalization is −58 dB.** Chromium uses `powf(10, −58·0.05)` = 0.0012589, not the spec's 0.00125. The old value played every convolution 0.06 dB quieter, a −43 dB error. Fixed in `mdaw_dsp::reverb_ir::convolver_normalization_scale`. It also moved the WAV export from −63 to −86.5 dB.
- **A bypassed track compressor keeps running.** Its input is disconnected but its output still feeds the chorus, so Chromium processes it on silence and its detector relaxes. Modelling it as frozen gave −7 dB when it was switched back in. It now runs on silence: −101.7 dB.
- **A live stage out of the path is frozen.** It is disconnected on both sides and nobody pulls it: delay lines, the reverb tail, compressor state and LFO phase resume where they stopped. Running it on silence instead gave −10 to −16 dB.
- **Taking a stage out changes its input's channel count, which resets some kernels.** BiquadFilter and WaveShaper kernels are re-initialized: the reverb's send highpass, the EQ crossover, the distortion's oversampler. Without that reset, a reverb put back in was −38 dB off.
- **A stereo effect wired in for the first time mid-render gets a silent right input for one quantum.** This applies to the chorus and the ping-pong delay when they were off at `start()`, because their buses still have one channel. A stage that already ran is not affected (`live_all`, `live_rewire`). Without this rule the first chorus or delay switch-on was −45 / −15 dB off; with it, −106 dB.
- **Changing a BiquadFilter's type resets its state** (`SetType`).
- **DelayNode has two paths.**
  - k-rate: one delay per quantum, f64 read position.
  - a-rate: the NEON/SSE vector path, all in f32 (`pos = f32(w) + (f32(len) − d·sr)`, `s1 + f·(s2 − s1)`), with a buffer of `128 + ceil(max·sr)` frames.
  - The a-rate path is taken when the param has an input (the chorus LFO, ping-pong's delayTime Signal) or changes within the quantum.
- **Feedback cycles run one quantum late.** Finding 1 below.
- **Capture artifact, not app behaviour: Tone caches a phased LFO's `PeriodicWave` per page.** A wave made by an earlier context at another rate runs at the wrong frequency in the next one. The chorus' right LFO ran at 0.7·44100/48000 Hz, which made the 44.1 kHz cases look −12 dB off. The capture now loads a fresh page per case. The app has one context, so it is not affected.

### The three FX findings

1. **Feedback loops are one render quantum longer in Chromium.**
   - Evidence: a 480-frame delay with feedback 0.5 echoes at frames 480, 1088, 1696, 2304… (period 608 = 480 + 128). Raw `DelayNode` cycle in Chromium 153 (a one-off probe page, re-run for this report); the Rust model is pinned by `fx::effects::tests::a_feedback_cycle_repeats_every_delay_plus_one_quantum`.
   - FeedbackDelay and PingPongDelay model it: the cycle reads the previous quantum. The phase-3 disabled-strip model did not, and the delay leaks now match.
2. **Turning the filter LFO off silences a lowpass track (and the live synth).**
   - Connecting the LFO sets `filter.frequency.overridden`, and Tone never clears it. The `frequency.value = cutoff` write after the LFO is disposed then stores 0 Hz.
   - A lowpass or bandpass goes silent, and only the delay / reverb tails after the filter remain. A highpass passes everything.
   - Strip evidence (`strip_apply`, LFO off at 3.3 s): the capture's step RMS falls from −21.6 to −30.5 dBFS while the tails decay, and the last 100 ms reach −41.9 dBFS. The Rust render matches it at −101.7 dB over the whole case (`lfo_off_silences_the_lowpass`).
   - Live evidence (`live_all`, LFO off at 3.0 s; −114.8 dB): the 0.6-feedback delay and 0.5 reverb tails mask it. The `highpass` switch at 3.3 s then gets louder (−20.5 → −18.0 dBFS step RMS), which a 0 Hz cutoff explains.
   - Kept as in the TS. To fix it, rebuild the filter (or its frequency Signal) when the LFO goes.
3. **Setting a track's volume un-mutes it in Tone.**
   - `volume.volume.value = db` replaces the −∞ that `mute = true` wrote. `setTrackVolume` and every `resetTrackEffects` (each play start) therefore make a muted track audible again; its pattern is still not scheduled, but live input and tails are heard. `mute = false` afterwards is then a no-op.
   - Evidence (`strip_mute_volume`): −200 dBFS while muted; −24.1 dBFS after the −6 dB volume write while still "muted"; unchanged after un-muting.
   - **Deviation:** mute is a separate flag here, so a muted track stays silent (`setting_the_volume_unmutes_in_tone_but_not_here`, `volume_and_mute_go_through_the_strip_and_a_volume_change_keeps_mute`).

## Deviations (what the user notices)

**Instruments and live synth:**
- **Non-integer MIDI notes** are rounded (the TS passes them through `midiToNoteName`).
- **Noise is deterministic:** each instrument draws its noise start offsets from its own seeded
  generator (`DEFAULT_NOISE_SEED`); Tone draws them from the page's `Math.random`.
- **Sampled piano:** a fixed pool of 160 sources (the TS stacks without limit); a release
  scheduled before its source's start is silent; the CharacterChain LFOs are exact sines.
- **Live synth:** notes play 4 ms after the next render quantum (the TS: `immediate()` + 4 ms);
  the active voice count / held notes lag the commands by up to one device buffer (they are
  published by the instrument); `isPianoReady` is the engine's own sample slot.
- **Live parameter edits** (`setSynthParam`) are the one audio-thread operation that may allocate.
- **Export:** the pluck and saw-lead export deviations above; Tone's doubled Part events and the
  pluck worklet race are not reproduced (every note plays once, correctly).

**Effects:**

- **Mute** stays muted through volume changes and play starts (finding 3). This applies to tracks and the live synth.
- **Timing:**
  - `chorusDepth` automation is applied at its exact event time; Tone applies it inside a callback that runs up to the look-ahead early.
  - Strip changes apply at the next render quantum, not 100 ms later (the phase-3 timing model).
  - A play start resets the strips at the start quantum; Tone writes them at `now + 0.1`, which is also its start.
- **Master volume persistence** (`micro-daw:masterVolume`) belongs to the settings store. The app reads it and calls `set_master_volume_db`.
- **Meters:** the core publishes the meter window (the older half of the last four quanta, mono). The 0.8 smoothing is applied per `get_*_level` call, like `Tone.Meter.getValue`, so the decay rate depends on how often the UI reads.
- **Kept TS quirks:**
  - export has no automation;
  - hidden lanes are silent;
  - `resetTrackEffects` turns a delay feedback of 0 into 0.3 and a time of 0 into 0.25;
  - an existing LFO is not re-ranged by a reset;
  - the live LFO runs at Tone's default 0..1 Hz range until `updateLfoRange`;
  - live `setFilterCutoff` only re-ranges a running LFO.

## Open decisions

1. **USER DECISION PENDING: `SubtractiveVoice` never applies `oscA` / `oscB` `semi` / `fine` at
   build** (Tone's `omniOptions` carries no detune; only a live `set()` applies them, to the
   voices that exist then). Kept for parity: in the shipping app warm-pad's oscB −12 st and
   saw-lead's oscB +7 ct are silent. Fixing it changes how those presets sound.
2. **Turning a filter LFO off silences a lowpass / bandpass track and the live synth** (FX finding
   2). Kept as in the TS.
3. **`setVolume` un-mutes in Tone** (FX finding 3). The Rust engine keeps mute a separate flag
   (deviation, for tracks and the live synth).

## Not done in this phase

- The two export deviations above (pluck, saw-lead) are investigated but not closed.
- Live parameter edits are not allocation-free (documented exception).
- MIDI input driving the live synth (phase 5) and the synth editor / instrument UI (phase 7).
