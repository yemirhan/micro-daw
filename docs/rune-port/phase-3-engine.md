# Phase 3: engine (`rune/crates/mdaw-engine`, `rune/crates/mdaw-audio-host`) vs the Electron app

**Scope:**
- The sample-accurate core: clock, tick transport (PPQ 192), schedules, metronome, count-in, note repeat, capture events. It runs in the cpal callback and offline.
- SamplerKit and the buffer cache.
- The control-side `ArrangementEngine`: transport API, Play Pattern, loop recording, linear recording, DrumEngine sink rules, audio tracks.
- Export: WAV and MIDI.
- The cpal output host.

**Tests (after the review fixes):** 213 engine lib + 2 allocation-guard + 1 host-rate tests in the engine, 28 in the host; 753 Rust tests in the workspace. Clippy (`-D warnings`) and `fmt --check` are clean. An allocation guard proves the audio path (core, strips, master bus, SamplerKit, audio regions) never allocates.

## Parity evidence

| Area | Comparison | Result |
|---|---|---|
| SamplerKit (pan, trim, reverse, decay, gate, choke, stop-all, velocity, pending cancel, filter, gain, pitch, voice steal, live setKit; 48 kHz plus 44.1 and 96 kHz mixes) | The app's real SamplerKit with real Tone in Chromium (OfflineAudioContext with suspend/resume), 19 cases compared over the full render | 13 bit-exact; the rest −141 to −166 dB (the remaining error is Chromium's summing order and its sin/cos). Limits: ≤ −90 dB whole render, ≤ −80 dB per block |
| Transport bugs R4-B1, R5-N1, R5-N4, R2-11, R2-12, R2-4, R2-3, R2-5/E1/L-e, R3-U6 | Each `scripts/probes` scenario rendered offline on the real core | Each one is impossible by construction (no extrapolated transport; start beat while pending; generations; gate lengths from ticks + wrap counters), and each has a test (see the core report in the code docs: `core/tests*.rs`) |
| Loop recording (quantize on the absolute grid with swing, triplets, note-repeat fine grid, louder-wins, overdub/replace, one undo step per take, count-in early hits and pre-roll, live merge without replaying a snapped-forward hit, gate held / wrapped / capped, undo during a take) | Ported `ArrangementEngine.beat.test.ts` / `round5` / `reload` cases, run on the real core offline | pass |
| Note repeat (transport grid anchored at tick 0, first line half a step after the press, across loop wraps and tempo changes, free-running grid shared between pads, release cancels at once) | Ported `NoteRepeat.test.ts` + new wrap, tempo and count-in cases | pass |
| WAV export (`ExportService.exportWav`) | The Electron app's real export in Chromium at 48 kHz on 3 drum projects (factory; TR-808 with swing, gate, choke, pitch, reverse, pan, filter, trim; factory + TR-909 with 1/8 swing, solo and mute) | lengths exact; full-render error −63.0 / −65.9 / −63.7 dB (limit −60). See "Tone bug" below |
| MIDI export (`exportMidi` via midi-writer-js) | Golden: 6 templates + 11 synthetic projects | **byte-identical** (including midi-writer's quirks: tick-0 notes in sequence, doubled end-of-track) |
| Export length (`getArrangementEndSeconds`) | Same golden, 4 injected duration functions | exactly equal |
| Audio output on real hardware | `cargo run -p mdaw-audio-host --example latency_probe` on this Mac | MiniFuse 4, 44.1 kHz, Fixed(256): 518 callbacks, 5.80 ms buffer latency, no drops or errors |

## Deviations (what the user notices)

- **No 100 ms look-ahead.** Play, stop, seek, tempo and loop changes take effect at the next render quantum, at most one device buffer plus 2.7 ms later. Play therefore sounds about 100 ms sooner than in Electron, and a seek before the start just moves the pending start (Electron restarts 100 ms later).
- **Note repeat** runs on the exact grid in the core. There is no catch-up, 15 ms pump, 40 ms look-ahead or 30 ms late-skip, so hits land where the TS aims them, and a release stops later repeats at its frame.
- **Tone bug not replicated.** Tone's offline render fires some Part events twice, on 128-frame block boundaries (4 of 35 and 11 of 58 hits in two of the captures), so the Electron export doubles those hits. Rust plays each note once. Against the as-is capture, two projects differ by −9.3 / −8.5 dB; the −60 dB check is against the capture with the duplicates removed. Both captures are kept in the fixture.
- **Disabled-effects track strip.** Tone's strip is not a pass-through even with every effect off: an 18 kHz lowpass, the 3-band crossover, the panner's mono downmix, and −56 dB wet leaks. These are modelled. The reverb's leak is in export only, not live.
- **Ordering inside Tone's look-ahead window:** Tone invokes transport callbacks up to 100 ms early, so a live pad command lands after every transport event at or before its frame. Events within a tick follow Tone's insertion order (pattern, metronome, repeat, punch close); across ticks the order is true time order.
- **A live hit snapped onto an existing step** can sound twice for up to one UI frame (suppression waits for the capture to be read). The TS suppresses it at once.
- **Pattern notes land on integer ticks**, because Tone's Part rounds them. This matches Electron.
- **Kept TS quirks:** `play()` starts at 0; an audio region doesn't sound if playback starts inside it; Song recording doesn't schedule the other tracks; solo is ignored in WAV export; there's no automation in export.

## Not in this phase

- Synth tracks are silent (live and export) and effect parameters have no effect until phase 4 (presets, voices, FX, automation).
- `drumEngine.mute` and sample preview are UI phases.
- The engine isn't yet driven by MIDI (phase 5) or the UI (phase 7).
