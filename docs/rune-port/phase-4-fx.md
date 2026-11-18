# Phase 4 (FX): effect chains, automation, master bus, meters vs the Electron app

**Scope** (`rune/crates/mdaw-engine/src/fx/`, wired in `core/` and `engine/`):
- The per-track strip of `createTrackAudioNodes`: Filter (+LFO) → Distortion (2× oversampled) → EQ3 → [Compressor] → Chorus → FeedbackDelay → RoomReverb → Panner → Volume → Meter. It follows `applyTrackEffect` (setTrackEffect), `resetTrackEffects` (every play start, removeAutomationLane), `setTrackVolume` / `setTrackPan` / `setTrackMute`.
- Track automation (`scheduleAutomation`): every point becomes a callback in the slot's schedule and fires on the core's transport, loop passes included.
- The live synth bus of `AudioEngine`: its setters, `rewire()`, volume and mute, and the legacy MIDI CC map with pickup (`handleCC`).
- The master bus fader (`setMasterVolumeDb`, 30 ms ramp) and `Tone.Meter` (`getTrackLevel`, `getMasterLevel`).
- The Chromium nodes these are built from:
  - `DynamicsCompressorNode` and `ConvolverNode` (partitioned, zero latency, allocation-free).
  - `DelayNode` (k-rate and a-rate paths), `BiquadFilterNode`, oversampled `WaveShaperNode`, `OscillatorNode` LFOs.
  - Tone's `Param` / `Signal` semantics: units, `rampTo`, the `overridden` flag.

**Not in this part:** presets 0–40, voices, the allocator and the character chain (the phase-4 instruments work). Synth tracks and the live bus have no instrument until those land. `ArrangementEngine::set_live_instrument` is where the live voice plugs in.

**Tests:** 258 engine lib, 2 allocation-guard and 1 host-rate tests; 798 in the workspace. Clippy (`-D warnings`) and `fmt --check` are clean. The allocation guard now covers:
- strips with every effect on, and every strip op (apply, reset with the LFO and compressor toggling, volume, the forced automation pre-pass);
- automation for all ten parameters on the transport;
- the live bus with every setter, both rewire directions, and live hits through it;
- the master fader ramp.

## Parity evidence

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

## Chromium / Tone behaviour found and modelled

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

## The three findings

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
