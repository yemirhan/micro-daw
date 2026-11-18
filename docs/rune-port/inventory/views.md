# Parity checklist: micro-daw UI (Electron/React) for the Rust port

All paths are relative to `src/`. Citations are `file:line`. "svc" means the service or engine call a control makes.

Things worth knowing before you start:
- **Unused settings:** `audio.defaultMasterVolume`, `audio.bufferSizeHint` and `midi.autoConnectLastDevice` are saved, but nothing else in `src/` reads them. Only `general.autoCheckUpdates` has an effect (`app/App.tsx:88`).
- **One export dialog:** "Export as WAV" and "Export as MIDI" in the palette both just open the Export dialog (`app/App.tsx:286-287`).
- **Two ways to group:** Ctrl/⌘G and the palette's "Group Tracks" group every track. The toolbar "Group" button groups only the tracks of the selected regions, or all tracks if nothing is selected (`app/App.tsx:418-424` vs `components/arrangement/ArrangementView.tsx:285-302`).
- **Unused prop:** `TrackHeader` accepts `onToggleAutomationLaneVisibility` but never uses it (`components/arrangement/TrackHeader.tsx:66`). Clicking an item in the automation menu adds or removes the lane; it never just hides it.
- **"Record Loop" from the palette** switches to the Beat view but does not switch the mode back to DAW (`app/App.tsx:245-248`).

---

## 1. App shell (`app/App.tsx`)

### State and persistence
- [ ] `mode` (AppMode) can be `'daw' | 'samples' | 'packs' | 'settings' | 'dev'`. Default `'daw'`, not persisted (`hooks/useAppMode.ts:5`, `types/appMode.ts:2`).
- [ ] `dawView` can be `'beat' | 'song'`. Default `'beat'`, not persisted (`app/App.tsx:79`).
- [ ] Browser panel open: localStorage key `micro-daw:browser-open`, default true (only the string `'false'` closes it) (`app/App.tsx:50-58, 100-109`).
- [ ] Not persisted: `editingRegion`, `showExportDialog`, `showMixer`, `commandPaletteOpen`, `showTemplatePicker`, `synthEditorOpen`, `controllerSetupOpen` (`app/App.tsx:89-97`).
- [ ] Changing mode calls `allNotesOff()` (`app/App.tsx:198-203`).
- [ ] `useArrangement({position:false, tracks: dawView==='song', beat:false})` (`app/App.tsx:83`).

### Start
- [ ] `handleStart` runs `audio.start()`, then `drumEngine.start()`, then `midi.enable()` (`app/App.tsx:205-209`).
- [ ] Nothing but the StartScreen renders until `audio.started` (`app/App.tsx:486-488`). The StartScreen stays mounted as an overlay until its exit transition ends (`app/App.tsx:482-484`).
- [ ] At mount: window drop guard (`:172`), knob routing (`:148`), pack provider listing project kits (`:175-178`), dev-only debug hooks (`:185-194`).

### Layout (DAW mode)
- [ ] TopBar gets: midi devices and selection, master volume, preset, mute, synth editor toggle, undo/redo, project name/dirty/save, view switch, browser toggle, controller setup (`app/App.tsx:494-549`).
- [ ] The TransportBar sits in the TopBar centre only when `mode==='daw' && dawView==='song'`. Its record button is disabled when no track is armed. It shows punch recording when recording with the loop on (`app/App.tsx:528-548`).
- [ ] Sidebar (`app/App.tsx:553-561`).
- [ ] Browser panel on the left when `browserOpen` (`app/App.tsx:566-570`).
- [ ] Beat view: BeatView, an optional SynthEditor, and an InstrumentDock (`padsAvailable={false}`, `defaultCollapsed`) (`app/App.tsx:573-611`).
- [ ] Song view: PianoRollEditor if `editingRegion`, otherwise ArrangementView. Then MixerView if `showMixer`, then the SynthEditor (optional), EffectsPanel, and an InstrumentDock that is hidden while the piano roll is open (`app/App.tsx:613-683`).
- [ ] Dialogs: ExportDialog and TemplatePicker (DAW mode only), ControllerSetupDialog, CommandPalette (`app/App.tsx:688-711`).
- [ ] Other modes: SamplesView, PacksView, DevView, SettingsView (`app/App.tsx:713-740`).
- [ ] Each zone has an ErrorBoundary: Controls, Browser, Beat, Instrument, Piano Roll (reset closes the editor), Arrangement, Packs.

### Shared actions
- [ ] `togglePlay`: in the Beat view, `beatActions.togglePlay()`. In the Song view, stop if playing or recording, otherwise `arrangement.play()` (`app/App.tsx:161-169`).
- [ ] `toggleMetronome`: in the Beat view, `beatActions.setLoopSettings({metronome:!…})`. In the Song view, `arrangement.toggleMetronome()` (`app/App.tsx:157-160`).
- [ ] Browser "assign" calls `beatActions.assignFile(arrangementEngine.getSelectedPad(), {path,name})` (`app/App.tsx:151-154`).
- [ ] Choosing a template closes the picker and calls `project.newProjectFromTemplate(template.arrangement)` (`app/App.tsx:694-697`).

### Global keyboard shortcuts (`app/App.tsx:303-479`)
Active only once `audio.started`. Ignored when focus is in an input, textarea, select or contenteditable (`:66-73, :307`). "Mod" means ⌘ or Ctrl.

| Key | Action | Line |
|---|---|---|
| Mod+K | Toggle the command palette | 311 |
| Mod+S / Mod+Shift+S | `project.save()` / `project.saveAs()` | 318 |
| Mod+O | `project.open()` | 323 |
| Mod+N | `project.newProject()` | 324 |
| Mod+, | Toggle Settings (back to daw) | 327 |
| Mod+1/2/3/4 | Mode daw / samples / packs / dev | 334-337 |
| Mod+Z / Mod+Shift+Z | Undo / redo | 340 |
| Mod+I | `arrangement.importAudioFile()` | 351 |
| Mod+E | Toggle the Export dialog | 358 |

The shortcuts below work only in DAW mode, and not when the event was already handled or a modal is open (`[role=dialog|alertdialog][data-state=open]`) (`:365-367`).

