# Phase 4 review (commit dd7cd11): findings and fixes

| Reviewer | Code quality | Parity | Docs |
|---|---|---|---|
| GPT-6.1 Sol | 6 | 7 | 8 |
| Opus 5.5 | 7.5 | 8 | 8 |

Both reviewers measured the live-edit path on the audio thread and probed the instruments
outside the captured settings. The HEAD figures below were re-measured on dd7cd11 with the same
tests as the fixes (a scratch checkout with the new tests ported to the old API).

After the fixes: 918 workspace tests (engine 376 lib, 11 of them new, + 4 allocation-guard, 1
new), all green; `cargo clippy --workspace --all-targets -- -D warnings` and
`cargo fmt --all --check` clean.

**Parity is unchanged.** Every Chromium comparison prints the same numbers as dd7cd11 (diff of
the full `--nocapture` output of the synth, piano, character, FX, compressor, convolver,
sampler and export parity tests plus `synth_parity_report`: identical). The live `set()`
captures still match at −119.9 to −135.8 dB, harp at −146 dB.

| # | Finding | Fix | Test / evidence (before → after) |
|---|---|---|---|
| 1 | `SynthSet` rebuilt unused voices on the audio thread (wave tables and noise buffers behind process-wide mutexes, preset clones, comb buffers); queued edits all applied (both, High) | `setSynthParam` builds a `SynthPatch` on the control side: the merged preset (shared `Arc` between instrument and voices), the oscillator tables / noise buffer (`Voice::Prep`), and fresh voices for the slots from the instrument's published `used` count on. The core swaps them in (`PolyInstrument::apply`); oscillators are rebuilt in place (node storage pre-sized to 7); the patch, holding the replaced preset and voices, returns through the garbage ring (`Garbage::Patch`). Voice storage is pre-sized to `MAX_VOICES`. Queued `SynthSet`s of one slot coalesce (latest wins). The documented exception is removed | `live_synth_parameter_edits_do_not_allocate` (during playback with held notes): cutoff ×24 1632 → 0 allocations/frees; unison 1→5→3 370 → 0; polyphony 8→32→128 1075 → 0; saw/fat → pulse 231 → 0; harp dampening / release / polyphony 214 → 0; grand-piano edits 18 → 0. Edit quantum, release: unison 6.5 ms → ≈0.1 ms, pulse switch 1.7 ms → ≈0.06 ms, polyphony 0.82 ms → ≈0.07–0.14 ms, harp 0.30 ms → 0.01 ms (median quanta 7–55 µs; single samples, so OS jitter included). `a_prepared_patch_applies_without_allocating_and_keeps_what_it_replaced` |
| 2 | Pluck OnePole unstable (`a0 ≥ 2`): harp at 48 kHz, dampening 12000, velocity 1 → 16 kHz → non-finite from frame ~1360, survives `releaseAll`, poisons FX / master (Sol, High) | Safety deviation: effective dampening clamped to `a0 ≤ 1.999` (15271.2 Hz at 48 kHz, 14030.5 Hz at 44.1 kHz); below that, Tone's formula bit for bit. Output guard: `PolyInstrument` checks its voice bus, the core checks every instrument output and every slot output after its strip; a non-finite quantum is silenced, the instrument clears its state (`clear_non_finite`), `Shared::non_finite_outputs` counts it | `pluck_is_stable_over_the_editor_ranges_at_44k_and_48k` (dampening 300–12000, sensitivity 0–1, sustain 0.8–0.999, pick 0.1–8, velocities 0–1, both rates): HEAD non-finite from frame 1849 (44.1 kHz, 12000 Hz, sensitivity 0.5) → all finite, guard never fires. `the_dampening_clamp_only_touches_unstable_settings`, `a_non_finite_instrument_output_is_silenced_and_its_state_cleared`. Harp parity −146.3 dB unchanged |
| 3 | Rebuild path to pulse kept width 0 (Opus, Medium) | `ToneOsc::update` writes the resolved width on both paths (after the type change, like `updateOmni`) | `switching_to_pulse_applies_the_resolved_width`: saw → pulse 0.3 and fat → pulse: HEAD width 0 / 0 → 0.3 / 0.4 (default), clamp to 0.95; `sending_the_same_preset_twice_does_not_change_the_audio` (6 presets, bit-identical) |
| 4 | Voice filter kept its history on a type change; Chromium resets it (Sol, Medium) | `ToneFilter::set_kind` resets the state when the type actually changes (same type: kept), as the FX filter already does | `a_type_change_resets_the_state_like_chromium`: warmed 1 kHz LP → HP, constant input: HEAD [0.983833, 0.953533, …] → [0.940989, 0.822324, …] (the reviewer's Chromium capture; y[0] is the highpass's b0 from rest) |
| 5 | Retire list of 4: the 5th switch within 1.2 s hard-cut the oldest retiree (Opus, Medium) | `RETIRE_SLOTS` 16 (+2 fade entries, pre-allocated); when full: reclaim retirees already silent (peak < −120 dBFS for 16 quanta after their release), else fade the oldest over 2 quanta. Limit documented | `rapid_preset_switches_with_a_held_chord_never_cut_a_ringing_retiree` (sub-bass chord, switches 10 ms apart; max \|2nd difference\| vs the steady chord's 4.6e-4): 6 switches HEAD 8.3e-2 → 1.05e-3; 20 switches (eviction + fade) HEAD 8.3e-2 → 1.08e-3 |
| 6 | Export `knownDeviation` cases bypassed every assertion (Sol, Medium) | Per-case ceilings: pluck_harp ≤ −11 dB, saw_lead ≤ −24 dB; worst block ≤ measured + 3 dB for every case (+1 dB for the deviations); every render finite and above −60 dBFS RMS | `wav_export_matches_the_electron_app_in_chromium`: figures unchanged at the time (−86.5 … −70.9 dB; −12.0 / −25.9 dB), now asserted; superseded by the export-deviation fix below (all cases ≤ −60 dB) |
| 7 | Polyphony silently capped at 64 (Opus, Low) | `MAX_VOICES` 128, storage pre-allocated (a polyphony change pushes prepared voices without allocating). Above 128 is capped and documented (the TS has no cap; the editor stops at 16) | `polyphony_up_to_128_voices_is_supported` (128 held notes, live growth 8 → 128 keeping the voices in use) |
| 8 | Status published every quantum by every instrument incl. retirees (stale overwrite), 128-note scan, held set and voice count not consistent (both, Low) | Held bitset kept on note events (own bit + re-check of held bits only); a seqlock `StatusSnapshot` (instance, active, used, held); a replaced instrument is `retire`d and stops publishing; the engine ignores a snapshot of another instance | `only_the_current_instrument_publishes_its_status`, `the_held_set_follows_steals_mono_hand_overs_and_release_all` |
| 9 | Docs: Stop timing, track rebuild tails, realtime claims, measured vs limit (both, Low) | Stop at `t` vs the TS's `now()` + 100 ms documented as a deviation; track rebuild cutting tails listed as a kept TS quirk; phase-4 doc states the measured realtime figures and "measured X, test limit Y" (synth parity: block energy measured ≤ 0.0005 dB, limit 0.5 dB; whole render limit −90 dB) | [`phase-4-instruments.md`](phase-4-instruments.md), [`rune-parity.md`](../rune-parity.md) |
| – | Export deviations pluck_harp (−12.0 dB) and saw_lead (−25.9 dB) (both reviewers: hypotheses) | Root causes found with instrumented Chromium captures (every Web Audio call + Tone `now()` per note logged). saw_lead: Tone builds a voice at its clock time T, so constructor writes (sub osc `detune = −1200`, fat spread) land at T, one frame after the frequency signal started; modelled by `Voice::offline_built(T)` (reviewer lead (c); leads (a) filter tail and (b) wave cache ruled out). pluck_harp: Tone's tick round-trip fires the beat-7.5 event at tick 1439 (`export_wav::tone_fired_tick`, `PolyInstrument::set_offline_dispatch`); a fresh voice's comb params are swapped in at T, after its first quantum on the comb defaults (`PluckVoice::swap_comb_params`) | `wav_export_matches_the_electron_app_in_chromium`: saw_lead −25.9 → **−84.4 dB** (worst block −13.8 → −59.0), pluck_harp −12.0 → **−84.2 dB** (−1.7 → −56.2), fx_tracks −70.9 → −78.7 dB; both held to −60 dB now, worst-block ceilings retightened to measured + 3 dB. Chromium's own pluck export varies run to run by −106 to −117 dB (5 runs, 3 distinct WAVs) |

**Still open:**
- A pluck near the stability limit stays very bright, as in Tone: the OnePole boosts the top
  octave by up to `a0/(2 − a0)` (peaks up to ~60 at sustain 0.999 in the range test).
- The `CharacterChain` and track strips have no state reset of their own: the guards keep
  non-finite samples out of them (the voice bus and every instrument output are checked
  first), and the slot guard after the strip is a last-resort mute.
- `SynthPatch` fresh voices are sized from the instrument's last published `used` count; a
  stale count (e.g. right after a preset switch) only costs extra control-side voices, never an
  audio-thread allocation.
