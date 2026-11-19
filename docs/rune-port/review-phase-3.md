# Phase 3 review (commit f1affaa): findings and fixes

| Reviewer | Code quality | UI/UX | Parity | Verdict |
|---|---|---|---|---|
| GPT-6.1 Sol | 6 | 6 | 7 | Request changes |
| Opus 5.5 | 8 | 7.5 | 8 | Approve once H1–H3 + M1 are fixed |

Both reviewers ran the 709–713 Rust tests. Sol re-captured the Electron WAV export in Chromium, reproducing −63.0/−65.9/−63.7 dB and Tone's duplicate triggers, and re-ran the hardware probe. Opus wrote adversarial tests. Both judged these justified: no 100 ms look-ahead, note repeat in the core, Tone's double-fire not replicated, the strip colouration modelled.

Each fix has a regression test that fails on f1affaa (except the host tests, where the API changed). After the fixes: 753 workspace tests, clippy and fmt clean (commits d3829b9, 456cd0a, 71a0495, 31fff93).

| # | Finding | Fix | Test / evidence |
|---|---|---|---|
| 1 | Command ring full → commands silently dropped; engine says Stopped, core keeps Started (both, High) | Per-producer control-side outbox, retried on every pump/tick; latest-value setters coalesce but never across transport commands; count-in + start in one command; the generation returned is the one the core sees | `a_full_ring_queues_instead_of_dropping`, `stop_after_a_stall_reaches_the_core`. Before: 1024/1100 accepted, core still Started |
| 2 | Timed then untimed commands from one producer reorder → stuck note repeat / gate (Opus H2) | The sender stamps "now" as `max(previous, now)` and clamps timed commands ≥ the previous one; per-producer FIFO in the core, merged by `(time, producer, seq)` | `a_producer_s_commands_apply_in_send_order`. Before: `[Release, Hit, Repeat…]`, stuck |
| 3 | Buffers freed on the audio thread when garbage lists fill (Sol High, Opus L1) | Current-buffer clones only decrement; garbage drained after each touching command/tick; ring 8192 + backpressure; `mem::forget` + counter as a last resort | `saturated_paths_never_free_on_the_audio_thread` (real SamplerKit/AudioRegions, 300-hit bursts, 66 live kit swaps, 120 region swaps): 0 frees (168 before) |
| 4 | Pattern events dispatched before repeat events within a quantum → choke broken (Sol High) | Ticks fire in time order; Tone's insertion order within a tick (pattern, metronome, repeat, punch close); live commands after transport events at the same frame | `pattern_and_repeat_fire_in_time_order_within_a_quantum`, `a_live_hit_applies_after_the_pattern_at_the_same_time`: choked voice < 1e-6 (0.212 before) |
| 5 | Suppression outlives the take (Sol) | Entries scoped to their transport generation; cleared on Stop/Start | `a_suppression_does_not_survive_the_take` |
| 6 | Shared atomics torn (both) | Seqlock `ClockSnapshot`; one snapshot used for gate finalisation and position | `snapshot_is_never_torn`: 12,032 consistent (field-by-field reads tore 8,460 times) |
| 7 | No wall-clock anchor (Opus M1, plan §3) | `process_timed` + `BlockClock`; `now_extrapolated`, `audio_time_for_instant`, `heard_time`; the host passes cpal playback timestamps | `anchor_maps_wall_clock_to_context_time`, `process_timed_publishes_the_callback_anchor` |
| 8 | Device rate change → wrong tempo/pitch (both; Sol measured 130.35 BPM) | Fixed context rate. The host picks a device config at that rate, else resamples (polyphase Kaiser sinc, allocation-free); `HostEvent::Resampling` | `tempo_is_exact_at_any_device_rate`: 120.000000 BPM at 48/44.1/96k; −104 to −119 dB sine error |
| 9 | Punch closure filtered after the wrap (2.96 vs 3 beats) (Sol) | Core emits `PunchClose` at Tone's safety tick, ordered with captures | `punch_closes_open_notes_at_the_transport_callback_time` |
| 10 | Silent 63/64-track limits; export counted muted tracks (Sol) | `MAX_TRACKS = 256` + a separate fallback slot; explicit `TrackLimitError`/event; export skips muted tracks first | `more_than_64_tracks_play_and_the_limit_is_explicit`, `export_skips_muted_tracks_before_counting` |
| 11 | Event overflow drops captures silently (Sol) | Separate critical ring (captures, wraps, punch, transport) vs visual ring; latched overruns mark the take as incomplete + `CaptureOverrun` event | `a_critical_event_overrun_marks_the_take_and_is_reported` (1,616/18,000 lost silently before) |
| 12 | Flaky NullHost pacing test; no `catch_unwind` in the callback; O(n²) staged queue (Opus Low) | Injectable deadline pacer; panic → silence + latched flag; `VecDeque` | `pacer::*`, `render_panic_is_caught_and_latches_silence` |
| – | Core must keep rendering with no device (both, part of 1) | `null_fallback` on by default: a pacer drives the core whenever no stream runs | `no_device_keeps_the_core_rendering_through_the_null_fallback` + 4 more |

**Still open:**
- Device switching, reopen and the null-to-real hand-off are untested on real hardware.
- The MIDI thread must call `LiveInput::flush()` (phase 5 requirement).
- A snapped live hit can still sound twice for one UI frame (documented deviation).
- Strict per-producer FIFO means a far-future timed command delays that producer's later "now" commands.
- The resampler adds 64–70 frames of latency, which is included in `output_latency`.