| Key | Action | Line |
|---|---|---|
| Tab (no modifiers; not inside a scoped-key target or `[role=grid]`) | Switch Beat/Song | 373 |
| B | Toggle the browser | 379 |
| N | `noteRepeat.toggle()` | 385 |
| Beat view: Space (ignored on key repeat) | togglePlay | 393 |
| Beat view: M | toggleMetronome | 398 |
| Beat view: Shift+Backspace | `beatActions.clearPattern()` | 403 |
| Escape (piano roll open) | Close the piano roll | 411 |
| Mod+G | Create group "Group" from all tracks | 418 |
| Shift+M | Add a marker at the playhead, name cycles Intro/Verse/Chorus/Bridge/Drop/Breakdown/Outro/Section | 428 |
| ] / [ | Next / previous marker | 437-446 |
| Song view: Space | togglePlay | 450 |
| Song view: R | Stop recording if recording; start recording if stopped | 454 |
| Song view: M | `arrangement.toggleMetronome()` | 462 |
| Song view: L | `arrangement.toggleLoop()` | 466 |
| Song view: X | Toggle the mixer | 470 |

---

## 2. Command palette (`components/CommandPalette.tsx`, `config/commands.ts`)

- [ ] Dialog. Placeholder "Search commands, sounds, projects…". Empty state "No results found." (`components/CommandPalette.tsx:70-72`).
- [ ] Commands are grouped by `group` in the order they first appear. Search matches `label + keywords` (`components/CommandPalette.tsx:44-55, 79`).
- [ ] "Recent Projects" group: up to 10, with the file path in mono as a subtitle. Selecting one calls `onOpenRecent(path)` (`components/CommandPalette.tsx:92-111`, `config/commands.ts:118-127`).
- [ ] Selecting any item runs its action and closes the palette (`components/CommandPalette.tsx:57-66`).
- [ ] Footer hints: "↑↓ navigate", "↵ run", "esc close", "⌘K toggle" (`components/CommandPalette.tsx:114-119`).
- [ ] A command with `modes` is shown only in those modes (`config/commands.ts:114-116`).

Every command (`config/commands.ts:52-112`) and its action (`app/App.tsx:212-300`):

| id | Label | Group | Shortcut shown | Modes | Action |
|---|---|---|---|---|---|
| new-project | New Project | Project | ⌘N | all | project.newProject |
| open-project | Open Project | Project | ⌘O | all | project.open |
| save | Save | Project | ⌘S | all | project.save |
| save-as | Save As | Project | ⌘⇧S | all | project.saveAs |
| switch-daw | Switch to DAW | Navigation | ⌘1 | all | mode daw |
| switch-samples | Switch to Samples | Navigation | ⌘2 | all | mode samples |
| browse-packs | Browse Packs | Navigation | ⌘3 | all | mode packs |
| switch-dev | Switch to Developer | Navigation | ⌘4 | all | mode dev |
| toggle-view | Switch Beat / Song View | View | Tab | daw | daw + toggle |
| switch-beat | Beat View | View | – | daw | daw + beat |
| switch-song | Song View | View | – | daw | daw + song |
| toggle-browser | Toggle Sample Browser | View | B | daw | daw + toggle browser |
| play-stop | Play / Stop | Transport | Space | daw | togglePlay |
| loop-record | Record Loop | Transport | R | daw | beat view + beatActions.toggleRecord |
| record | Record to Track (Song) | Transport | – | daw | start or stop song record |
| toggle-metronome | Toggle Metronome | Transport | M | daw | toggleMetronome |
| toggle-note-repeat | Toggle Note Repeat | Transport | N | daw | noteRepeat.toggle |
| clear-pattern | Clear Pattern | Editing | ⇧⌫ | daw | beatActions.clearPattern |
| controller-setup | Controller Setup… | MIDI | – | all | open ControllerSetupDialog |
| undo / redo | Undo / Redo | Editing | ⌘Z / ⌘⇧Z | all | undoRedo |
| add-synth-track / add-drum-track / add-audio-track | Add … Track | Editing | – | daw | addTrack('synth' / 'drums' / 'audio') |
| import-audio | Import Audio File | Editing | ⌘I | daw | importAudioFile |
| add-marker | Add Marker | Editing | ⇧M | daw | addMarker (cycled name, playhead) |
| next-marker / prev-marker | Next / Previous Marker | Editing | ] / [ | daw | seek |
| create-group | Group Tracks | Editing | ⌘G | daw | group all tracks |
| new-from-template | New from Template | Project | – | all | open TemplatePicker (it only renders in DAW mode) |
| export-wav / export-midi | Export as WAV / MIDI | Export | ⌘E | daw | open Export dialog |
| toggle-fullscreen | Toggle Fullscreen | View | – | all | `electronAPI.toggleFullscreen()` |
| open-settings | Settings | View | ⌘, | all | mode settings |
| preset-i | (preset name) | Instrument | – | daw | `audio.changePreset(i)`; keywords include engine and category |

---

## 3. Start screen (`components/StartScreen.tsx`)
- [ ] Phases are idle, starting and exiting. Exit lasts 380ms, or 160ms with reduced motion, then `onExited` fires (`:109-115`).
- [ ] The animation pauses until the document is visible, with a 1500ms fallback (`:95-107`).
- [ ] Clicking anywhere starts. So does any key except Tab, Escape, Shift, Control, Alt, Meta and CapsLock, and nothing starts on key repeat or with a modifier held (`:143-162`).
- [ ] Button "Start session ↵". While busy it reads "Starting audio engine…" (`:228-248`).
- [ ] Status text "Click anywhere or press any key. This turns on audio and MIDI." On error: "Audio could not start. Click to try again." (`:138, :250`).
- [ ] Footer shows `v{version}` from `electronAPI.getAppVersion()`, otherwise "micro daw", plus "Audio engine · Standby / Starting / Error" (`:84-93, :256-266`).
- [ ] Wordmark "micro" + "daw"; tagline "Sample · Play · Record"; animated trace, then the mark (`:216-224`).
- [ ] Exits early when `ready` becomes true while starting (`:122-124`).

---

## 4. Sidebar (`components/sidebar/Sidebar.tsx`)
- [ ] Collapsible to icons; collapsed by default (`app/App.tsx:492`).
- [ ] Mode buttons with tooltip and shortcut: DAW ⌘1, Samples ⌘2, Packs ⌘3, Developer ⌘4. The active mode is highlighted (`:30-35, 53-65`). The modifier shows as ⌘ on Mac and "Ctrl+" elsewhere (`:27-28`).
- [ ] "Project" group: New (⌘N), which calls `onNewProject`; Open (⌘O), which calls `onOpen` (`:72-91`).
- [ ] "Recent" group (hidden when collapsed): one button per project showing its name, with the file path as tooltip. Calls `onOpenRecent(filePath)` (`:94-111`).
- [ ] Footer: Settings (⌘,), active when mode is settings (`:114-128`).

---

## 5. Song view: arrangement (`components/arrangement/`)

### ArrangementView (`components/arrangement/ArrangementView.tsx`)
- [ ] Constants: header column 224px, ruler height 28 (`:23-24`).
- [ ] Not persisted: `pxPerBeat` (default 30, range 10–80, `utils/constants.ts:81-83`), `snapValue` (default 1), `tool` (default pointer), selection set (`:94-97`).
- [ ] Auto-follow while playing or recording: when the playhead leaves the view (50px right margin), set `scrollLeft = playheadX − 100` (`:158-173`).
- [ ] Ctrl/⌘+wheel zooms by `−deltaY*0.5` px/beat, rounded and clamped, keeping the beat under the cursor in place (`:176-204`).
- [ ] Window keys (skipped in inputs) (`:207-257`):
  - Mod+A selects all regions.
  - Delete/Backspace deletes the selected regions, or the region under the open context menu.
  - Escape clears the selection.
  - V selects the pointer tool; C selects the cut tool.
- [ ] Header column: "Tracks" label plus track count (`:541-542`). Empty state "No tracks yet" / "Add a synth or drum track, arm it, and press R to record from your keyboard." with buttons "+ Synth track" and "+ Drums" (`:545-560`).
- [ ] Each visible automation lane gets a 48px header cell showing "↳" and the parameter label (`:429-439`).
- [ ] Render order: groups first (GroupHeader, a 28px tinted row in the lanes, then the group's tracks unless collapsed), then ungrouped tracks (`:467-508`).
- [ ] Clicking the ruler/lanes background clears the selection (`:569`).
- [ ] Rubber band on lanes: left button, starts after 3px. Selects regions whose x and track y overlap. Shift adds to the selection. A plain click on empty space clears the selection unless Shift is held (`:320-401, 599-609`).
- [ ] The grid continues below the last track (`:613`). LivePlayhead is dimmed while stopped and red while recording (`:616-621`).
- [ ] Track FX drawer opens on the right, one track at a time (`:627-631`).
- [ ] Region context menu items:
  - Split at playhead (engine position)
  - Duplicate
  - Copy, if available
  - Paste, at the playhead on that track, only when `arrangementEngine.hasClipboard()`
  - Quantize, with the current snap; not for audio regions
  - Delete (`:635-653`)
- [ ] Marker context menu: rename, change colour, delete (`:656-666`).
- [ ] Clicking the ruler seeks with `arrangementEngine.setPosition(beat)` (`:579`).
- [ ] Marker drag end calls `updateMarker({beat})`; clicking a marker calls `seekToMarker` (`:273-283`).

### ArrangementToolbar (`components/arrangement/ArrangementToolbar.tsx`)
- [ ] "+ Synth" (tooltip "Add a synth track"), "+ Drums", "Audio" (tooltip "Import an audio file", ⌘I) (`:63-79`).
- [ ] "Marker" ("Add marker at playhead", ⇧M) uses DEFAULT_MARKER_NAMES cycled by marker count (`:85-91`; `ArrangementView.tsx:266-271`).
- [ ] "Group" ("Group tracks (selected regions' tracks, or all)", ⌘G) (`:92-98`).
- [ ] Tool radio group: Pointer ("Pointer: select, move, resize", V) and Cut ("Cut: click a region to split it", C) (`:104-125`).
- [ ] Snap select (magnet icon) with options 1/16, 1/8, 1/4, 1/2, Bar (0.25, 0.5, 1, 2, 4) (`:35-41, 128-138`).
- [ ] Zoom out / zoom in step by ±10 px/beat and disable at the limits. The percentage button resets zoom to 30 (`:141-160`).
- [ ] "Export" ("Export WAV or MIDI", ⌘E) (`:165-169`).
- [ ] Undo/redo props are accepted but not rendered; undo/redo lives in the TopBar (`:43`).

### TimelineRuler (`components/arrangement/TimelineRuler.tsx`)
- [ ] 28px tall. Tooltip "Click to move the playhead". A click seeks to the snapped beat (`:49-55, 97`).
- [ ] Bar ticks are labelled with the bar number every N bars, N in {1, 2, 4, 8, 16, 32, 64}, so labels stay at least 32px apart. Beat ticks show only when pxPerBeat ≥ 14 (`:37-47`).
- [ ] The loop region is always drawn, highlighted when the loop is on. Start/end handles exist only when the loop is on ("Drag loop start/end"). Dragging snaps and keeps the loop at least 1 beat long (`:57-136`).
- [ ] Marker flags (`:156-168`).

### MarkerFlag (`components/arrangement/MarkerFlag.tsx`)
- [ ] Tooltip "{name}: click to jump, drag to move, right-click to edit" (`:77`).
- [ ] Pointer down; a move turns it into a drag (snapped, clamped to 0–length), otherwise it counts as a click on release (`:26-63`).
- [ ] Right-click opens the context menu (`:72-76`).

### MarkerContextMenu (`components/arrangement/MarkerContextMenu.tsx`)
- [ ] Closes on outside mousedown or Escape (`:29-44`).
- [ ] "Rename" turns into an inline input: Enter or blur commits (trimmed, non-empty), Escape cancels (`:66-88`).
- [ ] "Color" swatches from MARKER_COLORS (8 colours); picking one closes the menu (`:91-110`, `utils/constants.ts:87-96`).
- [ ] "Delete" in red (`:114-122`).

### MarkerJumpDropdown (used in TransportBar:272)
- [ ] Hidden when there are no markers. Trigger shows a flag icon and the count, tooltip "Jump to marker ( [ / ] )". Items show a colour bar, the name and `bar.beat`. The value is kept empty so picking the same marker again still jumps (`components/arrangement/MarkerJumpDropdown.tsx:15-44`).

### TrackHeader (`components/arrangement/TrackHeader.tsx`)
- [ ] 64px tall. Colour strip at 35% opacity when muted. Background highlighted while its FX drawer is open (`:135-143`).
- [ ] Drum tracks get a "Beat" chip plus a name button. Both call `arrangementEngine.setActiveDrumTrack(track.id)`. Tooltips: "Active drum track: the Beat view edits this one" / "Edit this drum track in the Beat view" (`:147-180`).
- [ ] FX button ("Track effects") toggles the drawer (`:190-203`).
- [ ] Automation button ("Automation lanes") opens a fixed menu headed "Automate" (`:206-271`):
  - One checkbox item per parameter: Volume, Pan, Reverb, Delay, Chorus, Distortion, Filter, EQ Low, EQ Mid, EQ High.
  - Clicking a parameter adds its lane (visible) or removes it if it exists.
  - "Clear all" removes every lane.
  - Closes on an outside pointerdown.
- [ ] Delete button ("Delete track") is visible on hover (`:273-283`).
- [ ] Instrument row: audio tracks show "Audio". Other tracks get a popover with search "Search sounds…" and vibe chips (All plus the vibes in use), groups "Drums" (Drum kit, sets `{type:'drums', presetIndex:0}`), and one group per vibe of synth presets (sets `{type:'synth', presetIndex}`). Empty state "No sounds found." (`:287-380`).
- [ ] Arm toggle (not on audio tracks): "Arm track for recording" / "Disarm track". It pulses while recording. Clicking arms this track, or sets armed to null (`:385-393`; `ArrangementView.tsx:421`).
- [ ] Mute (M) and Solo (S) toggles (`:394-395`).
- [ ] Volume slider −40…0 dB in steps of 1, with a readout (+ sign above 0) (`:396-408`).
- [ ] PanDial (`:418-470`):
  - Drag vertically: 80px per unit, clamped −1…1, rounded to 0.01.
  - Double-click centres it.
  - Arrow keys step ±0.05.
  - Label "C", "L{n}" or "R{n}".

### TrackToggle (`components/arrangement/TrackToggles.tsx`)
- [ ] Shows M, S, or a ● for arm. Active colours: mute amber, solo blue, arm red. Clicks stop propagation. Sizes sm (18px) and md (24px) (`:6-53`).

### GroupHeader (`components/arrangement/GroupHeader.tsx`)
- [ ] Collapse chevron ("Expand group" / "Collapse group") (`:53-62`).
- [ ] Double-click the name ("Double-click to rename") for an inline input: Enter or blur commits if non-empty, Escape cancels (`:65-90`).
- [ ] Track count shown in mono (`:88`).
- [ ] Mute toggles mute on all tracks; solo toggles solo for the group (`:92-93`).
- [ ] "Ungroup" (X) appears on hover (`:95-104`).

### RegionBlock (`components/arrangement/RegionBlock.tsx`)
- [ ] Pointer tool: pointer down selects the region (Shift toggles). Then:
  - Edge zones 6px wide resize from the left or right (only if resizing is allowed).
  - Otherwise the region moves.
  - Move is snapped with a minimum of 0. Resize keeps at least `snapValue || 0.25` beats (`:121-189`).
- [ ] Cut tool: a click splits at the snapped beat under the cursor. Cursor is a crosshair (`:136-144, 158-159`).
- [ ] Hover cursors: col-resize at the edges, grab elsewhere (`:160-167`).
- [ ] Double-click opens the piano roll (not for audio regions) (`:222-225`).
- [ ] Right-click opens the context menu (`:226-229`).
- [ ] Visuals:
  - Header strip when the region is at least 18px wide, showing the name or "Audio" / "Clip".
  - Selected: ring and a stronger tint. Muted track: 45% opacity and desaturated. Dragging: 75% opacity.
  - Resize hints when at least 24px wide (`:195-269`).
- [ ] Body: a waveform minimap for audio regions (needs bpm), otherwise a note minimap (pitch range fitted to the notes, minimum 8 rows, or 4 for drums) (`:56-96, 248-260`).
- [ ] Sample problem badge for `unavailable` or `missing`, with the exact tooltip strings at `:11-15` (`useSampleStatus` in `components/arrangement/sampleStatus.ts`).

### TrackLane, LiveRegionBlock, AutomationLaneView, Playhead
- [ ] Lane is 64px plus 48px per visible automation lane (`components/arrangement/TrackLane.tsx:115-118`). Background grid: bar lines, beat lines when pxPerBeat ≥ 14, alternate-bar shading (`:124-132`).
- [ ] Live recording region: red tint, pulsing "REC" header (`components/arrangement/LiveRegionBlock.tsx:12-36`).
- [ ] Automation lane (`components/arrangement/AutomationLaneView.tsx`):
  - Clicking adds a point at the snapped beat, value = 1 − y/48 (`:32-43`).
  - Right-clicking a point deletes it (`:51-58`).
  - Pointer down stops propagation, so it doesn't start a rubber band (`:47-49`).
  - Draws the line and the area under it (`:98-116`).
- [ ] Playhead head triangle; the primary colour, or red while recording (`components/arrangement/Playhead.tsx:15-31`).

### TrackEffectsPanel (`components/arrangement/TrackEffectsPanel.tsx`)
- [ ] 336px drawer. Header "Track FX" plus the track name; close X ("Close track effects") (`:96-106`).
- [ ] Every change calls `arrangementEngine.setTrackEffect(track.id, effect, params)` (`:76-78`).
- [ ] Each section has an on/off switch ("{Label} on/off") and an expand chevron. Sections that are on start expanded. Disabled sections render at 45% opacity (`:63-65, 210-249`).

| Section | Controls (min..max, step, double-click default) |
|---|---|
| Filter | type select LP/HP/BP/Notch/AP; Cutoff 60..18000 (18000); Reso 0..20 (1); LFO Rate 0.1..20 (1); LFO Dep 0..1 (0); LFO wave Sin/Tri/Sqr/Saw |
| EQ | Low / Mid / High −12..12, step 0.5 (0) |
| Compressor | Thresh −60..0 (−24); Ratio 1..20 (4); Attack 0.001..1 (0.003); Release 0.01..1 (0.25) |
| Chorus | Depth 0..1 (0.5) |
| Delay | Time 0.01..1 (0.25); Fdbk 0..0.95 (0.3); Mix 0..1 (0.3) |
| Distortion | Drive 0..1 (0.3); Mix 0..1 (0.5) |
| Reverb | Wet 0..1 (0.3) |

- [ ] Starting values come from DEFAULT_TRACK_EFFECTS (`types/effects.ts:73-81`). Note that the reverb wet starting value is 0.25 while the knob's double-click default is 0.3.
- [ ] Knob behaviour, shared by every Knob in the app (`components/ui/Knob.tsx`):
  - Drag vertically: 200px covers the full range; Shift makes it 4× finer.
  - Wheel step: the larger of `step` and 1.5% of the range.
  - Double-click resets to the default.
  - Arrow keys step by 1% of the range, or 10% with Shift.
  - Home/End jump to min/max. Delete/Backspace resets to the default.

---

## 6. Piano roll editor (`components/piano-roll-editor/`)

### PianoRollEditor (`components/piano-roll-editor/PianoRollEditor.tsx`)
- [ ] If the region no longer exists: "This region no longer exists." plus a "Back to arrangement" link (`:106-115`).
- [ ] Settings persist under localStorage `micro-daw-piano-roll` as `{snap: 0.25, drawLength: 1, follow: true}` (`:70-90, 189-195`).
- [ ] The note clipboard is module-level, so it survives closing and reopening the editor (`:68`).
- [ ] Viewport defaults: 64 px/beat (range 6–640); row height 16, or 30 for drums (range 7–40) (`:164-171`; `utils/pianoRollHelpers.ts:45-51`).
- [ ] Fits to the notes once the size is known and takes focus (`:329-334`). Fit clamps piano row height to 12–22 (`:316-326`).
- [ ] Selection and draft are cleared when the notes change from outside (undo, recording) (`:177-185`).
- [ ] Ghost notes show the other regions on the same track (`:204-211`).
- [ ] Follow playhead when enabled (`:341-346`).
- [ ] Ruler position: while playing it calls `onSeek`; while stopped it sets the cue (`:348-355`).
- [ ] Setting a loop calls `onSetLoop` and switches the loop on if it was off (`:358-364`).
- [ ] Every edit commits as one undo step with a label, via `editRegionNotes`, and grows the region if needed (`:372-381`).
- [ ] Note preview lasts 0.3s via `previewNote(trackId, …)` (`:92, 367-370`).
- [ ] Keyboard (handled on the focused root, skipped in text fields) (`:398-497`):

| Key | Action |
|---|---|
| Mod+A | Select all |
| Mod+C / Mod+X | Copy / cut (cut label "Cut Notes") |
| Mod+V | Paste at the playhead or cue, snapped ("Paste Notes") |
| Mod+D | Duplicate the selection ("Duplicate Notes") |
| Space | Stop if playing, otherwise play from the cue |
| Delete / Backspace | Delete the selection ("Delete Note(s)") |
| ↑ / ↓ | Transpose one row; Shift = 12 (piano only); drums move one row; previews a single note ("Transpose Notes") |
| ← / → | Nudge by the grid step ("Move Notes"); Alt changes length, piano only ("Change Length") |
| Q | Quantize the selection, or all notes |
| V / D / B / E | Pointer / draw / draw / eraser |
| F | Fit |
| = or Numpad+ / − or Numpad− | Zoom ×1.25 / ÷1.25 |

- [ ] Wheel (`:280-314`):
  - Mod+wheel zooms X at the cursor.
  - Alt+wheel, or Mod+Shift+wheel, zooms Y.
  - On the ruler and velocity lane, the wheel scrolls horizontally. On the keys it scrolls vertically. On the grid it scrolls natively.
- [ ] Status text (`:506-513`):
  - One note selected: "{row label} · {position} · {length} · vel {0-127}".
  - Otherwise, while hovering: the cursor position · pitch.
- [ ] Corner cell shows "grid {len}" or "grid off" (`:557`).
- [ ] Velocity gutter labels "Vel", 127, 64, 1 (`:621-634`).

### PianoRollToolbar (`components/piano-roll-editor/PianoRollToolbar.tsx`)
- [ ] "Back" button ("Close editor (Esc)"); region name and track name with a colour dot (`:113-123`).
- [ ] Tools: Select (V), Draw (D), Erase (E) (`:29-33`).
- [ ] "Grid" select with 1/1, 1/2, 1/4, 1/8, 1/16, 1/32, 1/4 T, 1/8 T, 1/16 T, 1/32 T, Off (`:137-154`; `utils/pianoRollHelpers.ts:53-65`).
- [ ] "Length" select (piano only): 1/1, 1/2, 1/4 ., 1/4, 1/8 ., 1/8, 1/16, 1/32, 1/4 T, 1/8 T, 1/16 T. When the length is not one of these it adds a "custom" item (`:156-183`; `utils/pianoRollHelpers.ts:68-79`).
- [ ] "Quantize" is disabled when snap is off or there are no notes (`:185-195`).
- [ ] Status text (≥ xl width); "n/N sel" or "N notes" (`:199-204`).
- [ ] Follow toggle; zoom out; zoom slider 0–1000 (log mapping); zoom in; shorter rows (−2px); taller rows (+2px); fit (`:208-234`).
- [ ] Shortcuts popover "Piano roll shortcuts" with 23 rows (exact text at `:242-266`).

### NoteGrid (`components/piano-roll-editor/NoteGrid.tsx`)
- [ ] Canvas drawing (`:147-305`):
  - Rows shaded for black keys; hovered row highlighted; alternate-bar shading.
  - Row lines when row height ≥ 9 (always for drums); octave lines at C (piano).
  - Grid lines at 3 levels; the area past the region end dimmed at 55%.
  - Ghost notes; notes coloured by velocity in 12 buckets (drums use per-row colours).
  - Velocity tick along the note bottom (piano, width > 10).
  - Note names when row height ≥ 11 and width > 28.
  - Dashed cue line; playhead; rubber band.
- [ ] Minimum note width 4px, or 9px for drums. Drag threshold 3px (`:27-29`).
- [ ] Escape (capture phase) cancels the gesture in progress (`:330-341`).
- [ ] Pointer down (`:377-489`):
  - Right button or the eraser tool: erase, and keep erasing while dragging.
  - Double-click on a note body with the pointer tool deletes it.
  - Clicking a note: Shift toggles it in the selection; clicking a selected note without Shift collapses the selection on release. It previews the note.
  - Note edge: resize (piano only).
  - Note body: move. Alt+drag duplicates.
  - Empty space with the draw tool, or a double-click with the pointer tool, adds a note. Piano: drag to set the length. Drums: paint along the row.
  - Empty space with the pointer tool: rubber band (Shift adds).
- [ ] Holding Mod while dragging bypasses snap. Alt also bypasses it for everything except move (`:535-536`).
- [ ] Auto-scroll near the edges: 24px zone, 12px horizontal / 8px vertical steps (`:491-501`).
- [ ] Cursors: crosshair (draw), cell (eraser), default; pointer or grab over a note; ew-resize on edges; grabbing while moving (`:503-511`).
- [ ] Commit labels: Move Note(s), Duplicate Notes, Resize Note(s), Draw Note(s), Delete Note(s) (`:609-655`). Resizing the end edge, or drawing with a drag, updates the default draw length.
- [ ] Hover reports the row and the snapped position back to the editor (`:518-528`). Pointer leave clears them (`:674-680`). The context menu is suppressed (`:681`).

### PianoRollRuler (`components/piano-roll-editor/PianoRollRuler.tsx`)
- [ ] 30px tall; the top 10px is the loop strip (`:6-7`).
- [ ] Tooltip: "Click/drag: set playhead · Drag the top strip (or Shift+drag): set loop · Click the strip: toggle loop" (`:201`).
- [ ] Clicking or dragging below the strip sets the position (snapped; Alt or Mod = free) (`:161-172`).
- [ ] On the strip, or with Shift: dragging more than 3px draws a loop; a click without moving toggles the loop (`:156-194`).
- [ ] Draws: area past the region end, ticks, labels ≥ 40px apart, region end line, cue and playhead triangles (`:53-130`).

### VelocityLane (`components/piano-roll-editor/VelocityLane.tsx`)
- [ ] 92px tall; padding 10 top, 6 bottom; guides at 25/50/75/100% (`:15-17, 67-68`).
- [ ] Tooltip "Drag a stem to set velocity · Drag empty space (or Alt+drag) to draw a ramp" (`:227`).
- [ ] Stems within 6px of the pointer are hit, selected ones first (`:154-156`).
- [ ] Dragging a stem that belongs to a multi-selection offsets all selected notes, readout "±n". Otherwise it sets the stem's velocity, readout 0–127 (`:158-170, 189-198`).
- [ ] Empty space, or Alt: a ramp across the notes in range (only selected ones if there is a selection); dashed line; readout "a → b" (`:172-206`).
- [ ] Commits "Edit Velocity" (`:209-220`).

### PianoKeyColumn and DrumKeyColumn
- [ ] Piano keys: 76px wide, black keys 60% width. C keys are labelled, plus the hovered key. Selected pitches light up. Click or drag across keys to audition ("Click or drag across the keys to audition") (`components/piano-roll-editor/PianoKeyColumn.tsx:6-157`).
- [ ] Drum column: one button per row with a colour dot and a label (when row height ≥ 12), tooltip "{label} (MIDI n)". Pointer down auditions (`components/piano-roll-editor/DrumKeyColumn.tsx:37-69`).

---

## 7. On-screen keyboard (`components/PianoRoll.tsx`, `components/PianoKey.tsx`; used in InstrumentDock:212)
- [ ] Range C2–C6 (MIDI 36–84). White keys 40px wide, or 32px compact; 1px gap; black key offset 15 or 12 (`components/PianoRoll.tsx:52-55`; `utils/constants.ts:14-15`).
- [ ] Every pointer holds at most one key. Velocity 0.7. Sliding onto another key retriggers (glissando). A key held by two pointers stays down until both release (`components/PianoRoll.tsx:57-104`).
- [ ] Keys are released on pointer up or cancel anywhere, when the pointer leaves the keyboard, on window blur, when the tab is hidden, and on unmount (`components/PianoRoll.tsx:106-129`). Right-click plays nothing; the context menu is suppressed.
- [ ] Non-compact header: "Keyboard" and "held {n}" (`components/PianoRoll.tsx:140-150`). C note names above the keys.
- [ ] Key sizes: white 160px tall (100 compact), black 100px (60 compact). Held state uses the accent colour. Note label on white keys when not compact (`components/PianoKey.tsx`).

---

## 8. Mixer (`components/mixer/`, `hooks/useMixer.ts`)
- [ ] Shown only in the Song view; X toggles it. Header "Mixer" and "Press X to hide" (`components/mixer/MixerView.tsx:25-28`).
- [ ] Levels are polled every animation frame from `arrangementEngine.getTrackLevel(id)` and `getMasterLevel()` (`hooks/useMixer.ts:20-31`).
- [ ] With no tracks: "Add tracks to see mixer channels" (`components/mixer/MixerView.tsx:41-45`).
- [ ] Channel strip, 92px wide (`components/mixer/ChannelStrip.tsx`):
  - Track name.
  - Pan knob −1..1, default 0, label C/Ln/Rn.
  - Fader −40..0, double-click resets to 0 dB, height 104.
  - Meter plus a scale (0, −6, −12, −24, −48).
  - "{vol} dB" readout.
  - Mute and solo, size md.
  - Muted strip at 70% opacity.
- [ ] Fader (`components/mixer/Fader.tsx`):
  - Clicking off the cap jumps to that position; grabbing the cap keeps its offset.
  - Drag; Shift is 4× finer.
  - Arrow keys ±1, or ±6 with Shift.
  - Double-click resets to the default.
  - Values rounded to integers. 0 dB tick.
- [ ] Meter: floor −60 dB; green below −12, amber to −3, red above (`components/mixer/LevelMeter.tsx`).
- [ ] Master strip: "Master" label; "CLIP" lamp lit when level > −0.5 dB; two meters; readout "{n} dB" or "-∞ dB" (`components/mixer/MasterStrip.tsx`).

---

## 9. Live FX / instrument rack
### EffectsPanel (`components/EffectsPanel.tsx`), Song view only
- [ ] Title "Live FX" and a filter type select (LP/HP/BP/Notch/AP), calling `audioEngine.setFilterType` (`:99-110`).
- [ ] Always-visible knobs:
  - Cutoff 60..18000 (default 18000)
  - Reso 0..20 (default 1)
  - Reverb 0..1 (default 0.12)
  - Chorus 0..1 (default 0)
- [ ] Section tabs (one expanded at a time; the dot lights when the section is active) (`:87-93, 214-245`):
  - LFO: Rate 0.1..20, Depth, wave select. Lit when depth > 0.
  - Delay: Time, Feedback, Mix. Lit when wet > 0.
  - Drive: Drive, Mix. Lit when wet > 0.
  - EQ: Low/Mid/High ±12. Lit when any band ≠ 0.
  - Comp: Thresh, Ratio, Attack, Release. Lit when ratio > 1.
- [ ] Each knob calls the matching `audioEngine.set*` setter, then `onEffectChange(audioEngine.getEffectParams())` (`:49-71`). Defaults are DEFAULT_EFFECT_PARAMS (`types/effects.ts:32-53`).

### SynthEditor (`components/SynthEditor.tsx`), toggled from the TopBar's InstrumentSelector button
- [ ] Header: "Sound"; engine badge Analog, FM, String or Sampled, plus "· Mono" for mono subtractive presets (`:217-222, 277-281`).
- [ ] Category chips jump to the first preset in that category (`:283-301`).
- [ ] Reset button: "Reset edits" when there are edits, otherwise "Reset" (disabled). It clears the overrides and calls `onPresetChange(idx)` (`:266-269, 302-309`).
- [ ] Buttons for the presets in the current category, plus the preset description (`:312-329`).
- [ ] Sections per engine: subtractive `:84-140`, fm `:142-183`, pluck `:185-197`, sampler `:199-208`. Each change calls `onSynthParam(dottedPath, value)` (that is, `audio.setSynthParam`) and updates the local override mirror (`:258-264`).
- [ ] Log knobs work in log2 space and round to whole Hz (`:374-391`). Choice controls are vertical button stacks (`:409-436`).
- [ ] Overrides are seeded from `audioEngine.getSynthOverrides()` when the preset matches, and reset when the preset changes (`:237-245`).

### InstrumentSelector (`components/InstrumentSelector.tsx`; TopBar:232)
- [ ] Trigger shows the category badge (≥ 1360px wide) and the preset name (`:65-80`).
- [ ] Popover: search "Search sounds…", vibe chips ("All" plus the vibes in use), groups by category (Keys, Bass, Lead, Pad, Pluck, Brass, Bells & Mallets, Percussion, FX), filtered by vibe. Each row shows a selected dot and the vibe label. Search matches name, category, vibe and description (`:82-140`).
- [ ] Synth editor toggle: "Edit sound" / "Hide synth editor" (`:142-156`).

### RoutingModeSelector (`components/RoutingModeSelector.tsx`; InstrumentDock:162)
- [ ] Segmented control: Auto ("low keys trigger drums, the rest play the synth"), Keys ("every key plays the synth"), Split ("left half drums, right half synth"). Deselecting is ignored (`:9-23`).

### MidiDeviceSelector (`components/MidiDeviceSelector.tsx`; TopBar:223)
- [ ] Placeholder "No MIDI input"; status dot (green when connected); tooltip "MIDI input" / "No MIDI input connected"; disabled item "No devices found" when the list is empty. Selecting calls `midiService.selectDevice` (`:18-55`; `hooks/useMidi.ts:30-33`).

### VolumeControl (`components/VolumeControl.tsx`; TopBar:240)
- [ ] Mute button ("Mute output" / "Unmute output"); the icon shows VolumeX when muted or at ≤ −40 dB (`:16-32`).
- [ ] Slider −40..0 step 1 ("Output volume"); "{n} dB" readout at ≥ 1500px wide (`:33-45`).
- [ ] Mute calls `audioEngine.mute()/unmute()` and `drumEngine.mute()/unmute()` (`hooks/useAudioInput.ts:8-20`).

---

## 10. Export dialog (`components/ExportDialog.tsx`)
- [ ] Modal 360px wide. Closes on a backdrop click or Escape, except while exporting (`:16-25, 63-65`).
- [ ] Title "Export". Subtitle "Nothing to export yet. Add a track first." or "{audible} of {n} track(s) will be included (muted tracks are skipped)." (`:78-83`).
- [ ] "WAV audio" / "16-bit PCM stereo, rendered offline" runs `exportWav(arrangement)` and downloads `{name||'untitled'}.wav` (`:27-40, 87`).
- [ ] "MIDI file" / "Standard MIDI; drums on channel 10" runs `exportMidi` and downloads `.mid` (`:42-55, 88`).
- [ ] While exporting: spinner and "Exporting…"; both buttons disabled. Errors show in a red alert (`:94-113`).

## 11. Template picker (`components/TemplatePicker.tsx`)
- [ ] Modal up to 720px wide. Escape or a backdrop click closes it (`:38-56`).
- [ ] Title "New from template". Subtitle "Start with tracks, sounds and a groove already set up." Close X (`:65-76`).
- [ ] Tabs: All, Starter, Rock, Hip-Hop, Jazz, EDM, Ambient (`:13-31, 79-95`).
- [ ] Two-column cards: name, category badge (coloured per category), description, up to 6 track colour bars, "{bpm} BPM · {trackCount} tracks" (`:98-138`).

### Templates (`data/templates/*.json`, order from `index.ts`; preset numbers are SYNTH_PRESETS indices)
All templates are 64 beats long, and no track is muted or soloed.

| id | Name / category / BPM | Description | Tracks (preset, vol, pan) and regions | Markers |
|---|---|---|---|---|
| blank-4track | Blank 4-Track / starter / 120 | "Four empty tracks ready to go: synth, bass, pad, and drums." | Classic (0 Grand Piano, −6); Bass (7 Analog Bass, −6); Pad (6 Supersaw Pad, −8); Drums (drums, −4). No regions. | – |
| rock-song | Rock Song / rock / 120 | "Classic rock setup: drums with a driving beat, bass, rhythm guitar, and lead." | Drums −4: "Rock Beat" 0–16, 24 notes. Bass (7) −6: "Bass Riff" 0–16, 10 notes. Lead (10 Square Lead) −8, pan 0.2. Pad (6) −12, pan −0.2. | Intro@0, Verse@16, Chorus@32, Outro@48 |
| hip-hop-beat | Hip-Hop Beat / hip-hop / 90 | "90 BPM trap-style pattern with booming drums, sub bass, and a synth pad." | Drums −3: "Trap Beat" 0–16, 20 notes. Sub Bass (8 Sub Bass) −4: "808 Line" 0–16, 5 notes. Pad (6) −14. | – |
| jazz-trio | Jazz Trio / jazz / 130 | "130 BPM jazz trio: swing drums, walking bass, and electric piano." | Drums −6: "Swing Ride" 0–16, 20 notes. Walking Bass (7) −6: "Walk" 0–16, 16 notes. Electric Piano (13 Rhodes EP) −8, pan 0.15. | – |
| edm-drop | EDM Drop / edm / 128 | "128 BPM four-on-the-floor beat with bass, lead, and pad tracks." | Drums −3: "4-on-floor" 0–16, 37 notes. Bass (7) −5: "Bass Drop" 0–16, 8 notes. Lead (10) −8. Pad (6) −14. | Build@0, Drop@16, Breakdown@32, Drop 2@48 |
| ambient-pad | Ambient Pad / ambient / 70 | "70 BPM atmospheric setup with evolving pad, warm synth, and sparse drums." | Pad (6) −8, pan −0.3: "Ambient Wash" 0–32, 6 notes. Warm (3 Warm Pad) −12, pan 0.3: "Melody" 8–24, 4 notes. Drums −10: "Sparse" 16–32, 5 notes. | – |

---

## 12. Settings (`components/settings/SettingsView.tsx`, `hooks/useSettings.ts`)
- [ ] Stored under localStorage `micro-daw-settings`. Loading keeps only known keys and fills missing ones from DEFAULT_SETTINGS (`utils/constants.ts:102-109`). Updates merge per section and save immediately (`hooks/useSettings.ts:5-40`).

| Field | Default | UI | Used by |
|---|---|---|---|
| general.autoCheckUpdates | true | Switch "Check for updates automatically" / "Looks for a new version when the app starts." | Calls `updaterCheck` once at mount (`hooks/useAutoUpdater.ts:24-29`) |
| audio.defaultMasterVolume | −12 | Slider −40..0 "Default master volume" / "Used for new projects." with "{n} dB" readout | **Nothing reads it** |
| audio.bufferSizeHint | 256 | Select 128/256/512/1024/2048 "{n} samples"; "Buffer size" / "Smaller is more responsive; larger is safer on slow machines." | **Nothing reads it** |
| midi.autoConnectLastDevice | true | Switch "Auto-connect last device" / "Reconnect to the keyboard you used last time." | **Nothing reads it** |

- [ ] Header "Settings" / "Preferences are saved on this computer." (`:134`).
- [ ] "Software update" row: "Installed: {version||'dev'}" (`:152-178`).
  - Buttons: "Download & install" (when available), "Restart to update" (when downloaded), "Check now" (disabled while checking or downloading).
  - Status lines: "Checking for updates…", "Version X is available", "You're up to date", "Downloading N%" with a bar, "Version X is ready to install", or the error message (`:39-88`).
  - Calls `electronAPI.updaterCheck`, `updaterDownload`, `updaterInstall`, `onUpdaterStatus`, `getAppVersion` (`hooks/useAutoUpdater.ts`).
- [ ] About section: Version; Author "Yusuf Emirhan Sahin"; Source code link "GitHub" to https://github.com/yemirhan/micro-daw; Credits "Piano samples: Salamander Grand Piano V3 by Alexander Holm, licensed under CC BY 3.0." (`:239-272`).

---

## 13. Samples view (`components/samples/`, `hooks/useSampleLibrary.ts`)
- [ ] Header "Samples" / "Import, trim, process or generate audio for your arrangement" and "+ Import audio" (`components/samples/SamplesView.tsx:23-31`).
- [ ] Library sidebar, 288px: "Library" and a count. Empty state "No samples yet" / "Import WAV, MP3 or other audio files, or create a tone in the Generate tab." plus an import button (`:35-62`).
- [ ] List row: 72×28 waveform thumbnail (`sampleManager.computePeaks`), name, duration (ms / x.xs / m:ss), mono or stereo. Selecting stops any audition (`components/samples/SampleListItem.tsx`; `hooks/useSampleLibrary.ts:134-137`).
- [ ] Tabs: **Edit** (info, needs a sample), **Process** (needs a sample), **Generate** (always available). With no sample selected only Generate is enabled (`components/samples/SampleDetail.tsx:27-31, 48-50`).

### Edit tab, labelled "Edit" (`components/samples/InfoTab.tsx`)
- [ ] Rename: the pencil icon opens an inline input. Enter or blur commits (trimmed, and only if changed); Escape cancels (`:66-102`).
- [ ] "Audition" / "Stop" plays the trimmed range with a `Tone.Player` and tracks progress per frame (`:104-107`; `hooks/useSampleLibrary.ts:150-193`).
- [ ] "Send to arrangement": `addTrack('audio')` named after the sample, then `addAudioRegion(track, sample, 0)`, applying the trim offset and length (beats = seconds/60 × bpm). Then switches to DAW mode (`:108-111`; `hooks/useSampleLibrary.ts:236-251`; `components/samples/SamplesView.tsx:15-18`).
- [ ] Waveform: labels "0.00s", "Drag the orange handles to trim", "{dur}s" (`:115-130`).
- [ ] "Start (s)" / "End (s)" number inputs with step 0.01. The trim is clamped so start ≤ end − 0.01. "Reset trim" is disabled when untrimmed (`:133-167`; `hooks/useSampleLibrary.ts:195-234`).
- [ ] Readouts: Length (trimmed), Rate (kHz), Channels. The file path is shown in mono (`:169-183`).
- [ ] "Remove from library" (`:186-194`).

### WaveformDetail (`components/samples/WaveformDetail.tsx`)
- [ ] 192px tall canvas: centre line and quarter grid. The full waveform is dimmed; the trimmed range is drawn in the accent colour. Outside the trim is shaded. Handles are 2px lines with 8×12 tabs. The playback cursor shows while auditioning, redrawn per frame (`:37-157`).
- [ ] Dragging within 10px of a handle moves it. Cursor is col-resize near a handle (`:159-215`).

### Process tab (`components/samples/ProcessTab.tsx`)
- [ ] Intro: "Switch on the stages you want; they run top to bottom and create a new sample, leaving the original untouched." (`:40-42`).
- [ ] Each stage has a switch; its parameters show only when on (`components/samples/ParamControls.tsx:50-81`).
  - Filter ("Cut lows or highs"): type Lowpass/Highpass/Bandpass; Cutoff 20..20000 (default 1000); Resonance 0.1..20 (default 1).
  - Pitch shift ("Up or down in semitones"): −24..24 (default 0).
  - Reverse ("Play the sample backwards").
  - Normalize ("Raise the peak to full scale").
  - Reverb ("Add room and tail"): Decay 0.1..10 (default 2.5); Wet (default 0.3).
  - Delay ("Echoes"): Time 0.01..1 (default 0.25); Feedback 0..0.95 (default 0.3); Wet (default 0.3).
  - Defaults are DEFAULT_PROCESSING (`types/samples.ts:39-46`).
- [ ] "Apply to new sample" / "Processing…". Disabled when no stage is on, with the hint "Switch on at least one stage". It runs `processAudio`, then `sampleManager.registerBuffer('{name} (processed)')` and selects the new sample (`:175-181`; `hooks/useSampleLibrary.ts:253-275`).

### Generate tab (`components/samples/GenerateTab.tsx`)
- [ ] Waveform chips: Sine, Square, Saw, Triangle, Noise. Freq 20..20000 (disabled for noise). Duration 0.1..30s. Level 0..1. Defaults: sine, 440 Hz, 1s, 0.8 (`types/samples.ts:55-60`).
- [ ] "Hold to preview" plays while the pointer is down and stops on up or leave. Playing a MIDI key previews it and sets the frequency, showing the detected note name (`useSignalPreview`) (`:29-40, 104-113`).
- [ ] "Generate" / "Generating…": names the sample `{type} {freq}Hz {dur}s` or `white-noise {dur}s` (`hooks/useSampleLibrary.ts:277-295`).
- [ ] Hint: "MIDI keys preview at their pitch".

### Persistence
- [ ] localStorage `micro-daw-sample-library` stores `[ {sampleId, samplePath, sampleName, trim, addedAt} ]`. Generated or processed samples (empty path) are skipped. At mount each entry is reloaded with `sampleManager.loadFromPathWithId` (`hooks/useSampleLibrary.ts:28-70`).
- [ ] Import uses `electronAPI.sampleShowImportDialog()` and skips paths already in the library (`hooks/useSampleLibrary.ts:94-121`).

---

## 14. Sample browser (`components/browser/`, `hooks/useSampleBrowser.ts`)
- [ ] Panel width persists under `micro-daw:browser-width`: default 248, range 200–520. The resize handle drags; when focused, ←/→ step 16 (48 with Shift); double-click resets (`components/browser/BrowserPanel.tsx:5-101`).
- [ ] Other persisted keys: `micro-daw:browser-roots` (JSON array), `micro-daw:browser-last-path` (restored only if it is still inside a root), `micro-daw:browser-auto-preview` (`'0'` or `'1'`, default on) (`hooks/useSampleBrowser.ts:17-19, 120-131`).
- [ ] Pinned root "Installed packs" (`<userData>/packs`) when any packs are installed. It is listed first and cannot be removed (`hooks/useSampleBrowser.ts:54-58, 122-123`).
- [ ] Outside Electron: "Browser unavailable" / "Browsing sample folders needs the desktop app." (`components/browser/SampleBrowser.tsx:147-156`).
- [ ] Header "Browser" with buttons: auto-preview toggle (Volume2/VolumeX, "Auto-preview on/off"), "Refresh" (not at the root list), "Add folder" (`fsPickFolder`) (`:162-186`).
- [ ] No roots: "Add a sample folder" / "Point the browser at folders with your drums and one-shots. Click to preview, drag onto a pad or press Enter to assign." with "Choose folder…" and "+ ~/Music", "~/Downloads", "~/Splice" suggestions when those exist (`:188-215`; `hooks/useSampleBrowser.ts:66, 184-204`).
- [ ] Breadcrumbs: a HardDrive icon ("Folders" at the root list, tooltip "All folders"), then root, sub-folders, current folder (`components/browser/BrowserBreadcrumbs.tsx`).
- [ ] Filter input: placeholder "Filter folders" or "Filter this folder". Every space-separated term must match the name, case-insensitive. A clear-X button empties it (`components/browser/SampleBrowser.tsx:220-245`; `components/browser/browserHelpers.ts:25-32`).
- [ ] List rows: icon (root, folder, file, or speaker while previewing), name without extension, extension, remove-root X on hover ("Remove folder from browser"), chevron on folders. Files are draggable with MIME `application/x-micro-daw-sample` carrying `{path,name}` plus `text/plain` (`components/browser/BrowserRow.tsx`).
- [ ] Mouse: clicking a folder opens it; clicking a file selects it and previews it. Double-clicking a file assigns it (`components/browser/SampleBrowser.tsx:65-79`).
- [ ] Keys:
  - In the list and the filter: ↓/↑ move by 1; PgDn/PgUp move by 10 (moves preview when auto-preview is on). Enter opens a folder or assigns a file. Escape clears the filter, or stops the preview.
  - List only: Home and End. → opens the selected folder. ← or Backspace goes up. Space previews. Any printable key starts type-to-filter (`:82-131`).
  - Plain keys stay inside the browser; Mod chords and Escape bubble up (`:20-23`).
- [ ] Preview uses `drumEngine.preview({kind:'file', path})`; stop uses `drumEngine.stopPreview()`, also called on unmount (`hooks/useSampleBrowser.ts:273-285, 316`).
- [ ] Listing uses `fsListDir`. Slow responses for a folder you've left are dropped. Errors are shown with the IPC prefix stripped; fallback "Could not read folder" (`hooks/useSampleBrowser.ts:137-165`).
- [ ] Going up re-selects the folder you just left (`:241-247`). While filtering, the first match is selected by default (`:268-271`).
- [ ] Status text: "Loading…"; `Nothing matches "{q}"` or "No folders or audio files here"; footer "{n} items" or "{filtered}/{total} items" and "↵ assign ␣ play" (when at least 16rem wide) (`components/browser/SampleBrowser.tsx:257-293`).

---

## 15. Developer view: MIDI monitor (`components/dev/DevView.tsx`, `hooks/useMidiMonitor.ts`)
- [ ] Header "MIDI Monitor" with a device chip (name, or "No device") (`components/dev/DevView.tsx:103-117`).
- [ ] Filter select: All messages, Note On, Note Off, Control Change, Pitch Bend, Aftertouch, Program Change (`:31-39, 120-131`).
- [ ] Pause/Resume ("Pause capture" / "Resume capture"); "Clear log" (`:133-153`).
- [ ] Columns: Time (s), Type (coloured dot), Ch, Detail, Raw hex (`:158-167`).
- [ ] Empty state: "Waiting for MIDI input…" / "No MIDI device connected", with the hint texts at `:179-182`.
- [ ] Auto-scrolls while within 40px of the bottom. Footer shows "{n} events (filtered)", "Capture paused", "Jump to latest" or "Following live" (`:78-98, 191-213`).
- [ ] Data: `midiService.onRawMessage`. Clock (0xF8) and active-sensing (0xFE) messages are dropped. Events are buffered and flushed every 50ms; at most 500 are kept. While paused, incoming messages are dropped (`hooks/useMidiMonitor.ts:6-7, 128-163`).
- [ ] Decoding (`hooks/useMidiMonitor.ts:28-113`):
  - Note On (velocity 0 counts as Note Off) and Note Off: "C4 (60) vel=n".
  - CC: names for CC 1, 2, 7, 10, 11, 64, 65, 66, 67, 71, 74, 91, 93, otherwise "CC#n = v".
  - Pitch Bend: "value=±n" (14-bit − 8192).
  - Ch Pressure and Key Pressure.
  - Program: "program=n".
  - System messages: Start, Stop, Continue, SysEx ("n bytes"), otherwise "status=0x..".

---

## 16. Hooks and services the UI depends on
- [ ] **useArrangement** (`hooks/useArrangement.ts`):
  - Slice stores: transport, tracks, beat, position (`:118-204`).
  - The actions table is a thin wrapper over `arrangementEngine` (`:236-330`).
  - First load runs `loadFromLocalStorage()`, falling back to `resetToStarter()`. Autosave is debounced 300ms and flushed on beforeunload, pagehide and unmount (`:339-377`). The storage key is `micro-daw-arrangement` (`services/ArrangementEngine.ts:1529, 1537`).
- [ ] **useProject** (`hooks/useProject.ts`): mirrors `projectManager` state (filePath, isDirty, name, recent). Any engine change marks the project dirty, except the first change after open, new, recent or template. Window close calls `projectManager.handleBeforeClose()`. Actions: save, saveAs, open, newProject, openRecent, newProjectFromTemplate.
- [ ] **useUndoRedo** (`hooks/useUndoRedo.ts`): reads `undoManager` (canUndo/canRedo and labels); undo and redo go through `arrangementEngine.performUndo()` and `performRedo()`.
- [ ] **useMidi** (`hooks/useMidi.ts`): `midiService.enable`, `setCallbacks`, `onDeviceChange`, `selectDevice`.
- [ ] **useAutoUpdater**, **useAudioInput**, **useAppMode**, **useSettings**, **useSampleLibrary**, **useSampleBrowser**, **useMidiMonitor**, **useMixer**: covered in the sections above.