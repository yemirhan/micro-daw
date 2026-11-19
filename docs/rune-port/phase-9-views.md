# Phase 9: the remaining views (`rune/crates/mdaw-app`) vs the Electron app

**Scope** (plan §4 row 9; inventories [`packs-ui-and-sample-dsp.md`](inventory/packs-ui-and-sample-dsp.md) §A–B,
[`views.md`](inventory/views.md) §2, §9–13, §15): the Packs page, the Samples view (edit / process /
generate) with the SampleProcessor DSP, the sample manager + audio import into the Song view and
audio-region waveforms, Settings, the Developer MIDI monitor, the command palette, the export dialog,
the controller setup dialog, the template picker, the synth editor, the top bar's instrument selector
and the Live FX strip. Built on rune-port after Phase 8 and merged with its review fixes (bf77715).

## Method

**Modules** (new unless noted):

| Module | Port of | Notes |
|---|---|---|
| `mdaw-engine/src/sample_processor.rs` (+ `tests.rs`, `parity_tests.rs`) | `services/SampleProcessor.ts` | `process_audio` (reverse → [filter → PitchShift → FeedbackDelay → Reverb] → normalize, `min(tail, 3)`), `generate_signal`, `PitchShift`, `NoiseReverb` + `reverb_envelope`, `NoiseBank` (Tone's cached white-noise buffer), `MathRandom` |
| `mdaw-dsp` `WaveTables::from_coefficients`, `mdaw-engine` `Osc::with_wave` / `Lfo::with_osc` | Chromium custom `PeriodicWave`, Tone `_getRealImaginary` | the phase-shifted LFOs of `Tone.PitchShift` (sawtooth 180°, triangle 90°) |
| `samples.rs` | `SampleManager.ts`, `useSampleLibrary.ts` | `SampleStore` (the engine's `AudioSamples`), decoding on workers (sync in headless runs), peaks, library + `micro-daw-sample-library`, restore by id, audition, process / generate jobs, the Song import |
| `signal_preview.rs` | `useSignalPreview.ts` | the Generate tab's synth on the engine preview slot; MIDI keys through a raw listener |
| `kit_preview.rs` | `kitPreview.ts`, `remoteKit.ts` | the demo groove rendered offline through the engine's `SamplerKit`, remote kits with a 32-entry decoded LRU |
| `views.rs` | `App.tsx`'s remaining state, `usePacksController`, `useSettings`, the dialogs' state | `Views` (`App::p9`), all actions, `p9_poll`, phase-9 keys |
| `ui/p9/{kit,samples,packs,settings,dev,dialogs,rack}.rs` | `view-kit.tsx`, `components/{samples,packs,settings,dev,controller}/*`, `CommandPalette`, `ExportDialog`, `TemplatePicker`, `SynthEditor`, `EffectsPanel`, `InstrumentSelector` | |
| `scenes_views.rs`, `scripts/parity-phase-9/` | — | 21 screenshot states, the same steps in both apps |
| `icons_views.rs`, `tools/gen-icons-views.mjs` | lucide | 24 icons |
| `mdaw-engine` `set_preview_instrument` / `preview_synth_note` | — | a synth on the preview slot (signal preview) |
| `mdaw-model` `send_sample_to_arrangement` | `sendToArrangement` | the trim applied in place without an undo entry of its own, as the TS mutates the returned region |

**Comparison method.**
- DSP: `rune/parity/chromium/capture-sample-processor.steps.mjs` runs the app's real `processAudio` /
  `generateSignal` in headless Chromium (ui-snap) on generated inputs and stores per case the length,
  peak and the block evidence of `stats.mjs` (`rune/parity/fixtures/sample_processor.json`); the Rust
  test estimates the error-to-signal ratio of the whole render from it. `Math.random` is a seeded
  mulberry32 per case on both sides. With `PARITY_DEBUG_DIR` both sides compare raw renders sample by
  sample (`sample_processor_parity_debug`).
- Screenshots: `scripts/parity-phase-9/run.sh` (ui-snap + `micro-daw --snap`, 1440×900, scale 1). The
  Rune side was taken from a private copy of my binary (other worktrees overwrite the shared one).
  ui-snap has no Electron main, so its Packs states get a stub pack API with the bundled catalog and
  nothing installed (what the headless Rune app has).
- Interactions: 41 `Runtime` tests in `src/tests/t_p9_*.rs` asserting library / storage / engine /
  router / doc state and undo labels; 6 DSP unit tests.
- Real window: `MDAW_SMOKE=views` (below).

## SampleProcessor parity (Rust vs Chromium renders of the real TS)

Error-to-signal over the whole render, per channel (target ≤ −60 dB, test threshold −60 dB):

| Case | dB | Case | dB |
|---|---|---|---|
| filter LP 1 kHz, 48 k stereo | −127.6 / −127.1 | delay 0.25 / 0.3 / 0.3, 48 k stereo | −137.0 / −136.6 |
| filter HP 2.5 kHz Q 4, 44.1 k mono | −138.7 | delay 0.6 / 0.8 / 0.5 (tail capped at 3 s), 44.1 k mono | −68.6 |
| filter BP 800 Hz Q 10, 48 k stereo | −115.5 / −115.9 | reverb 2.5 s / 0.3, 44.1 k stereo | −128.9 / −131.0 |
| pitch +7, 48 k stereo | −76.8 / −78.8 | reverb 0.8 s / 0.7, 44.1 k mono | −130.2 |
| pitch −12, 48 k mono | −75.2 | everything + reverse + normalize, 44.1 k stereo | −89.6 / −88.0 |
| pitch 0, 44.1 k mono | −196.7 (exact) | reverse + normalize only | exact |
| pitch +24, 44.1 k stereo | −72.8 / −75.3 | generate sine / square / saw / triangle | −139.5 / −126.2 / −129.5 / −138.5 |
| | | generate white noise (seeded) | exact |

Lengths are identical in every case. Findings on the way (recorded, not app bugs):
- **Tone caches phase-shifted PeriodicWaves across contexts** (`Oscillator._periodicWaveCache`, keyed
  by type + phase only). A wave built in a 48 kHz render is reused by a later 44.1 kHz render with the
  48 kHz rate scale, so PitchShift's B / cross-fade LFOs run 0.919× too slow there. In the app every
  Samples render runs at the context rate, so this never shows; the capture clears the cache per case.
- **Tone's white-noise buffer is built once per page** from `Math.random`, usually by the app before
  the capture runs; the capture overwrites that cached buffer with seeded noise (`seedNoise`) and runs
  the reverb cases at its rate (the context rate, as in the app).

## Screenshots (`docs/parity/phase-9/<state>-electron.png` / `-rune.png`)

Every pair was inspected (after the fixes listed under "Fixed while comparing").

| State | Remaining differences |
|---|---|
| `samples-empty`, `samples-generate` | none seen beyond the shared ones |
| `samples-sine` (Edit tab), `samples-trim` | Chromium's number inputs show "0,20" (locale) centred with spin buttons; Rate 44.1 kHz (Chromium's context) vs 48.0 (headless Rune) |
| `samples-process` | section rows within 1–2 points |
| `packs`, `packs-builtin`, `packs-remote`, `packs-search` | filter chips 1–2 points narrower; content 1–2 points higher; the selected card has the 1.5-point ring but not the 4-point 18 % halo (one box shadow per element) |
| `settings` | Chromium's profile had "Check for updates" off (stored setting of the ui-snap profile); version "dev" (no `electronAPI`) vs "1.4.0" |
| `dev`, `dev-events` | device chip (MiniFuse 4 seen by Chromium's Web MIDI, none in the headless Rune app) |
| `palette`, `palette-search` | none seen after the fixes (cmdk highlights the first item; so does Rune) |
| `export`, `templates`, `controller` | controller: device sentence (as above) |
| `synth-editor` | the editor is about 3 points shorter |
| `instrument-menu` | cmdk highlights the first row (known since phase 8) |
| `live-fx`, `song-audio` | none seen; the Song lanes now have the same height as Electron's (the strip was the phase-8 difference) |

Shared differences: the top bar's left inset (traffic lights) and the MIDI device name, as in phases 7–8.

Fixed while comparing: OKLCH colour mixing for faceplates (hue interpolated like `color-mix(in oklch)`),
select trigger heights (`data-[size=default]:h-8` wins over `h-7`), arbitrary `text-[12px]` line
height, the monitor's time column (HTML collapses the padded spaces), Settings `divide-y` rules, the
palette's per-group `p-1` and exact lucide icons, the controller dialog's hover-only clear buttons /
CC column / row heights, the template grid (it collapsed inside a content-sized dialog), the effect
section header height.

## Interaction tests (`rune/crates/mdaw-app/src/tests/t_p9_*.rs`)

| Inventory | Test(s) |
|---|---|
| §13 header, empty library, only Generate without a sample | `t_p9_samples::empty_library_shows_the_generate_tab_and_import` |
| §B generateSignal names / lengths, not persisted, noise disables Freq | `generate_registers_a_named_sample_selected_but_not_persisted`; unit `generator_names_and_lengths` |
| §13 Edit tab texts, readouts, trim handles (10-point hit), inputs (clamp start ≤ end − 0.01, `|| duration`), Reset trim | `edit_tab_shows_the_waveform_readouts_and_trims` |
| §B import (basename, dedupe by path, select first), persistence, restore by id with name + trim, rename (Enter trimmed / Escape), remove | `import_dedupes_persists_and_restores_name_and_trim` |
| §B audition (trimmed range, toggle, auto-stop, stopped by selection) | `audition_plays_the_trim_and_stops_by_itself` |
| §13 Send to arrangement (audio track named after the sample, offset, `trimmed/60·bpm`, DAW mode) | `send_to_arrangement_adds_an_audio_track_with_the_trimmed_region` |
| §B Process (hint, stage subtitles, parameters while on, `(processed)`, selected, reverse + normalize result) | `process_needs_a_stage_and_makes_a_processed_copy`; DSP: `sample_processor_matches_chromium` + unit tests |
| §B signal preview (mute on mount, MIDI note sets `round(mtof)` + note name, hold to preview, unmount) | `generate_tab_midi_keys_play_and_set_the_frequency` |
| Electron bug: in-memory samples survive the page remount; selection / tab per mount | `the_page_mounts_with_a_fresh_selection_and_keeps_in_memory_samples`; TS `useSampleLibrary.test.ts` |
| Song import (⌘I / Audio): a track per file, its waveform, no missing badge; project regions load by path | `song_import_makes_an_audio_track_per_file_with_its_waveform`, `project_audio_regions_load_their_samples_by_path` |
| §A header, sections, cards, aria `name: tagline` | `t_p9_packs::header_sections_and_cards` |
| §A search (every word), tag chips (active stays, All), clear, "No packs match", Show all packs | `search_terms_tags_and_clearing` |
| §A detail (built-in texts), deselect, Escape | `selecting_a_builtin_opens_its_detail_and_escape_closes_it` |
| §A Load on pads (kit, Beat view, pending load dropped) | `load_on_pads_replaces_the_kit_and_goes_to_the_pads` |
| §A kit preview (96 BPM steps, flashes per item, toggle, auto-stop at 32·dur + 1.2, a sound preview stops it) | `kit_preview_plays_the_groove_flashes_and_stops` |
| §A remote detail texts, Get & load, remote kit preview failure + Retry (offline) | `a_remote_pack_streams_previews_and_offers_get_and_load` |
| §A uninstall confirmation (4 s / 9 s), "Cancelling…" | `uninstall_asks_once_more_and_disarms`, `cancelling_shows_until_the_install_settles` |
| §2 palette (open, groups, filter, Enter runs, modes, empty, Escape, ⌘K toggle, arrows, preset / add-track commands) | `t_p9_dialogs::palette_opens_filters_runs_and_closes`, `palette_arrows_move_and_commands_act` |
| §10 export (subtitle, options, WAV / MIDI written, Escape, nothing to export) | `export_dialog_counts_tracks_and_writes_wav_and_midi` |
| §11 template picker (texts, tabs, load, not dirty) | `template_picker_filters_and_loads_a_template` |
| controller dialog (wizard, Skip / Backspace, learn consumes the note, Esc twice, single pad, Add knob + CC bind, remove, reset, channel, Done) | `controller_dialog_learn_flows_and_escape` |
| modal dialogs own the keyboard | `a_dialog_owns_the_keyboard` |
| §12 Settings (texts, switches / select / slider persisted in `micro-daw-settings`) | `t_p9_pages::settings_rows_write_the_settings_key` |
| §15 MIDI monitor (formatting, clock dropped, filter, pause drops input, clear, feed stops on leave) | `midi_monitor_lists_filters_pauses_and_clears` |
| §9 Live FX (knobs on the live bus, sections one at a time, filter type, LFO wave) | `live_fx_knobs_sections_and_selects_drive_the_live_bus` |
| §9 synth editor (categories, knob → override, Reset edits, choices) | `synth_editor_presets_categories_knobs_and_reset` |
| §9 instrument selector (search, empty, pick) | `instrument_selector_searches_filters_and_picks` |

## Real device check

`MDAW_SMOKE=views MDAW_DATA_DIR=<tmp> micro-daw` (real window, MiniFuse 4 at 44.1 kHz):

```
[smoke] samples: 1 in the library, selected Some("sine 440Hz 1s")
[smoke] samples: ["sine 440Hz 1s 1.00s", "sine 440Hz 1s (processed) 3.50s"]
[smoke] packs: kit preview playing=Some("builtin:tr808/tr808") output peak=0.4315
[smoke] export: ".../smoke-export.wav" written=Some(...) bytes=Some(2222684) error=None
[smoke] export: ".../smoke-export.mid" bytes=Some(410)
```

The pages render in the window, the processed sample has the reverb tail, the 808 groove reaches the
device buffer, and the exports are written. I did not listen.

## Electron bugs (fixed in both apps, user decision 2026-11-18)

1. **Generated and processed samples vanished from the Samples library** whenever the page was left
   and opened again: they are not persisted (empty path) and `useSampleLibrary`'s state started over
   on remount. The library now lives for the session (module scope in the TS, `App::p9` in Rust);
   the selection and the tab stay per mount as before. TS `src/hooks/useSampleLibrary.ts` + test
   `useSampleLibrary.test.ts`; Rust `the_page_mounts_with_a_fresh_selection_and_keeps_in_memory_samples`.

Kept as in the TS (judgment calls, documented): `SampleDetail`'s tab is chosen at mount (Generate,
since nothing is selected yet), so a sample selected later still shows Generate until Edit is clicked;
the Settings buffer size / default master volume / auto-connect are saved but nothing reads them.

## Deviations and gaps

Kept / by design:
- File dialogs: Rune's open dialog picks one file (Electron's import dialog allows several); export asks
  for the path with a save dialog before rendering (Electron renders, then downloads).
