# Phase 8: Song view (`rune/crates/mdaw-app`) vs the Electron app

**Scope** (plan §4 row 8, [`inventory/views.md`](inventory/views.md) §5–8, `beat-ui.md` §14):
timeline / ruler, tracks (header controls, add / remove, colours), regions (select, move,
resize, split, duplicate, copy / paste, delete, multi-select, rubber band, snapping, quantize),
groups, markers, loop region, automation lanes, track FX drawer, mixer, Song transport bar,
piano roll (notes, velocity lane, selection, quantize, keyboard shortcuts), on-screen keyboard
and the Song dock. Built on top of the Phase 7 shell (merged with its review fixes, c7bb8af).

## Method

**Modules** (new unless noted):

| Module | Port of | Notes |
|---|---|---|
| `song.rs` | `ArrangementView` state, `arrangementActions`, `App.tsx` Song state | `SongUi` (zoom, snap, tool, ordered selection, own scroll, drags, FX drawer, mixer, `editingRegion`), every action through `ArrangementEngine::edit` (undo labels are the doc's), row layout in render order, the FX parameter table, LCD formats, frame work (`song_poll`: LCD / playhead live values, auto-follow, mixer levels) |
| `piano.rs` | `PianoRollEditor`, `NoteGrid`, `VelocityLane`, `PianoRollRuler`, key columns | settings (`micro-daw-piano-roll`), viewport, fit, follow, the module-level clipboard, keys, every gesture with the TS commit labels; maths from `mdaw_model::piano_roll` |
| `song_keys.rs` | `App.tsx` Song shortcuts, ArrangementView window keys, the editor's keys | Escape cancels a grid gesture before closing the editor (capture phase) |
| `ui/song/{mod,toolbar,timeline,headers,menus,fx,mixer,transport,dock}.rs` | `components/arrangement/*`, `mixer/*`, `TransportBar`, `InstrumentDock` (Song) | the timeline scrolls itself and draws in viewport points (only what shows is built); ⌘ / Ctrl + wheel zooms about the pointer |
| `ui/piano/{mod,toolbar}.rs` | `piano-roll-editor/*` | the TS canvases are Rune shapes in viewport points; notes are elements |
| `scenes_song.rs`, `scripts/parity-phase-8/` | — | the 17 screenshot states, same steps in both apps |
| `icons_song.rs`, `tools/gen-icons-song.mjs` | lucide | the 13 extra icons |
| `mdaw-engine`: `ArrangementEngine::preview_note`, `Command::DrumNote` | `previewNote` | fixed-length audition through the track's own instrument (drum pad with gate release, or synth on / off); `engine/tests_preview.rs` |

**Comparison method.** `scripts/parity-phase-8/run.sh` renders each state with
`scripts/ui-snap.mjs` (the Electron renderer in headless Chromium, `steps.mjs`) and
`micro-daw --snap --scene` (1440×900, scale 1), both over the rock-song template. I looked at
every pair (half-scale montages and full-size crops). Interactions: 67 `Runtime` tests in
`src/tests/t_song_*.rs` and `t_piano.rs` drive the real view (pointer, wheel, keys, paste) and
assert the doc / engine state and undo labels. Real device: `MDAW_SMOKE=song`.

## Screenshots (`docs/parity/phase-8/`)

| State | Pair | Remaining differences |
|---|---|---|
| Song view, rock-song | `song-*` | Electron shows the Live FX strip (`EffectsPanel`, phase 9) above the dock, so its lanes are 70 points shorter; top-bar left cluster offset (Phase 7 top bar) |
| Region selected | `song-region-selected-*` | Electron's ring is outside the box (`box-shadow`), Rune's 1.5-point border is inside |
| Split / duplicate / new track + ⌘A | `song-regions-edited-*` | as above |
| Automation lane (Bass volume) | `song-automation-*` | none seen |
| Markers + loop 8–24 on | `song-markers-loop-*` | none seen |
| Track FX drawer | `song-track-fx-*` | knob row spacing ±2 points |
| Piano roll (Bass) / all selected / drums | `song-piano-roll-*`, `song-piano-velocity-*`, `song-piano-drums-*` | Live FX strip (phase 9) changes the grid height, so the vertical scroll differs by a few rows |
| Dock with C4 + E4 held | `song-dock-keys-*` | none seen |
| Mixer | `song-mixer-*` | none seen |
| No tracks | `song-empty-*` | empty-state buttons 4 points higher |
| Automation menu, instrument popover | `song-automation-menu-*`, `song-instrument-menu-*` | cmdk highlights the first item; Rune doesn't until the pointer / keys move |
| Region context menu | `song-region-menu-*` | Chromium makes this fixed menu 380 points wide (shrink-to-fit of `w-full` buttons); Rune sizes it to its content (140) |
| Playing | `song-playing-*` | playhead position differs with wall-clock timing; Electron's pressed Play shows `aria-pressed:bg-surface-3` over the green tint |

Fonts: with Phase 7's regenerated faces and smoothing Off the text weight now matches Chromium
(the heavier text of the first captures was the font family split, fixed in review 7 #1–2).

## Interaction tests (`rune/crates/mdaw-app/src/tests/`)

Each line of `views.md` §5–8 → test(s).

| Inventory | Test(s) |
|---|---|
| §5 constants (224 / 28), defaults (30 px/beat, snap 1, pointer), "Tracks" + count | `t_song_arr::constants_and_defaults` |
| §5 auto-follow (50-point margin, `playheadX − 100`, clamped) | `auto_follow_scrolls_when_the_playhead_leaves_the_view` |
| §5 ⌘ / Ctrl + wheel zoom about the cursor; wheel scrolls | `cmd_wheel_zooms_keeping_the_beat_under_the_pointer` |
| §5 window keys ⌘A, Delete / Backspace (selection or context-menu region), Escape, V / C | `window_keys_select_all_delete_escape_and_tools`, `delete_removes_the_region_under_the_context_menu` |
| §5 empty state | `header_empty_state` |
| §5 automation header cells, render order (groups first), collapsed groups | `t_song_timeline::automation_lane_click_adds_right_click_deletes`, `t_song_arr::rubber_band_counts_group_rows_and_skips_collapsed_groups` |
| §5 rubber band (3 px, Shift adds, click clears) | `rubber_band_selects_regions_of_rows_below_the_first_track` |
| §5 region context menu (split at playhead, duplicate, copy, paste only with a clipboard, quantize not on audio, delete) | `region_menu_items_and_actions`, `region_menu_hides_quantize_on_audio_regions` |
| §5 marker menu (rename Enter / Escape / trimmed / empty ignored, 8 colours, delete) | `t_song_timeline::marker_menu_rename_enter_escape_colour_delete`, `marker_menu_colour_and_delete` |
| §5 ruler click seeks snapped + clears the selection; tooltip | `ruler_click_seeks_snapped_and_clears_the_selection` |
| §5 bar labels every N bars, beat ticks ≥ 14 px | `ruler_labels_every_n_bars_and_beat_ticks_from_14_px` |
| §5 loop region always, handles only when on, snapped, ≥ 1 beat | `loop_region_always_drawn_handles_only_when_on`, `loop_handles_drag_snapped_one_undo_step_and_at_least_one_beat` |
| §5 marker flag tooltip, click jumps, drag (snapped, one "Update Marker"), right-click | `marker_flag_click_jumps_drag_moves_right_click_edits` |
| §5 marker jump dropdown (hidden without markers, bar.beat, picking seeks), ⇧M names, ] / [ | `marker_keys_and_jump_dropdown` |
| §5 toolbar (+ Synth, + Drums, Marker, Group from selection or all, tools, snap 1/16…Bar, zoom ±10 with limits, % reset) | `toolbar_adds_tracks_markers_groups_and_picks_tools`, `group_button_uses_the_selected_regions_tracks`, `snap_select_and_zoom_buttons` |
| §5 TrackHeader: 64 px, Beat chip / name → active drum track, FX one drawer at a time, automation menu (add / remove / Clear all), delete | `t_song_tracks::header_size_strip_and_drum_chip`, `fx_button_toggles_one_drawer_at_a_time`, `automation_menu_adds_removes_and_clears_lanes`, `delete_button_removes_the_track` |
| §5 instrument popover (search, vibe chips, Drums / synth presets, "No sounds found."), audio tracks | `instrument_popover_search_vibes_and_choices`, `audio_tracks_show_audio_and_no_arm` |
| §5 arm (null on second click) / M / S, volume slider + readout, PanDial (80 px/unit, double-click, ±0.05, labels) | `arm_mute_solo_toggles`, `volume_slider_and_readout`, `pan_dial_drag_double_click_keys_and_label` |
| §5 GroupHeader (collapse, double-click rename, group mute / solo, Ungroup), ⌘G | `group_header_collapse_rename_mute_solo_ungroup`, `cmd_g_groups_every_track` |
| §5 RegionBlock: select / Shift toggle, move snapped ≥ 0, edge resize with minimum, cut tool, double-click → piano roll (not audio), sample badge | `t_song_arr::region_click_selects_and_shift_toggles`, `region_move_is_snapped_clamped_and_one_undo_step`, `region_edges_resize_with_a_minimum_length`, `cut_tool_splits_at_the_snapped_beat`, `double_click_opens_the_piano_roll_except_audio`, `scene_region_selected_shows_the_selection` |
| §5 automation lane (click adds snapped value `1 − y/48`, right-click deletes, no rubber band), playhead (dimmed / red) | `t_song_timeline::automation_lane_click_adds_right_click_deletes`, `playhead_is_dimmed_while_stopped_and_red_while_recording` |
| §5 TrackEffectsPanel (header, sections on → expanded, switches, chevrons, 45 % off, selects, knob table, reverb 0.25 vs 0.3) | `t_song_mix::fx_drawer_header_sections_and_close`, `fx_selects_filter_type_and_lfo_wave`, `fx_param_table_matches_the_ts`, `fx_knob_drag_is_one_undo_step` |
| §6 editor: gone region, settings persisted, clipboard survives, fit (rows 12–22), status / corner / gutter texts | `t_piano::a_region_that_is_gone_offers_the_way_back`, `settings_persist_and_the_clipboard_survives_reopening`, `open_fits_and_closes_with_back_or_escape`, `toolbar_selects_quantize_state_zoom_rows_and_shortcuts` |
| §6 keys (⌘A / C / X / V / D, Delete, ↑↓ ±1 / ±12, ← → / Alt, Q, V / D / B / E, F, = / −, Space; Escape closes) and selection cleared by outside changes | `keys_select_copy_cut_paste_duplicate_delete_transpose_nudge`, `tool_keys_quantize_fit_zoom_and_space` |
| §6 wheel (⌘ X zoom, Alt Y zoom, ruler horizontal, keys vertical) | `wheel_scrolls_and_zooms_like_the_ts` |
| §6 NoteGrid gestures and labels (Draw Note(s), Move Note(s), Duplicate Notes, Resize Note, Delete Note(s), band, double-click add / delete, draw length follows, Escape cancels, eraser drag) | `draw_tool_click_and_drag_sets_the_length`, `pointer_gestures_select_move_duplicate_resize_double_click_and_band`, `eraser_drag_and_right_click_erase_escape_cancels`, `drum_regions_paint_along_a_row` |
| §6 ruler (cue snapped while stopped, strip click toggles loop, drag draws a loop and switches it on) | `ruler_sets_the_cue_draws_and_toggles_the_loop` |
| §6 velocity lane (set, offset of a multi-selection, Alt / empty ramp, "Edit Velocity", tooltip) | `velocity_lane_set_offset_and_ramp` |
| §6 toolbar (grid options, Off disables Quantize, custom Length item, tools, follow, zoom, rows ±2, 23 shortcuts) | `toolbar_selects_quantize_state_zoom_rows_and_shortcuts` |
| §6 key columns (audition by drag, tooltip, drum rows) | `key_column_auditions_and_lights_selected_pitches`, `drum_regions_paint_along_a_row` |
| §7 on-screen keyboard (C2–C6, velocity 0.7, glissando, release on up / leave / blur / unmount) | Phase 7's keyboard is reused: `t_lifecycle::the_dock_keyboard_glides_and_releases_when_the_pointer_leaves`; Song dock: `t_piano::song_dock_pads_keys_bank_and_collapse` |
| §8 mixer (X, header, empty, strips, fader jump / grab / Shift / keys / double-click, pan knob, M / S md, muted 70 %, meters + master, CLIP) | `t_song_mix::mixer_toggles_with_x_and_shows_strips`, `mixer_empty_state`, `fader_click_jumps_grab_keeps_offset_keys_and_double_click`, `mixer_pan_knob_mute_solo_and_meters` |
| `beat-ui.md` §14 TransportBar (buttons, disabled states, tooltips, PUNCH / REC, LCD formats, time ≥ 1440, BPM field) and Space / R / M / L | `transport_buttons_states_and_tooltips`, `lcd_formats_and_time_width`, `bpm_field_wheel_drag_and_type`, `song_keys_space_r_m_l` |

## Real device check

`MDAW_SMOKE=song MDAW_DATA_DIR=<tmp> cargo run -p mdaw-app` (real window, MiniFuse 4 at 44.1 kHz):

```
[smoke] song view: rock-song playing
[smoke] song transport=Playing position=7.88 tracks=4 song output peak=0.4838
```

The template plays through the device (peak measured in the render callback); I did not listen.

## Electron bugs (fixed in both apps, user decision 2026-11-18)

1. **A drag pushed one undo entry per pointer move** (region move / resize, loop handles,
   track-FX knobs): undoing a drag took dozens of steps. Now each drag is one step ("Move
   Region", "Trim Region", "Set Loop Markers", "Change Track Effect"); a press without movement
   leaves none. TS: `RegionBlock.tsx`, `TimelineRuler.tsx`, `Knob.tsx` (`onChangeStart`),
   `TrackEffectsPanel.tsx`. Tests: `src/components/arrangement/songUndoGroups.test.tsx`;
   Rust `region_move_is_snapped_clamped_and_one_undo_step`, `region_edges_resize…`,
   `loop_handles_drag…_one_undo_step…`, `fx_knob_drag_is_one_undo_step`.
2. **The rubber band selected nothing below a row above it**: `handleLanesPointerUp`
   returned before advancing its running top, used the doc order and ignored group header rows
   and collapsed groups. Now `regionsInBand` (exported from `ArrangementView.tsx`) walks the
   rows as drawn. Tests: `songUndoGroups.test.tsx` (`regionsInBand`); Rust
   `rubber_band_selects_regions_of_rows_below_the_first_track`,
   `rubber_band_counts_group_rows_and_skips_collapsed_groups`.

Found and kept (judgment call, documented): **the arrangement playhead reads 0 while stopped**
(`getPosition()` returns 0 and `play()` starts at bar 1), so a ruler click or marker click while
stopped moves nothing visible and "Add marker at playhead" / "Paste" use beat 0. It is the
engine's design (the piano roll has its own cue for that reason); adding a song cue changes the
transport semantics of phase 3 in both apps, so it is left for a decision. Rune behaves the same.

## Deviations and gaps

Kept / by design:
- The timeline and the piano roll scroll themselves (no native scroll bars; wheel / trackpad
  only, as the TS grid's overlay scroll bars are hidden in practice).
- Context menus close on Escape / outside click as the TS; outside-clicking a marker rename
  commits it (the input's blur).
- The instrument popover filters by an in-order character match (cmdk's fuzzy score is not
  ported: order of results is the TS order, not by score).
- The region menu is sized to its content (Chromium: 380 points, see screenshots).
- REC pulse animations (`animate-rec-pulse`) are not drawn.
- Right-button drag erasing in the piano roll: a right click erases one note; drag-erasing
  needs the eraser tool (Rune's secondary button opens `on_secondary_press`, it has no drag).
- Marker flags: the hit area is the line plus its label's column (the DOM hit-tests the
  label box only).

Not in this phase (listed, not dropped):
- **Audio import (Audio button, ⌘I) and audio-region waveforms**: the Rust app has no sample
  manager yet; audio regions from projects are drawn with the "Sample file not found" badge
  and are silent. Sample loading belongs with the Samples view (phase 9).
- Export button / ⌘E (dialog, phase 9); the Live FX strip `EffectsPanel` (phase 9).
- Seen in testing, not fixed: after renaming a marker, an immediate second right-click on the
  same flag in a headless run did not reopen the menu (a fresh open works); not reproduced in
  the window.

## Phase 7 files changed (for the merge)

`app.rs` (`Menu::Song`, `App::song`, `song_poll` in `poll`, busy while the mixer shows,
`dock_keys` tracking covers the Song dock), `keys.rs` (Song hooks: `song_keys::before_app` /
`daw_keys`, ⌘G, text fields; `is_char` public), `lib.rs` (modules), `scenes.rs` (17 Song scenes
appended), `tests.rs` + `tests/harness.rs` (Song helpers appended), `ui/mod.rs` (Song branch,
`Menu::Song` overlays), `ui/topbar.rs` (Song transport in the centre; placeholder removed),
`ui/knob.rs` (`KnobId::Song`, generic `on_change`, `knob_keyed`), `ui/dock.rs` (`keyboard`
public), `smoke.rs` (`MDAW_SMOKE=song`).

## Verified / not verified

**Verified here:** `npm test` 904 passed; `cargo test --workspace` 1,268 passed (22 ignored);
clippy `-D warnings` and `fmt --check` clean; 17 screenshot pairs inspected; the real window
plays the template on the device.

**Not verified:** listening; real-pointer feel (drags, trackpad scroll) — driven headlessly;
VoiceOver; Windows / Linux; MIDI recording into the Song view from the MPK (the path is the
phase-5 capture, exercised by engine tests, not by hand).
