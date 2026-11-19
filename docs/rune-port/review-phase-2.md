# Phase 2 review (commit 2ebb2fe): findings and fixes

| Reviewer | Code quality | UI/UX | Parity | Verdict |
|---|---|---|---|---|
| GPT-6.1 Sol | 7 | 7 | 7.5 | Not yet: fix the decoder failures and the classic-voice playback path first |
| Fable 5.2 | 7 | 8 | 8.5 | Solid for the engine once decode hardening (H1) is done |

Both reviewers re-ran the Chromium captures through the app's real `loadPadBuffer` / `sampleManager.loadFromPath`. They confirmed the factory reproduction at 44.1, 48 and 96 kHz (Sol); Fable extended it to 22.05–192 kHz, where every rate is −103 dB or better except `perc` at 192 kHz (−87.7 dB). Both confirmed classic bit-exactness and regenerated every golden fixture without diff.

From phase 3 on, the second reviewer is Opus 5.5 instead of Fable (the user's request).

| Finding | Fix | Evidence |
|---|---|---|
| Decoder panics (zero-rate WAV/AIFF, isomp4 unwraps, overflows) and multi-GB allocations from ≤60 KB inputs (both, High) | Header checks, Chromium's rate/channel limits, `catch_unwind` boundary, frame/packet budgets, vendored isomp4 + metadata patches | `tests/decode_fuzz.rs`: 20k cases in CI, 2×1M ignored; no escaping panic; peak heap 9.1 MB |
| Stereo Vorbis right channel wrong; tests compared channel 0 only (both, High/Medium) | Vendored symphonia-codec-vorbis: inverse coupling treats +0.0 as positive (FFmpeg's behaviour), no ±1 clamp. Every channel compared for every file | error 1.2 → 2.4e-7; inverting channel 1 fails the test |
| Fragmented M4A decoded as empty (Sol, Medium) | `mdhd` duration 0 means unknown; length taken from the `trun` durations | 14,254 frames, equal to Chromium |
| Classic voices at non-44.1 kHz devices: no measured path (both, Medium) | `abs_node::BufferPlayer`, an exact port of Chromium's AudioBufferSourceNode rendering | 71 cases, max error 0, including through the real `SamplerKit.trigger` |
| Weak CI: factory tests passed with an inverted crash tail; classic checked only sampled points (both, Medium) | Factory checked per block over the full length at 3 rates; classic checked by full-length digest | Strictness demonstrated by the inverted crash, an inverted kick sample and a 1-ulp classic change, each of which now fails |
| NaN/Inf decoded samples (Fable M4) | Matches Chromium (passed through); `sanitize_non_finite` for the engine boundary | per-channel non-finite counts compared |
| Truncated/corrupt-file salvage behaviour (Fable Low) | Matched to a Chromium capture of `testdata/edge` | one deviation: a corrupt MP3 frame (Rust returns a bit-exact prefix) |
| `voices.rs` 1771 lines (both, Low) | Split per machine | largest non-test file 550 lines |
| Per-render allocations and powf, lock poisoning, no factory cache, loose `AudioBuffer` API, decode copy (Fable Low) | `LazyLock` curves, precomputed gain, poison-tolerant cache, `cached_factory_voice`, validated `AudioBuffer` + `SharedBuffer`, `decode_vec` | |
| bufferCache contract not yet covered (both) | Assigned to phase 3, as a parity-table row | |
| Opus in `.ogg` now fails (Fable UI/UX) | Kept as a deviation; the UI must show a clear error (phase 9) | |