- Audition plays through the engine's preview slot (unity gain, compensated); it stops with the slot's
  10 ms fade.
- The signal preview's synth is a plain oscillator + Tone-shaped ADSR on the preview slot (not Tone's
  `PolySynth` object graph); not rendered against Chromium.
- The kit preview is rendered offline through the engine's own `SamplerKit` and played as one buffer
  (the TS schedules a live SamplerKit); same hits, gains and timing (0.08 s lead).
- The palette ranks results (label prefix, label, keywords, in-order characters) instead of cmdk's
  exact score; cmdk's first-row highlight is matched.
- MIDI monitor: "Jump to latest" never shows (scroll-away isn't detected); the list follows.
- The selected pack card's 18 % halo isn't drawn (ring only).

Not in this phase (listed, not dropped):
- Project New / Open / Save / Save As / recent projects (palette entries and ⌘N / ⌘O / ⌘S do nothing
  yet): phase 10's project service. "New from Template" loads the template but has no "unsaved
  changes" prompt (none exists yet in Rust).
- Auto-updater: the Settings update row shows the version and a "Check now" button with no updater
  behind it (plan §5: phase 10).
- Toggle Fullscreen from the palette uses Rune's full screen (no Electron state sync).

## Shared files changed (for the merge)

`app.rs` (`Menu::P9`, `App::p9`, `p9_poll` / `p9_busy` in `poll`), `services.rs` (`SampleStore` handed
to the engine instead of `NoSamples`), `keys.rs` (`views::keys` hook, phase-9 text fields), `lib.rs`
(modules), `scenes.rs` (21 scenes appended), `smoke.rs` (`MDAW_SMOKE=views`), `tests.rs` (4 modules),
`ui/mod.rs` (pages, phase-9 menus and dialogs, synth editor in the Beat view), `ui/knob.rs`
(`KnobId::View`), `ui/topbar.rs` (instrument popover, synth editor toggle, controller setup),
`ui/beat/mod.rs` + `ui/beat/transport.rs` (controller / export buttons), `ui/song/mod.rs` (synth
editor + Live FX strip), `ui/song/toolbar.rs` (Audio import, Export), `ui/song/timeline.rs` (real sample
status + `region_waveform`); `mdaw-engine` `lib.rs`, `fx/osc.rs`, `fx/lfo.rs`, `engine/preview.rs`;
`mdaw-dsp` `factory/osc.rs`; `mdaw-model` `doc/regions.rs`; TS `src/hooks/useSampleLibrary.ts`.

## Verified / not verified

**Verified here:** the DSP parity table (`cargo test -p mdaw-engine sample_processor`); the 41 phase-9
`Runtime` tests and every earlier app test after merging bf77715; `npm test`, `cargo test --workspace`,
clippy `-D warnings`, `fmt --check` (numbers in the commit report); 21 screenshot pairs inspected; the
`MDAW_SMOKE=views` window run on the MiniFuse 4.

**Not verified:** listening (signal preview, kit grooves, processed samples); Chromium renders of the
kit groove and the signal preview synth; real pack installs / uninstalls from the page against the
network (offline in tests; the service is phase 6's); the real MPK driving the learn dialog;
VoiceOver; Windows / Linux.
