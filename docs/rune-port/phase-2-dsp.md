# Phase 2: offline DSP (`rune/crates/mdaw-dsp`) vs the Electron app

**Scope:**
- drum synthesis: the factory kit and the classic machines
- the RoomReverb impulse response
- the master-bus soft clip and the drum shaper curves
- the WAV encoder
- audio decoding and resampling

Tests: 57 in `mdaw-dsp` (plus 425 in `mdaw-model`). Clippy (`-D warnings`) and `cargo fmt --check` are clean.

## Method

- **Pure TS code** (the classic machines, the reverb IR, the soft-clip curve, the WAV encoder) is run by vitest golden scripts (`rune/parity/ts/*.golden.ts`). Rust must reproduce it bit for bit.
- **Code built on Tone.js / Web Audio graphs** (the factory kit) and **browser decoding**: reference data is captured from the real Electron renderer code running in headless Chromium with real Tone.js, not the mock. The capture scripts are in `rune/parity/chromium/` and drive the app's own `loadPadBuffer` / `sampleManager.loadFromPath`.

## Results

| Area | Comparison | Result |
|---|---|---|
| Classic machines: 6 machines, 97 voices (`classic/dsp.ts`, `voices.ts`) | Full-length renders vs TS | **bit-exact**: max sample error 0 for every voice; lengths equal |
| Classic DSP primitives (rng, noise, env, osc, polyBLEP square, metal, partials, 16 biquad responses, DC block, lofi/μ-law, drive, finalize) | Golden vectors | max error 0 |
| Factory kit: 16 voices × 44.1 kHz and 48 kHz (`drumKit.ts` through `bufferCache` Tone.Offline + trimTail) | Seeded Chromium capture with real Tone | lengths exact; peak Δ 0.000 dB; envelope within 0.005 dB; centroid within 0.01 %; correlation of the first 2048 samples 1.000000; full-waveform error −111 to −130 dB below the signal |
| Factory kit with real `Math.random` noise | 8 Chromium renders vs 16 Rust renders per voice and rate | mean length, RMS, peak and centroid agree within 3 standard errors for every voice |
| RoomReverb IR | 6 option sets vs TS | bit-exact |
| Master soft-clip curve (4096 points); kick and snare tanh shapers | vs the TS function and the curves read from Chromium | bit-exact |
| WAV encoder (`wavEncoder.ts`) | mono/stereo, clipping, odd lengths, 44.1k/48k | byte-identical |
| Decoding + resampling: WAV (u8/s16/s24/s32/f32), FLAC 16/24, MP3 (gapless priming) | vs Chromium `decodeAudioData` at a 44.1 kHz context | lengths exact, max error 0 (Chromium's asymmetric int→float scaling and its SincResampler ported) |
| AAC m4a (ffmpeg `elst`, afconvert `iTunSMPB`) | same | lengths exact, max error ≤ 4e-7 |
| Ogg Vorbis, multi-page | same | lengths exact, correlation 0.9999997 |

## How the Web Audio semantics were reproduced

- The factory kit is rebuilt from the Web Audio spec plus two Chrome behaviours that the spec doesn't describe and the captures exposed:
  - A `setTarget` that follows a ramp starts from Chrome's last rendered frame, not from the ramp's end value.
  - Chrome snaps a `setTarget` to its target once it's within e⁻¹⁰, checked every 128 frames.
- Oscillators use Chrome's own band-limited wavetables. Biquads follow the spec, with lowpass/highpass Q in dB. Tone's −24 dB rolloff is two biquads in series.
- Noise:
  - In the Electron app, every render of a noise voice differs, because Tone picks a `Math.random` start offset in a shared noise buffer.
  - Rust uses fixed seeds, and the seeded capture used the same seeds. So the deterministic comparison is exact, and the random-noise comparison is statistical.

## Deviations

| Area | Electron | Rust | Note |
|---|---|---|---|
| Noise voices | random variation per render (tail length ±500 samples, peak ±1 dB) | deterministic per voice | The sound is the same; Rust is reproducible |
| Short single-page Ogg files | Chromium's FFmpeg ends the stream early (e.g. 110 frames lost on a 0.3 s file) | keeps the length the file declares (final granule) | Matches Chromium's behaviour on longer files |
| Vorbis overs | not clamped | clamped to ±1 by symphonia | Fixing it needs a symphonia fork |
| AIFF / AIFF-C / ALAC / f64 WAV | Chromium **can't decode** them, although the app accepts `.aif`/`.aiff` | decoded | Rust does more than Electron here |
| Ogg Opus | decoded | **not supported** by symphonia | 🟡: rare in drum packs; none in the catalog |
| Worker for the classic machines | Web Worker + main-thread fallback | plain function + a thread-safe once-per-voice cache, called from worker threads | All 97 voices render in about 139 ms (release build) |

## Not covered yet

- The metronome click and its live parameters belong to the engine (phase 3).
- The convolution reverb and the 200 Hz high-pass send are documented as data in `reverb_ir.rs`; the engine implements them (phase 4).
- The SampleProcessor DSP (pitch shifter, noise-IR reverb) is phase 9.
