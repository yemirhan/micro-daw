# Phase 7: UI shell + Beat view (`rune/crates/mdaw-app`) vs the Electron app

**Scope** (plan §4 row 7, inventories [`beat-ui.md`](inventory/beat-ui.md) and [`views.md`](inventory/views.md) §1, §3, §4):
theme, fonts, icons, start screen, top bar, sidebar, the Beat view (pads, pad editor, step
sequencer, transport, drum-track / kit / pad menus, missing packs, pending-load chip),
computer-keyboard pads, persisted preferences, the instrument dock and the sample browser, wired
to the real engine, `mdaw-midi` and `mdaw-packs`. `cargo run -p mdaw-app` is now a usable
Beat-mode app.

## Method

**Crate layout** (`rune/crates/mdaw-app/src`):

| Module | Port of | Notes |
|---|---|---|
| `theme.rs` | `src/index.css` | every OKLCH token as `rune::oklch`, radii, `.daw-label`; Manrope / JetBrains Mono / Syne embedded (`AppSpec::fonts`); the text helpers carry Chromium's line heights (Tailwind `text-xs` 16, `text-sm` 20 …, else `body { line-height: 1.4 }`; `leading-none` = `.lh(size)`); font smoothing off like `-webkit-font-smoothing: antialiased` (set on the first `poll`) |
| `assets/fonts/*.ttf` | `@fontsource` WOFF | `tools/woff2ttf.py` unwraps the app's own latin WOFF files (stdlib zlib, glyph tables untouched) and adds typographic names (IDs 16 / 17): @fontsource's static faces name each non-RIBBI weight as its own family ("Manrope SemiBold"), which Chromium never sees (it groups by `@font-face`) but fontique does, so without them 500 drew as 400 and 600 / 800 as 700. OFL licences beside them |
| `icons.rs` | `lucide-react` | `tools/gen-icons.mjs` turns the 47 lucide icons the app imports into Rune SVG bodies (+ filled play / square / circle for `fill-current`) |
| `services.rs` | the singletons | engine + cpal `OutputHost` (null-pacer fallback) or an offline core rendered on the UI clock (headless / tests); `MidiHost` + `MidiUi` + `ProfileSaves` (`build_with_settings`, `enable_with_settings` at Start); `PackService` (`init`, `poll`, `on_kit_selected`); localStorage keys (`SettingsStore`); starter project / saved project load and the 300 ms autosave (`useArrangementPersistence`) |
| `app.rs` | `App.tsx` state + `poll` | engine tick, `EngineEvent::PadHit` → pad flash, MIDI pump, packs pump, undo-group idle timers (700 ms pad edits, 400 ms velocity bursts), playhead + loop position as `Live` values; per-frame focus comparison (the DOM's `blur`: BPM draft commit, step-grid ring, Enter / Space pad release) and unmount cleanups (Beat view → held pads, browser → preview, dock → note); `useCatalogWhile`; `shutdown` (close / quit) |
| `actions.rs` | `useBeatActions` | every engine call of `useBeat.ts` with the same undo groups and labels |
| `keys.rs` | `App.tsx` shortcuts + BeatView keys | start-screen any-key, computer-keyboard pads (before the app shortcuts, like the capture-phase listener), Space / M / N / B / Tab / ⇧⌫ / ⌘Z / ⌘⇧Z / ⌘1–4 / ⌘, |
| `ui/start.rs`, `ui/topbar.rs`, `ui/sidebar.rs`, `ui/dock.rs`, `ui/browser.rs` | `StartScreen`, `TopBar`, `Sidebar`, `InstrumentDock`, `BrowserPanel` / `SampleBrowser` / `useSampleBrowser` | |
| `ui/beat/*` | `BeatView`, `BeatTransport`, `PadGrid` / `PadThumb`, `PadContextMenu`, `PadEditor` (+ chop panel, trim waveform), `StepSequencer`, `DrumTrackMenu`, `KitMenu`, `MissingPacks`, `PendingLoadChip`, `usePackAvailability` | |
| `ui/knob.rs` | `components/ui/Knob.tsx` | rune-kit has no knob: same 270° arc geometry, bipolar arcs, drag (200 px / 800 px with Shift), wheel, double-click reset, keys, step snapping |
| `scenes.rs`, `smoke.rs` | — | headless states for the screenshots; `MDAW_SMOKE=1` scripted run of the real window app |

**Threads.** Unchanged from phases 3–6: the UI thread owns the doc, the engine control side,
the router lock for on-screen input, the pack service and the settings file. On-screen and
computer-key pads call the router's `drumPress` / `drumRelease` / `drumHit` exactly like the TS
(`useMidiNotes`), so they are heard, captured by the loop recorder, note-repeated and flashed
the same way as controller pads. Flashes, the step playhead and the "bar.beat" LCD are `Live`
values: playback never re-runs the view.

**Comparison method.**
- Screenshots: `scripts/parity-phase-7/run.sh` puts both apps in the same state, 1440×900 at
  scale 1. Electron: `scripts/ui-snap.mjs` (the real renderer in headless Chromium) driven by
  `scripts/parity-phase-7/steps.mjs`. Rune: `micro-daw --snap … --scene <state>`
  (`scenes.rs`, offline engine, same steps through the same actions). I looked at every pair;
  the remaining differences are listed below.
- Interactions: 85 `Runtime` tests (`src/tests/*`, + 3 theme unit tests) drive the real view
  with pointer, wheel and key events (headless runtime with the production fonts, offline
  engine rendered on the virtual clock) and assert the doc / engine state, undo labels, the
  element tree and the accessibility tree, as the TS tests assert. `cargo test -p mdaw-app`
  runs in 3.5 s (it took 128–273 s before the review: the engine crates are now optimised in
  dev / test builds).
- Real device: `MDAW_SMOKE=1 cargo run -p mdaw-app` (below).

## Screenshots (`docs/parity/phase-7/`)

| State | Electron | Rune | Notes |
|---|---|---|---|
| Start screen | [start-electron](../parity/phase-7/start-electron.png) | [start-rune](../parity/phase-7/start-rune.png) | final state of the intro; footer: ui-snap has no `electronAPI` so it shows "micro daw", Rune the version (as the real Electron app does) |
| Beat view default | [beat-electron](../parity/phase-7/beat-electron.png) | [beat-rune](../parity/phase-7/beat-rune.png) | browser closed (ui-snap's browser has no file system) |
| Browser open | [browser-electron](../parity/phase-7/browser-electron.png) | [browser-rune](../parity/phase-7/browser-rune.png) | Electron-in-Chromium: "Browser unavailable"; Rune: the real empty state ("Add a sample folder") |
| Pad selected (B6 Low Tom) | [pad-selected-electron](../parity/phase-7/pad-selected-electron.png) | [pad-selected-rune](../parity/phase-7/pad-selected-rune.png) | both 1.21 s: Low Tom's tail is tonal. The factory Snap (B5, used before) is `Tone.Noise` trimmed at −60 dBFS, so its length changes with the noise (Electron 216 ms, Rune 210 ms) |
| Step pattern | [pattern-electron](../parity/phase-7/pattern-electron.png) | [pattern-rune](../parity/phase-7/pattern-rune.png) | |
| Playing (playhead) | [playing-electron](../parity/phase-7/playing-electron.png) | [playing-rune](../parity/phase-7/playing-rune.png) | playhead step differs with wall-clock timing |
| Pad context menu | [pad-menu-electron](../parity/phase-7/pad-menu-electron.png) | [pad-menu-rune](../parity/phase-7/pad-menu-rune.png) | |
| Kit menu | [kit-menu-electron](../parity/phase-7/kit-menu-electron.png) | [kit-menu-rune](../parity/phase-7/kit-menu-rune.png) | |
| Drum-track menu | [drum-track-menu-electron](../parity/phase-7/drum-track-menu-electron.png) | [drum-track-menu-rune](../parity/phase-7/drum-track-menu-rune.png) | |
| Quantize select | [quantize-menu-electron](../parity/phase-7/quantize-menu-electron.png) | [quantize-menu-rune](../parity/phase-7/quantize-menu-rune.png) | Radix aligns the list on the chosen item; Rune drops it below the trigger |
| Chop panel | [chop-electron](../parity/phase-7/chop-electron.png) | [chop-rune](../parity/phase-7/chop-rune.png) | |
| Count-in | [count-in-electron](../parity/phase-7/count-in-electron.png) | [count-in-rune](../parity/phase-7/count-in-rune.png) | the number differs with timing |
| Recording | [recording-electron](../parity/phase-7/recording-electron.png) | [recording-rune](../parity/phase-7/recording-rune.png) | |
| Note repeat on | [note-repeat-electron](../parity/phase-7/note-repeat-electron.png) | [note-repeat-rune](../parity/phase-7/note-repeat-rune.png) | |
| Key hints on | [key-hints-electron](../parity/phase-7/key-hints-electron.png) | [key-hints-rune](../parity/phase-7/key-hints-rune.png) | |
| 2×8 layout | [layout-2x8-electron](../parity/phase-7/layout-2x8-electron.png) | [layout-2x8-rune](../parity/phase-7/layout-2x8-rune.png) | "Low Tom" fits on one line in both now; at the 54-point label width Rune wraps "Open HH", Chromium "Sub Kick" |
| Dock open | [dock-open-electron](../parity/phase-7/dock-open-electron.png) | [dock-open-rune](../parity/phase-7/dock-open-rune.png) | compact keyboard card ported (well, label strip, 32 × 100 / 24 × 60 keys with their shadows) |
| Edited pad (gain, pan, pitch, trim) | [edited-pad-electron](../parity/phase-7/edited-pad-electron.png) | [edited-pad-rune](../parity/phase-7/edited-pad-rune.png) | |
| Missing packs | — | [missing-packs-rune](../parity/phase-7/missing-packs-rune.png) | no Electron reference: the browser-only renderer has no pack store (`window.electronAPI`), so it never reports a pack missing |

That is 18 pairs and one Rune-only state. Remaining visible differences after the review fixes
(recaptured 2026-11-19, every pair inspected):
- **Top bar**: the Rune window reserves 68 points on the left for the traffic lights (ui-snap's
  Chromium page has none), so the left group starts further right and the instrument name has
  less room; ui-snap's Chromium sees the MiniFuse MIDI input, the headless Rune scene has none.
- **Vertical offsets** of about 1 point in the pad grid and step rows, a few points in the
  widths of segmented controls (One-shot / Gate, Repeat): Chromium's fractional layout vs Rune's.
- **Chop panel** is ~4 points shorter (the native range input is taller than the slider).
- **Quantize list** opens below its trigger; Radix aligns it on the chosen item.
- **2×8 labels** wrap on different pads (above).
- Timing-driven values (playhead step, count-in number, loop position) differ by capture time.

Fixed by the review (before → after): text weights (all semibold text drew Bold; the start
wordmark "micro" was 121 points wide, now 177.6 vs Chromium's 181), font smoothing, menu row
heights (27 → 28), the knob stack (+4.9 points), the kit-name / rename inputs' vertical
alignment and the kit menu's initial focus, the chop sensitivity width (150 points → the row's
rest), the dock keyboard card.

## Interaction tests (`rune/crates/mdaw-app/src/tests/`)

Each line of [`inventory/beat-ui.md`](inventory/beat-ui.md) → the test that covers it, or why not.

| Inventory | Test(s) |
|---|---|
| §0 Space / M / ⇧⌫ / N / Tab / B / ⌘Z ⌘⇧Z; shortcuts skipped while a menu is open | `t_shell::space_toggles_pattern_play_and_ignores_repeat`, `m_toggles_the_loop_metronome_and_n_note_repeat`, `shift_backspace_clears_the_pattern_and_cmd_z_undoes`, `tab_switches_view_b_toggles_browser…`, `single_key_shortcuts_are_skipped_while_a_menu_is_open`, `mode_shortcuts_and_sidebar` |
| §0 ⌘E (export dialog), command palette entries | not ported: export dialog and palette are phase 9 (listed below) |
| §0 browser "assign" | `t_browser::keys_move_enter_assigns…`, `double_click_assigns…` |
| §1 persisted prefs (layout, grid, key hints; invalid → default) | `t_shell::preferences_read_with_fallbacks_and_write_on_change` |
| §1 computer-keyboard pads (R, Shift pass-through, repeat, held, key-up, blur, scoped targets) | `t_shell::computer_keys_play_pads_record_and_release`, `window_blur_releases_held_keys`, `pad_keys_are_ignored_while_typing_in_a_field`, `t_browser::double_click_assigns_and_pad_keys_stay_in_the_browser` |
| §1 layout, pads header, count-in overlay, recording tint, Clear disabled without notes | `t_pads::grid_rows_follow_the_layout`, `count_in_overlay_while_counting`, `t_steps::recording_tints_the_grid`, `clear_is_disabled_without_notes_and_clears`, `t_menus::a_loaded_kit_cancels_a_pending_get_and_load` (chip hides the 4×4 layout toggle) |
| §2 play / stop / record buttons and labels, LCD, loop position | `t_transport::play_button…`, `record_button…`, `loop_position_follows_the_transport_and_resets`, `t_pads::count_in_overlay…` |
| §2 BPM field (wheel, drag 2 px, click-to-type, Enter, invalid, Escape, arrows ±1/±10, clamp 40–240, commit on blur) | `t_transport::bpm_wheel_drag_click_to_type_and_keys`, `bpm_clamp_matches_min_max`, `t_lifecycle::a_typed_bpm_commits_when_its_field_loses_focus` |
| §2 bars, quantize, count-in, record mode, metronome, swing knob (drag, %, reset, wheel), swing grid, note repeat + rate, captions by width | `t_transport::loop_settings_controls`, `swing_knob_and_grid`, `note_repeat_toggle_and_rate`, `transport_section_captions_follow_the_width` |
| §3 grid rows (4×4 / 2×8 + bank gap) | `t_pads::grid_rows_follow_the_layout` |
| §3 hit flash on every live hit | `t_pads::a_hit_flashes_the_pad`, `t_editor::header_shows_trim_readout_and_audition_plays` |
| §3 pointer down: select + velocity from height; release once | `t_pads::pointer_down_selects_and_hits_with_velocity_from_the_click_height` |
| §3 Enter / Space hold | `t_pads::enter_and_space_on_a_focused_pad_hold_and_release` |
| §3 right-click menu at the pointer, inside the window, Escape | `t_pads::right_click_selects_and_opens_the_context_menu_at_the_pointer` |
| §3 drops (Finder file; browser row) | `t_pads::a_file_dropped_on_a_pad_is_assigned_and_selected`, `t_browser::a_row_dragged_onto_a_pad_assigns_it_there` |
| §3 labels / aria / empty / hardware note / key caps | `t_pads::pad_labels_and_a11y`, `empty_pad_reads_empty`, `key_hint_caps_show_with_the_toggle` |
| §3 PadThumb drawing | screenshots (no headless pixel assertion) |
| §4 context menu (rename, colour, reset, choke 0–4, copy / paste, chop, clear, disabled states) | `t_menus::rename_trims_and_ignores_empty`, `colour_reset_choke_copy_paste_chop_clear`, `paste_is_disabled_without_a_clipboard` |
| §5 knobs (drag one undo step, Shift fine, wheel w/o end + 700 ms close, double-click, keys, log decay / filter, formats) | `t_editor::gain_knob_drag_is_one_undo_step_and_formats`, `shift_drag_is_four_times_finer`, `knob_wheel_double_click_and_keys`, `decay_and_filter_knobs_write_log_values` |
| §5 play mode / reverse / choke 0–8 | `t_editor::play_mode_reverse_and_choke` |
| §5 trim handles (drag, null at the end, keys, double-click reset) | `t_editor::trim_handles_drag_keys_and_double_click_reset`, `trim_patch_rules` |
| §5 header readout, audition, empty texts | `t_editor::header_shows_trim_readout…`, `empty_pad_editor_text` |
| §5 chop panel (modes, text, apply as one undo step) | `t_editor::chop_panel_slices_and_applies_as_one_undo_step` |
| §6 geometry, click / paint / erase (one step each), Alt-drag velocity, Alt-click empty, wheel burst | `t_steps::geometry_matches_the_ts`, `click_toggles_and_drag_paints_then_erases…`, `alt_drag_sets_velocity…`, `wheel_nudges_velocity_on_filled_cells_as_a_burst` |
| §6 keys (move, Shift beat jump, select row, Space, −, repeat ignored, End, ⌘Home), announcement | `t_steps::keyboard_moves_toggles_sets_velocity_and_announces` |
| §6 row labels, step size, loop hint + Loop pattern, playhead | `t_steps::row_labels_select_and_step_size_changes_steps`, `loop_hint_and_loop_pattern`, `playhead_moves_in_whole_steps_while_playing` |
| §7 useBeat actions (undo labels "Edit pad", "Paste pad", "Chop to pads", "Paint steps", "Erase steps", "Step velocity", "Loop pattern", "Clear pads", "Replace pads") | the tests above assert each label |
| §7 togglePlay / toggleRecord / starter drum track | `t_transport::*`, `t_shell::starter_project_has_one_active_drums_track` |
| §8a drum-track menu (select, new, numbered labels) | `t_menus::drum_track_menu_selects_and_adds_tracks` |
| §8b kit menu (classic, factory, save / overwrite / "My kit", delete, prefill) | `t_menus::kit_menu_loads_classic_and_factory_kits_and_saves_user_kits` |
| §8c missing packs (strip, Get, titles, unavailable clear / replace) | `t_menus::missing_pack_strip_get_button_and_titles`, `unavailable_pack_clear_and_replace_are_one_undo_step` |
| §8d pending-load chip | `t_menus::a_loaded_kit_cancels_a_pending_get_and_load` |
| §8f pad buffer statuses | covered through the editor / thumb tests (loading, ready, empty); missing-file text by `empty_pad_editor_text`'s sibling paths, not separately |
| §9 dock (collapse, routing mode, on-screen keys through the router, glissando, release on leave / unmount) | `t_shell::dock_expands_and_routing_mode_changes`, `t_lifecycle::the_dock_keyboard_glides_and_releases_when_the_pointer_leaves` |
| §10 pure helpers | phase 1 golden tests (`mdaw_model::beat_helpers*`) |
| §11 / §12 flash store, note repeat mirror | `t_pads::a_hit_flashes_the_pad`, `t_transport::note_repeat_toggle_and_rate` |
| §13 legacy DrumPads | not rendered by the app (inventory note) |
| §14 Song TransportBar | phase 8 |
| §15 top bar (undo / redo, dirty dot, view switch, browser toggle) | `t_shell::undo_redo_buttons_and_dirty_dot`, `tab_switches_view…` |
| Start screen (click, any key, Tab / Escape / chords ignored, texts) | `t_shell::start_screen_starts_on_click…`, `start_screen_texts` |
| Sample browser (listing order, keys, Enter / double-click, drag to pad, filter, width) | `t_browser::*` (incl. `each_filter_term_matches_on_its_own…`, `listings_run_off_the_ui_thread…`), `t_lifecycle::closing_the_browser_stops_its_preview` |
| Selects as keyboard listboxes (quantize, repeat rate, choke, MIDI input) | `t_transport::select_lists_work_from_the_keyboard` |
| ARIA ranges / value texts / increment–decrement (knobs, trim handles, BPM, volume, chop sensitivity, progress ring), pad `aria-selected` | `t_a11y::*` |
| Held pads released on unmount / focus loss; dock glissando; quit flush; catalog for missing packs | `t_lifecycle::*` |
| Fonts (one family per face, weights distinct, Syne 800) and line heights | `theme::tests::*` |

## Real device check

`MDAW_SMOKE=1 MDAW_DATA_DIR=<tmp> cargo run -p mdaw-app` opens the real window, starts the
session (MIDI enabled), hits pad A1 through the router, toggles two steps and plays the pattern,
then prints and quits. On this Mac:

```
[smoke] pad A1 hit
[smoke] device output peak after the pad hit: 0.3011
[smoke] device Some(StreamInfo { device_name: "MiniFuse 4", device_rate: 44100, context_rate: 44100, resampling: false, channels: 6, sample_format: "f32", buffer: Fixed(256), … })
[smoke] stats StatsSnapshot { callbacks: 696, frames: 178176, last_frames: 256, skipped: 0, render_panics: 0, … }
[smoke] started=true transport=Playing position=4.00 pattern output peak=0.2188 midi devices=["MiniFuse 4"]
```

The pad and the pattern reach the device's output buffer (peaks measured in the render callback).
I did not listen to it. After the review the same run quits through the new close / quit hook
(`App::shutdown`: transport stopped, notes off, pending autosave written, installs paused) in
5.3 s from launch, and the data folder holds the arrangement with the two toggled steps.

## Quit and close

`main` runs `rune::App::new(spec).on_close(..).on_quit(..)`, both calling `App::shutdown` once:
a typed BPM commits, held pads / keys / the dock note are released, the preview stops, the
router sends all-notes-off, a playing transport stops, the 300 ms autosave is flushed at once
(`useArrangementPersistence`'s `beforeunload` / `pagehide` flush) and `PackService::pause_for_quit`
pauses running installs (Electron's `before-quit`). Tested by
`t_lifecycle::shutdown_flushes_the_pending_autosave_and_stops_everything`; the hook wiring
itself only by the smoke run above.

## Electron bugs

None found in this phase or its review (`src/` is unchanged; `scripts/parity-phase-7/steps.mjs`
selects Low Tom for the pad-selected state).

## Deviations and gaps (what the user notices)

Kept / by design:
- **Data folder**: the Rust app keeps its settings (`local-storage.json`, same keys) and packs in
  `<data dir>/micro-daw-rune` (`MDAW_DATA_DIR` overrides), not in Electron's `micro-daw` profile.
  Importing the Electron profile stays a phase-10 row.
- **Start screen**: the intro animation (trace → mark) is not ported; the screen shows its final
  state (what reduced motion shows) and fades out in 380 ms.
- **Tooltips** are Rune's `.help` (AppKit tooltips in a window): same texts and shortcut keycaps,
  the system's delay and look instead of Radix's 350 ms popover.
- **Selects** (quantize, repeat rate, choke, MIDI input) open as a list below the trigger; Radix
  opens the quantize list aligned on the chosen item.
- **Fonts**: the latin subsets only (as `@fontsource`'s latin files); glyphs outside them (→ ↵ ⌘ ⇧ ⌫ ␣)
  fall back to the system font, as Chromium falls back. Italic "Empty" is a synthetic oblique
  (Manrope has no italic, Chromium synthesises one too). Every weight is its own face now
  (typographic names added by the converter); Syne 800 "micro" measures 177.6 points with the
  −1.2 tracking (Chromium 181).
- **Text metrics**: Rune lays out with fractional advances; a few labels wrap differently
  (2×8 "Open HH" / "Sub Kick").
- **Pads**: the key cap shows with the toggle (`data-show-keys`) but not on hover / focus of a
  single pad; the "loading" pulse of thumbnails is not drawn.
- **Pad context menu** keeps 4 points inside the window using an estimated menu height.
- **Browser row drags** draw a small name label at the pointer instead of the OS drag image.
- **Select popups**: opened with the pointer, the first arrow key shows the highlight on the
  chosen option and the next one moves it (Rune's popup rule, as AppKit); Radix moves at once.
- **Instrument dock**: the keyboard is drawn simply (no per-key labels except C's).

Not in this phase (listed, not dropped):
- Instrument selector popover (preset browser), synth editor, controller setup dialog, export
  dialog (⌘E), command palette (⌘K), project save / open / new / recent (the project name is
  "Untitled"; the dirty dot follows the doc), full-screen state sync, Samples / Packs / Settings /
  Developer pages (placeholders), Song view: phases 8–10.
- Sample browser: type-to-filter from the list, the pinned "Installed packs" root and the
  Music / Downloads / Splice suggestions; the list is not virtualised.

## Verified / not verified

**Verified here (tested):** the 85 `Runtime` tests + 3 theme tests (`cargo test -p mdaw-app`,
3.5 s), every line of the inventory table above that names a test; the accessibility tree
(ranges, value texts, actions, selection) for the controls listed in `t_a11y`; the quit flush
(`App::shutdown`) in a test; fonts and line heights in unit tests.

**Verified here (looked at, not asserted):** 18 screenshot pairs + 1 Rune-only state inspected
side by side after the review fixes (remaining differences listed under Screenshots); the real
window on this Mac (MiniFuse 4, 44.1 kHz): start, pad hit and pattern reach the device output,
MIDI enabled and the device listed, quit through the close / quit hooks.

**Not verified:**
- Audible output and felt latency (measured in the callback only).
- The Akai MPK mini Play mk3 driving the Beat view (pads, knobs, note repeat): the user's hardware.
- Real-pointer feel: drags, wheel notches, trackpad scrolling and the dock glissando were driven
  headlessly.
- Finder drags onto pads in a real window (headless drops only).
- VoiceOver against the new accessibility semantics (asserted on the tree, not with a reader).
- The physical-KeyR branch of the record shortcut (headless keys carry no physical code; the
  pure helper is unit-tested).
- Get & load / downloads from the Beat view against the live catalog (the phase-6 service is
  tested; here only with the bundled catalog and no network).
- Windows / Linux.

Review record: [review-phase-7.md](review-phase-7.md).
