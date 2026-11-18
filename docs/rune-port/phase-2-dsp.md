# Phase 2: offline DSP (`rune/crates/mdaw-dsp`) vs the Electron app

**Scope:**
- drum synthesis: the factory kit and the classic machines
- the RoomReverb impulse response
- the master-bus soft clip and the drum shaper curves
- the WAV encoder
- audio decoding and resampling

Tests (after the review fixes): 82 unit + 2 integration in `mdaw-dsp`, plus a 20,000-case decode fuzz test in CI (425 more in `mdaw-model`). Clippy (`-D warnings`) and `cargo fmt --check` are clean.

## Method

- **Pure TS code** (the classic machines, the reverb IR, the soft-clip curve, the WAV encoder) is run by vitest golden scripts (`rune/parity/ts/*.golden.ts`). Rust must reproduce it bit for bit.
- **Code built on Tone.js / Web Audio graphs** (the factory kit) and **browser decoding**: reference data is captured from the real Electron renderer code running in headless Chromium with real Tone.js, not the mock. The capture scripts are in `rune/parity/chromium/` and drive the app's own `loadPadBuffer` / `sampleManager.loadFromPath`.

## Results

| Area | Comparison | Result |
|---|---|---|
| Classic machines: 6 machines, 97 voices (`classic/dsp.ts`, `voices.ts`) | CI checks a full-length FNV-1a digest per voice against the TS render | **bit-exact** (a 1-ulp change fails) |
| Classic DSP primitives (rng, noise, env, osc, polyBLEP square, metal, partials, 16 biquad responses, DC block, lofi/μ-law, drive, finalize) | Golden vectors | max error 0 |
| Factory kit: 16 voices × 44.1, 48 and 96 kHz (`drumKit.ts` through `bufferCache` Tone.Offline + trimTail) | Seeded Chromium capture with real Tone; CI checks full length per 1024-sample block | lengths exact; whole render −115.5 to −130.4 dB below the signal (limit −90); worst block −84.9 dB (limit −80); Δpeak 0.000 dB; Δenv ≤ 0.005 dB (limit ±0.05). Inverting the crash after sample 2048 now fails |
| Factory kit with real `Math.random` noise | 24 Chromium renders vs Rust renders per voice and rate (44.1/48/96 kHz) | mean length, RMS, peak and centroid within 4 standard errors for every voice |
| RoomReverb IR | 6 option sets vs TS | bit-exact |
| Master soft-clip curve (4096 points); kick and snare tanh shapers | vs the TS function and the curves read from Chromium | bit-exact |
| WAV encoder (`wavEncoder.ts`) | mono/stereo, clipping, odd lengths, 44.1k/48k | byte-identical |
| Decoding + resampling: WAV (u8/s16/s24/s32/f32), FLAC 16/24, MP3 (gapless priming) | vs Chromium `decodeAudioData`, **every channel**, full-length digest + per-block energy | bit-exact on all channels at native and context rate (Chromium's asymmetric int→float scaling and its SincResampler ported) |
| AAC m4a (ffmpeg `elst`, afconvert `iTunSMPB`, **fragmented** `empty_moov`) | same | lengths exact, −130 to −134 dB |
| Ogg Vorbis (libvorbis and ffmpeg's native encoder), stereo | same, both channels | −103 to −140 dB. Vendored fix in symphonia-codec-vorbis: inverse coupling now treats +0.0 as positive (as FFmpeg does), and overs are no longer clamped. Before the fix, ffmpeg-vorbis right channels were off by up to 1.2 |
| Damaged files (truncated WAV/MP3/M4A, corrupt Ogg page, bad FLAC, zero or out-of-range rates/channels) | vs Chromium on `testdata/edge/` | same accept/reject decisions and the same salvaged frames. The one exception is a corrupt MP3 frame: Chromium conceals it and continues, Rust stops and returns a bit-exact prefix (deviation) |
| Decoder robustness | seeded mutation/truncation fuzz (20k cases in CI; 2×1M ignored) | no panic escapes (`catch_unwind` boundary + header checks + frame budget + vendored isomp4/metadata fixes); peak heap ≤ 32 MB (9.1 MB observed), down from up to 4 GB |
| Pad playback (`AudioBufferSourceNode`: rate = buffer rate ÷ context rate × 2^(pitch/12), offset, duration, sub-sample start, linear interpolation) | 71 Chromium cases at 44.1/48/96 kHz, 8 of them through the real `SamplerKit.trigger` | **max error 0** (`abs_node::BufferPlayer`). Pre-resampling instead would be wrong by −13.9 dB, so the engine never pre-resamples pad buffers |

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
| Corrupt MP3 frame | conceals a few frames and continues | stops; the output so far is a bit-exact prefix of Chromium's | |
| NaN/Inf samples in float WAVs | passed through | passed through (same); `sanitize_non_finite` is exported, and the engine must apply it at its boundary | |
| AIFF / AIFF-C / ALAC / f64 WAV | Chromium **can't decode** them, although the app accepts `.aif`/`.aiff` | decoded | Rust does more than Electron here |
| Ogg Opus | decoded | **not supported** by symphonia | 🟡: rare in drum packs; none in the catalog |
| Worker for the classic machines | Web Worker + main-thread fallback | plain function + a thread-safe once-per-voice cache, called from worker threads | All 97 voices render in about 139 ms (release build) |

## Contract for the engine (phase 3)

- Play every pad and sample buffer at its own rate through `BufferPlayer`. Never pre-resample.
- Render in 128-frame blocks aligned to multiples of 128, as Chromium does. Cut voices with the envelope at `time + realDuration`.
- Call `sanitize_non_finite` on decoded buffers at the engine boundary.
- Decode, render and resample on worker threads only: 96 kHz factory renders take about 0.5 s cold, and `prewarm_wavetables` exists for that. A file's rate conversion happens at decode (`decode_file_at_rate`), as Chromium does.
- Release builds must keep `panic = "unwind"` so the decode boundary works.
- The Chromium reference is Playwright's Chromium 153; Electron 44's Chromium may differ slightly.

Capture commands: `node scripts/ui-snap.mjs [--mode daw] --steps rune/parity/chromium/<capture-decode.mjs | capture-playback.steps.mjs | capture-vorbis.mjs | capture-factory.steps.mjs> --out <png>`. The decode fuzz test scales up with `FUZZ_CASES=… cargo test -p mdaw-dsp --test decode_fuzz -- --ignored`.

## Not covered yet

- The bufferCache contract (pack-generation keys and invalidation, in-flight dedup, reversed-buffer cache, 8-entry preview LRU, missing-pack errors) goes in the engine's buffer layer (phase 3).
- The metronome click and its live parameters belong to the engine (phase 3).
- The convolution reverb and the 200 Hz high-pass send are documented as data in `reverb_ir.rs`; the engine implements them (phase 4).
- The SampleProcessor DSP (pitch shifter, noise-IR reverb) is phase 9.
