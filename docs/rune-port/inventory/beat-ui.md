I've written the Beat view parity checklist below, grouped by component. Every item cites `file:line`.

**Paths:** all citations are relative to `src/`.

**Scope notes**
- In the Beat view the `TopBar` centre slot shows `LiveInputReadout`. `TransportBar` only renders in Song view (`app/App.tsx:528-547`); it is still covered below.
- `DrumPads.tsx` is legacy. `App` does not render it; only its test does. `App` only uses `padFlash` from `useDrumPads` (`app/App.tsx:32,140`).
- The Beat view's sub-components (`DrumTrackMenu`, `KitMenu`, `MissingPacks`, `PendingLoadChip`, `PadThumb`, `stepKeys`, `chop`, `keyScope`, `missingPackActions`, `usePackAvailability`, `usePadBuffer`, `waveform`) are included because the listed components depend on them.

---

## 0. App-level wiring and shortcuts that reach the Beat view

- [ ] BeatView props from App: `onPadHit=drumPress`, `onPadRelease=drumRelease`, `onPadPreview=drumHit`, controller setup, export dialog, `onBrowsePacks=openPacks` (`app/App.tsx:576-583`)
- [ ] In Beat view the InstrumentDock gets `padsAvailable={false}` and `defaultCollapsed`; the SynthEditor sits above it when open (`app/App.tsx:588-610`)
- [ ] Browser "assign" (Enter or double-click in the browser) calls `beatActions.assignFile(engine.getSelectedPad(), {path,name})` (`app/App.tsx:151-154`)
- [ ] **Space** in Beat view, with no modifiers and no auto-repeat: `beatActions.togglePlay()` (`app/App.tsx:391-396`, `162-163`)
- [ ] **M** in Beat view: `setLoopSettings({metronome: !current})`. This is the loop recorder's click, not the global metronome (`app/App.tsx:157-160`, `397-401`)
- [ ] **Shift+Backspace** in Beat view: `beatActions.clearPattern()` (`app/App.tsx:402-406`)
- [ ] **N**: `noteRepeat.toggle()` in any DAW view (`app/App.tsx:384-389`)
- [ ] **Tab**: switches Beat/Song. It does not fire when focus is in a scoped-key target or inside any `[role="grid"]` (`app/App.tsx:369-376`)
- [ ] **B** toggles the browser; **Cmd/Ctrl+E** toggles the export dialog; **Cmd/Ctrl+Z** undoes and **Cmd/Ctrl+Shift+Z** redoes (`app/App.tsx:340-363`, `377-382`)
- [ ] App single-key shortcuts are skipped when `e.defaultPrevented` is set or a modal is open (`app/App.tsx:366`)
- [ ] Command palette entries: `loop-record` sets the view to beat and calls `toggleRecord`; `toggle-note-repeat`; `clear-pattern` (`app/App.tsx:245-250`)

---

## 1. BeatView (`components/beat/BeatView.tsx`)

### Persisted preferences (localStorage; an invalid or blocked value falls back to the default)
- [ ] `micro-daw:beat-pad-layout`: `'4x4' | '2x8'`, default `'4x4'` (`:49,119`)
- [ ] `micro-daw:beat-step-grid`: `'1/8' | '1/16' | '1/32'`, default `'1/16'` (`:50,120`)
- [ ] `micro-daw:beat-key-hints`: `'on' | 'off'`, default `'off'` (`:51,121,132-137`)
- [ ] Writes ignore storage errors (`:61-67`)

### Computer-keyboard pads (window listeners, capture phase) (`:144-193`)
- [ ] Ignored when the target is a scoped-key target or a modal is open. "Modal open" means a `[role=dialog|alertdialog][data-state=open]` exists (`:70-72,148`)
- [ ] **R**, with or without Shift, but not with Meta, Ctrl or Alt: calls preventDefault and stopImmediatePropagation, then `toggleRecord()` unless the key is auto-repeating (`:150-155`)
- [ ] Shift plus any other key: ignored, so Shift chords pass through to the app (`:156`)
- [ ] Pad keys are mapped by `padForKeyEvent` (§10). The handler calls preventDefault and stopImmediatePropagation, skips auto-repeat and keys already held, then calls `onPadHit(pad, 0.85)` (`:157-164`)
- [ ] Keyup on a held code calls `onPadRelease(pad)` (`:166-171`)
- [ ] Window blur, document becoming hidden, or unmount releases every held pad (`:172-192`)

### Derived state
- [ ] `playing = transportState !== 'stopped'` and `counting = countInRemaining > 0` (`:197-198`)
- [ ] `loopHint = describeLoopMismatch({loop: loopStart/End or null, loopEnabled, playing, patternStartBeat, bars: loop.bars})` (`:199-205`)
- [ ] Missing packs come from `missingPacks(pads, packs)`. Titles are memoised by their joined string. The catalog is loaded while anything is missing (`:96-105`)
- [ ] `selectedMissing` is the missing pack that contains the selected pad (`:226`)

### Layout
- [ ] Transport strip on top, then an optional browser aside (260px, `data-keys-scope="browser"`), then the pads and editor row (height `min(46vh,372px)`), then the step sequencer filling the rest (`:229-412`)
- [ ] Pads section width is 392px in 4x4 and 600px in 2x8. `data-show-keys` is set when key hints are on (`:255-259`)

### Pads header (`:261-335`)
- [ ] DrumTrackMenu (§8a) → "/" separator → KitMenu (§8b) → PendingLoadChip (§8d) → layout radiogroup → right cluster (`:262-334`)
- [ ] Layout radiogroup (aria "Pad layout") with options **4×4** and **2×8**. Clicking writes the preference (`:283-307`)
- [ ] In 4x4, while a pending-load chip exists, the layout toggle and the key-hints button are hidden (`:289,316`)
- [ ] Key-hints button (Keyboard icon, aria "Computer-key hints", `aria-pressed`). Tooltip: "Hide computer-key hints" or "Show computer-key hints" (`:309-320`)
- [ ] Controller setup button, only if `onOpenControllerSetup` is passed. Tooltip "Controller setup (<profile.name>): map pads and knobs". In 2x8 it also shows a text label: the profile name, or "Controller" if the name is longer than 18 characters (`:321-333`)
- [ ] KitMenu factory/load choices call `packService.cancelPendingLoad()` before `actions.loadFactoryKit()` / `actions.loadKit(kit)` (`:218-225`)

### Below the header
- [ ] MissingPacksStrip; `onFix` is passed only when `trackId` exists (`:337`)
- [ ] PadGrid gets `showKeyHints` always true; visibility is controlled by the CSS `data-show-keys` attribute. It gets `canPaste = hasClipboard` and `hardwareNotes = profile.padMap.notes`. Grid height: `flex-1` in 4x4, fixed 220px in 2x8 (`:339-359`)
- [ ] Callbacks: `onHit=onPadHit`, `onRelease`, `onSelect=selectPad`, `onDropSample=assignFile`, `onClear=clearPad`, `onRename=renamePad`. `onColor` and `onChokeGroup` call `actions.updatePad` (not grouped). `onCopy`/`onPaste`. `onChop` selects the pad and bumps `chopRequest` (`:207-213,348-358`)
- [ ] **Count-in overlay** while counting: a large number `Math.ceil(countInRemaining)` (132px, signal-rec colour, pop animation on each change) over the pads, with pointer events disabled (`:362-368`)

### PadEditor host
- [ ] Rendered only if the selected pad's settings exist. `onChange` calls `actions.editPad(selected, patch)` (grouped undo). `onChangeEnd` calls `endPadEdit`. `onPreview` calls `(onPadPreview ?? onPadHit)(pad, 0.85)`. `onChop` calls `chopToPads`. `onFixMissing` is passed only with a `trackId` (`:214-215,372-386`)

### Step sequencer host
- [ ] While loop recording and not counting, the sequencer gets a red inset top shadow (`:392`)
- [ ] `PatternSteps` subscribes to `usePatternRegion()`. `onClear` is passed only if the region has notes; otherwise the Clear button is disabled (`:422-430`)
- [ ] Wired to `toggleStep`, `setStepVelocity`, `nudgeStepVelocity`, `beginStepGesture`/`endStepGesture`, `selectPad`, `clearPattern`, `loopPattern`, `subscribePosition` (`:391-410`)

---

## 2. BeatTransport (`components/beat/BeatTransport.tsx`)

### State inputs
- [ ] BeatView passes `recording = isLoopRecording && !counting` (`BeatView.tsx:234`). Inside: `counting = countInRemaining > 0` and `armed = recording || counting` (`:336-337`)
- [ ] One row with four sections separated by 1px rules: Transport (no caption) | Loop | Record | Note repeat (no caption), then Export pushed right (`:339-473`)
- [ ] Section captions (uppercase) show only when the container is at least 1160px wide. Group labels ("Bars", "Quant", "Count-in") show only at 1280px or wider. Narrower than that, the row wraps (`:89-118`)

### Transport section
- [ ] **Play/Stop button.** Label and tooltip read "Stop" when `playing || armed`, otherwise "Play". Shortcut shown: Space. Icon: square when stopping, play triangle otherwise. `aria-pressed = playing`; green styling while playing. Calls `onTogglePlay` → `actions.togglePlay` (`:343-354`)
- [ ] **Record button.** Label and tooltip read "Stop recording" when armed, otherwise "Record loop". Shortcut shown: R. Pulsing solid red when armed, red icon otherwise. `aria-pressed = armed`. Calls `onToggleRecord` (`:355-370`)
- [ ] **Status LCD** (fixed 92px, `aria-live="polite"`, red border when armed) (`:373-388`):
  - [ ] While counting: "COUNT-IN" followed by `Math.ceil(countInRemaining)` (`:377-381`)
  - [ ] Otherwise: LoopPosition plus a pulsing red dot (aria "Recording") when `recording` (`:384-385`)
- [ ] **LoopPosition** text is "bar.beat" relative to the pattern start, wrapped modulo `bars*4`: `${floor(rel/4)+1}.${floor(rel%4)+1}`. It resets to "1.1" when not playing and is followed by "/ {bars}". It is updated imperatively from `subscribePosition` and only when the text changes (`:254-275`)
- [ ] **BpmField** (shows "BPM", 15px value) (`:171-251`):
  - [ ] Typing keeps only `[0-9.]`, at most 5 characters. Commit happens on blur or Enter: `parseFloat`, then clamp and round to `[40, 240]` (`MIN_BPM`/`MAX_BPM`, `utils/constants.ts:61-62`). An invalid value is discarded (`:194-200,209`)
  - [ ] Escape cancels the draft and blurs. ArrowUp/ArrowDown change the value by ±1, or ±10 with Shift (`:212-223`)
  - [ ] Focus selects all text (`:210`)
  - [ ] Wheel (non-passive): ±1 per event, up = +1 (`:183-192`)
  - [ ] Vertical drag when the field is not focused: moves count after more than 2px, value = `start + dy/2`, or `dy/8` with Shift (dy positive = up), clamped. A click without movement focuses the field for typing (`:225-245`)
  - [ ] Calls `onBpmChange` → `engine.setBpm`

### Loop section
- [ ] **Bars** segmented control (aria and title "Loop length in bars"): options 1, 2, 4, 8 → `onLoopChange({bars})` (`:13,396-398`)
- [ ] **Quant** select (aria "Quantize", title "Quantize recorded hits", 64px wide): options Off, 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32 → `onLoopChange({quantize})` (`:12,399-412`)
- [ ] **Swing control** (aria group "Swing") (`:132-168`):
  - [ ] Knob, size sm, range 0..0.75, step 0.01, default 0 (double-click resets). Its own value text is hidden; the readout is `${round(50 + swing*50)}%`, so 50% to 88% (`:133,140-152`)
  - [ ] Tooltip: "Swing: delays every other grid step (50% = straight). Double-click resets."
  - [ ] Grid button shows "8ths" for `'1/8'` and "16ths" for `'1/16'` (`swingGridLabel`, `:125`). Clicking toggles between them → `onLoopChange({swingGrid})`. Default when undefined: `'1/16'` (`:415`). aria "Swing grid: <label>". Tooltip "Swing grid: {off-beat 8th notes | every other 16th note} swing. Click for <next>. Independent of quantize." (`:155-165`)

### Record section
- [ ] **Count-in** segmented control (aria "Count-in bars"): options Off, 1, 2 → `{countInBars}` (`:14,425-427`)
- [ ] **Record mode** segmented control (aria "Record mode"): Overdub / Replace, non-monospace text → `{overdub: v==='overdub'}` (`:428-435`)
- [ ] **Metronome toggle** (aria "Metronome", `aria-pressed`). Tooltip "Metronome on while recording" or "Metronome off", shortcut M → `{metronome: !loop.metronome}` (`:436-451`)
- [ ] Engine defaults: bars 2, quantize 1/16, swing 0, swingGrid 1/16, countIn 1, metronome on, overdub on (`utils/arrangementHelpers.ts:42-50`)

### Note repeat section (shown when the `noteRepeat` prop is passed; BeatView always passes it)
- [ ] **Repeat** toggle (aria "Note repeat", `aria-pressed`). Border and fill use the primary colour when on. Tooltip when on: "Note repeat on: held pads retrigger every {rate} while playing"; when off: "Note repeat: hold a pad to retrigger it on the grid while playing". Shortcut N → `noteRepeat.toggle()` (`:278-297`)
- [ ] **Rate** select (aria "Note repeat rate"): 1/4, 1/8, 1/8T, 1/16, 1/16T, 1/32; default 1/16 → `noteRepeat.setRate` (`:298-309`; `services/NoteRepeat.ts:23-24`)

### Export
- [ ] Download icon button (aria "Export"), tooltip "Export WAV or MIDI" with shortcut `⌘E` on Mac or `Ctrl+E` elsewhere → `onExport` (`:15,464-472`)

---

## 3. PadGrid and Pad (`components/beat/PadGrid.tsx`)

### Grid
- [ ] Rows come from `padRows(layout)`, or from `bankRows(bank)` when a bank is given: `[[o+4..o+7],[o..o+3]]` with `o` = 0 for A and 8 for B (`:112,163-169`)
- [ ] 2x8 layout adds an extra left margin before column 4 (the bank gap), but only when no bank is set (`:136`)
- [ ] Gaps: 1.5 normally, 1 in compact (`:120`)
- [ ] The context menu is only available if at least one menu action prop is defined (`:114,143`)

### Hit flash
- [ ] Subscribes to `noteEventBus` and flashes on `type==='drum'` events, whatever the source: MIDI, mouse or keyboard (`:81-87`)
- [ ] Flash: `v = clamp(vel>1 ? vel/127 : vel, 0.25, 1)`. The light layer's opacity goes from `0.35+0.6v` to 0 over 260ms. The pad scales 0.965 → 1 over 140ms (`:46-52`)

### Pad pointer and keyboard
- [ ] **Left pointer down** (primary button only): preventDefault, release any held press, mark held as pointer, capture the pointer, `onSelect(pad)`, then `onHit(pad, velocityFromPointer(clientY, rect.top, rect.height))` (`:257-266`)
- [ ] Pointer up, cancel or lost capture: release (calls `onRelease` once) (`:267-269`)
- [ ] **Enter or Space** keydown, no modifiers: always preventDefault and stopPropagation. If not a repeat and nothing is held: select, then `onHit(pad, 0.8)`. Keyup on the same key releases. Blur releases a key press (`:270-287`)
- [ ] Unmount mid-press releases (`:220-224`)
- [ ] **Right-click** (contextmenu): preventDefault, `onSelect(pad)`, then open the menu at the cursor's client X/Y (`:106-109,288-291`)

### Drag and drop
- [ ] Drag enter and drag over are only accepted if `onDropSample` exists and `dragHasSample(dt)`. On enter the pad is highlighted; on over `dropEffect='copy'` (`:292-301`)
- [ ] Drag leave clears the highlight unless the pointer moved into a child (`:302-305`)
- [ ] Drop: clear highlight, `readDroppedSample(dt, getPathForFile ?? electronAPI.getPathForFile)`. If a sample is found: `onDropSample(pad, sample)`, then `onSelect(pad)` (`:94-104,306-310`)
- [ ] While dragging over a pad, the name line reads "Drop to load" and the pad gets a 2px primary inset ring (`:251-252,351`)

### Pad visuals and labels
- [ ] aria-label: `Pad A1: <name>` or `Pad A1, empty`. aria-description is the missing-pack title (`:235-236`)
- [ ] Selected pad: surface-3 background plus a 2px inset ring in the pad's colour (`:246,253-254`)
- [ ] 3px colour strip at the top, opacity 0.35 when empty and 0.95 otherwise (`:313`)
- [ ] Top row: `padLabel` (bold, foreground when selected). In non-compact pads, the right side shows `hardwareNoteLabel(note)` with title "MIDI note N" or "No MIDI note mapped" (`:323-340`)
- [ ] Missing-pack warning dot next to the label: always shown on compact pads; on normal pads only when the pad is 56px tall or less. Its title is the missing-pack title (`:326-334`)
- [ ] Name line: name, or "Empty" in italic muted text. Max 1 line, 2 lines when the pad is at least 80px tall. Title is the full name (`:343-352`)
- [ ] Non-compact only, hidden when the pad is 56px tall or less: a waveform row (18px) with PadThumb, or a hairline when empty (`:354-361`)
- [ ] Key-hint cap bottom-right. Visible on hover, on keyboard focus, or when `[data-show-keys]` is on an ancestor. Never shown on compact pads (`:132,362-366`)

### PadThumb (`components/beat/PadThumb.tsx`)
- [ ] Status missing with a missing-pack title: PackageX icon and "no pack", tooltip = the title (`:55-62`)
- [ ] Status missing without one: TriangleAlert icon and "missing", tooltip "Sample file not found" (`:64-71`)
- [ ] Otherwise a canvas: the whole waveform at 16% white, the trimmed range redrawn in the pad colour. Columns = `clamp(round(w/2), 16, 160)`. Pulses while loading (`:26-53,73-79`)

---

## 4. PadContextMenu (`components/beat/PadContextMenu.tsx`)

- [ ] Fixed position, 208px wide, `role="menu"`, aria "Pad A1". Repositioned so it stays at least 4px inside the viewport (`:83-90,98-105`)
- [ ] Closes on a mousedown outside it or Escape (window listener) (`:67-80`). Every action closes the menu after it runs (`:92-95`)
- [ ] Right-click inside the menu is suppressed (`:104`)
- [ ] Header: pad name or "Empty pad", plus `padLabel` (`:106-109`)
- [ ] **Rename…**, disabled when there is no sample. It turns into an autofocused, select-all input (aria "Pad name") prefilled with `padName`. Enter submits `onRename(pad, draft)` → `renamePad`, which trims and ignores empty or unchanged names (`:111-135`; `hooks/useBeat.ts:290-296`)
- [ ] **Color**: grid of 16 `PAD_COLORS` swatches, 8 columns (aria "Color <c>"). The current colour gets a ring. "Reset color" appears only if a custom colour is set and calls `onColor(pad, undefined)` (`:137-161`)
- [ ] **Choke group** radiogroup: Off, 1, 2, 3, 4. The menu offers only 0–4; the editor offers 0–8 (`:26,163-186`)
- [ ] **Copy pad** → `copyPad`; **Paste pad**, disabled when `!canPaste` → `pastePad`; **Chop to pads…**, disabled when there is no sample → selects the pad and opens the editor's chop popover (`:189-199`)
- [ ] Separator, then **Clear pad** (destructive red, disabled when there is no sample) → `clearPad` (`:200-207`)

---

## 5. PadEditor (`components/beat/PadEditor.tsx`)

### Header (`:129-173`)
- [ ] Colour bar, `padLabel`, then the name or "Empty pad". The name's title is the file path, for file sources only (`:130-134`)
- [ ] Missing pack (only when no buffer is loaded): "Pack not installed" with a Get button, or "No longer available" with Clear/Replace. Otherwise, if the buffer status is missing: "File not found" (`:104,135-144`)
- [ ] When a buffer is loaded: trim readout `formatSeconds(start) – formatSeconds(end) / formatSeconds(duration)`, title "Trim start – end / sample length" (`:146-151`)
- [ ] **Audition** (Play icon, aria and tooltip "Audition pad"), disabled when there is no sample → `onPreview(pad)` at velocity 0.85 (`:152-158`)
- [ ] **Chop** button (Scissors icon, "Chop", aria "Chop to pads"), disabled when there is no sample or no buffer. It opens a popover aligned to the end (`:159-171`)
- [ ] A change in `chopRequest` opens the chop popover if the pad has a sample (`:109-113`)

### Chop panel (`:246-300`)
- [ ] Title "Chop to pads". Mode radiogroup: **Hits** (transients, sensitivity reset to 0.5), **4**, **8**, **16** (equal slices) (`:262-275`)
- [ ] Sensitivity range input 0..1 step 0.01, shown only in transients mode (`:276-289`)
- [ ] Text: "{n} slice(s) → pads A1–X. Each pad keeps the same file with its own start and end." The last pad is `min(15, from + n - 1)`. With zero slices: "No slices found in the trimmed part." (`:259,290-294`)
- [ ] Apply button "Chop to {n} pad(s)", disabled at 0 → `onChop(pad, sample, slices)`, then the popover closes (`:120-124,295-297`)
- [ ] Slices are recomputed only while the popover is open and a buffer exists (`:115-118`)

### Trim waveform (`:316-470`)
- [ ] Canvas: centre line, the whole waveform at 22% white, and the trim range in the pad colour. Peak columns = `max(32, round(w))`. Redraws on resize (`:325-354`)
- [ ] With no buffer, an overlay message: "Loading…"; or the missing text / "Sample file not found. Drop a new one on the pad."; or, when empty, "Drop a sample on a pad, or press Enter in the browser." (`:414-420`)
- [ ] Missing-pack text: "This pad plays a sound from {name}. Get the pack to hear it." For an unavailable pack: "…{name}, which is no longer available to download. Clear the pad or replace it with a built-in sound." (`:179-185`)
- [ ] Areas outside the trim range are dimmed. Chop slices are drawn as numbered warning-colour markers (`:425-433`)
- [ ] Tooltip "Drag the handles to trim. Double-click to reset." **Double-click** sets `{startSec:0, endSec:null}`, then calls `onChangeEnd` (`:405-410`)
- [ ] Two handles, `role=slider` with aria "Trim start" / "Trim end", valuetext `formatSeconds`. The start handle's square sits at the top, the end handle's at the bottom (`:435-464`)
- [ ] **Handle drag** (primary button, only with a buffer). Seconds = cursor x / width × duration, clamped. Start = `min(sec, end - 0.005)`. End = `max(sec, start + 0.005)`; if within 1e-4 of the duration it becomes `null` (meaning full length). Pointer up calls `onChangeEnd` (`:21,365-386`)
- [ ] **Handle keys**: Right/Up = +1, Left/Down = −1. Step = `(shift ? 0.01 : 0.001) × max(1, duration)` seconds, with the same clamps. Each key press calls `onChangeEnd` (`:387-398`)

### Knobs (shared behaviour: `components/ui/Knob.tsx`)
- [ ] Shared Knob behaviour:
  - Vertical drag: 200px covers the full range, 800px with Shift. Drag changes are applied once per animation frame (`Knob.tsx:27,82-100`)
  - Wheel: ±`max(step, range × 0.015)`, with no `onChangeEnd` (`:112-123`)
  - Double-click resets to the default (`:125-134`)
  - Arrow keys: ±`max(step, range × 0.01)`, or `× 0.1` with Shift. Home = min, End = max, Delete/Backspace = default (`:139-159`)
  - Values snap to the step and are clamped
  - The value text is shown above the label (`:254-263`)
- [ ] **Gain**: −60..+12 dB, step 0.5, default 0. Format `+X.XdB`, `0.0dB`, `-X.XdB` (one decimal; "+" only above 0) (`:49,196-197`)
- [ ] **Pan**: −1..1, step 0.01, default 0. Format "C" when |v| < 0.005, otherwise `L##` or `R##` (rounded percent) (`:50,198-199`)
- [ ] **Pitch**: −24..+24 semitones, step 1, default 0. Format `+Nst`, `0st`, `-Nst` (`:51,200-201`)
- [ ] **Decay**: the knob runs 0..1 on a log scale over 0.005–4 s, step 0.001, default `toLog01(0.05)`. Display is `formatSeconds`. Written as `decay = fromLog01(t)` (`:17-18,202-204`)
- [ ] **Filter**: knob 0..1 on a log scale over 20–20000 Hz, default 1. Display shows "Open" when t ≥ 0.999, otherwise `formatHz`. Written as `filterCutoff` = 20000 when t ≥ 0.999, otherwise the rounded Hz value (`:19-20,205-207`)
- [ ] All knobs: `onChange` → `editPad` (an undo group that stays open and auto-closes after 700ms idle); `onChangeEnd` → `endPadEdit`

### Discrete controls (each calls `onChange` then `onChangeEnd`, so each is one undo step)
- [ ] **Play mode** radiogroup: One-shot (`'oneshot'`) / Gate (`'gate'`) (`:210-215`)
- [ ] **Reverse** toggle (`aria-pressed`, filled/inverted style when on) (`:216-226`)
- [ ] **Choke** `<select>` (aria "Choke group"): Off, 1–8 (`:22,227-239`)

---

## 6. StepSequencer (`components/beat/StepSequencer.tsx`)

### Header (`:367-412`)
- [ ] "Steps" label. Step-size radiogroup (aria "Step size"): 1/8, 1/16, 1/32 → `onGridChange` (persisted) (`:368-385`)
- [ ] Readout "{steps} steps · {bars} bar(s)" (`:386-388`)
- [ ] If `loopHint` is set: warning text (title "The transport loops a different range than this grid shows") plus a **Loop pattern** button (Repeat icon, tooltip "Loop exactly this pattern") → `loopPattern` (`:389-400`)
- [ ] Otherwise, only when the container is at least 760px wide: hint "Click to toggle, drag to paint, Option-drag or scroll for velocity", with "Alt" instead of "Option" on non-Mac (`:401-405`)
- [ ] **Clear** button (Trash2 icon), tooltip "Clear the whole pattern", shortcut ⇧⌫. Disabled when `onClear` is undefined, which happens when the pattern has no notes (`:406-411`)

### Geometry
- [ ] Label column 132px. Cell width = `max(18, floor(boxW/steps))`. Row height = `max(14, floor(boxH/16))`. Scrolls horizontally, never vertically (`:67-69,137-140,433`)
- [ ] Rows run top to bottom from pad 15 to pad 0 (`rowForPad`)

### Canvas drawing (`:144-190`)
- [ ] Each beat is a shaded block, alternating surface-0 and surface-1 (`:157-160`)
- [ ] The selected pad's row gets a `oklch(1 0 0 / 0.045)` tint (`:161-163`)
- [ ] Filled cell: pad colour at alpha `0.3 + 0.7 × velocity`. Empty cell: surface-3 on the first step of a beat, surface-2 otherwise. Inset 2px when the cell is at least 24px wide, else 1.5px. Corner radius `min(3, cellW/6)` (`:165-185`)
- [ ] Bar lines at every `perBar` steps: 1px, `oklch(1 0 0 / 0.16)` (`:188-189`)

### Playhead (`:193-219`)
- [ ] Hidden unless playing. It moves in whole steps: one cell wide, a 7% foreground fill plus a 2px left line in signal-play green. Hidden when the position is outside the pattern (`playheadStep` returns null)

### Mouse gestures (`:260-296`)
- [ ] **Left pointer down on a cell**: captures the pointer, focuses the canvas, hides the keyboard focus ring, moves the focus cell there, and selects the pad if it differs (`:260-272`)
- [ ] **Alt/Option-drag** starting on a filled cell: velocity drag. `adjustVelocity(startVel, startY - y)` with 120px = full range. The undo group "Step velocity" opens on the first change. Calls `onSetVelocity` (`:273-276,285-292`)
- [ ] Alt-click on an empty cell does nothing beyond the selection and focus above (`:273-276`)
- [ ] **Click or drag paint**: the mode comes from the first cell (empty → draw, filled → erase). Each cell is visited at most once per drag and toggled only if needed (`needsToggle`) → `onToggleStep(pad, step, stepBeats)`. The undo group "Paint steps" or "Erase steps" opens on the first toggle (`:251-258,277-279,294-295`)
- [ ] Pointer up, cancel, lost capture or unmount: ends the drag and calls `onGestureEnd` if a group was opened (`:228-233,449-451`)
- [ ] **Wheel** over a filled cell, only when vertical movement dominates: preventDefault, then `adjustVelocity(v, deltaY<0 ? +6 : -6)` (±0.05) → `onNudgeVelocity`, the 400ms burst-grouped undo (`:299-319`)

### Keyboard (canvas `tabIndex=0`, `role=grid`) (`:342-363`; `stepKeys.ts`)
- [ ] aria-label "Pattern grid, 16 pads by {steps} steps. Arrow keys move, Space toggles, plus and minus set velocity." (`:439`)
- [ ] Handled keys get preventDefault and stopPropagation, and the keyboard focus ring is shown (`:346-348`)
- [ ] A move that lands on another pad's row also selects that pad (`:349-351`)
- [ ] Toggle ignores auto-repeat (`:352-354`)
- [ ] Velocity keys act only on a filled cell, via `nudgeVelocity` (`:355-360`)
- [ ] Each handled key announces the cell in a polite live region (e.g. "Kick, step 5, on, velocity 80%") and scrolls the cell into view (`:326-340,483-485`)
- [ ] The focus ring shows only when the focus is keyboard focus (`:focus-visible`); blur hides it (`:453-462,465-470`)
- [ ] The focus cell's step is clamped to `steps - 1` when the step count shrinks (`:323`)

### Row labels (`:490-524`)
- [ ] Button with aria "Select pad A1", `aria-pressed`. Click → `onSelectPad` (`:506-516`)
- [ ] Colour bar (opacity 1 with a sample, 0.35 without), `padLabel`, then the name or "—". Name text is bold when selected, 80% foreground when named, muted when empty (`:517-521`)

---

## 7. useBeat / useBeatActions (`hooks/useBeat.ts`): engine calls

### Snapshot (`:99-127`)
- [ ] `trackId = getActiveDrumTrackId()`. The kit comes from `getKit(trackId)`, or a cached factory kit when there is no track (`:84-85,113`)
- [ ] `patternStartBeat = loopMarkers.start ?? patternRegion.start ?? 0` (`:106`)
- [ ] During pattern playback or loop recording, `loop = getActiveLoopRange()` and `loopEnabled = loop !== null`. Otherwise `loop` is the stored markers and `loopEnabled = isLoopEnabled()` (`:108-121`)
- [ ] Also exposes `selectedPad`, `loop` settings, `transportState`, `bpm`, `countInRemaining`, `isLoopRecording`, `patternPlayback`, `drumTracks` (`:114-125`)
- [ ] `listDrumTracks`: keeps tracks whose instrument type is `'drums'`. Repeated names get numbered ("Drums", "Drums 2") (`:68-76`)
- [ ] `usePatternRegion`: `{region, version: notesVersion hash, startBeat}`; only the sequencer re-renders on note changes (`:88-97,162-183`)
- [ ] `userKits` comes from `kitStore.list()` and its subscription. `hasClipboard` reflects the module-level pad clipboard (`:186-198,222-228`)

### Actions
- [ ] `track()` returns the active drum track id, or calls `ensureDrumTrack()` to create one (`:237`)
- [ ] `selectPad` → `setSelectedPad` (`:263`)
- [ ] `updatePad` → `updatePad(track, pad, patch)` (`:265-267`)
- [ ] `editPad`: `beginUndoGroup('Edit pad')` if no group is open, then `updatePad`. The group closes after 700ms idle or on `endPadEdit` (`:240-276`)
- [ ] `assignFile` → `setPadSample(track, pad, {source:{kind:'file',path}, name})` (`:282-284`)
- [ ] `clearPad` → `setPadSample(track, pad, null)` (`:286-288`)
- [ ] `renamePad`: trims the name and skips it if empty, unchanged, or the pad has no sample; otherwise `setPadSample` with the new name (`:290-296`)
- [ ] `copyPad`: copies a deep clone of the pad settings to the clipboard (`:298-302`)
- [ ] `pastePad`: in undo group "Paste pad", `setPadSample(sample)` then `updatePad(rest)` (`:304-315`)
- [ ] `chopToPads`: runs `planChop`; in undo group "Chop to pads", every pad other than the source gets a copy of the source with name "{name} {i+1}"; every pad gets `updatePad({startSec, endSec, reverse:false})` (`:318-331`)
- [ ] `beginStepGesture(label='Edit steps')` / `endStepGesture` → `begin/endUndoGroup` (`:339-355`)
- [ ] `nudgeStepVelocity`: inside a drag it joins the drag's group; otherwise it opens a "Step velocity" burst group that closes after 400ms idle. Calls `setStepVelocity` (`:82,371-384`)
- [ ] `toggleStep(pad, step, stepBeats, velocity?)` → `engine.toggleStep(track, ...)`. The default velocity is the engine's 0.8 (`utils/arrangementHelpers.ts:40`) (`:358-360`)
- [ ] `setStepVelocity` → engine (`:362-364`)
- [ ] `clearPattern(pad?)`: only if an active drum track exists → `engine.clearPattern(id, pad)` (`:386-389`)
- [ ] `loopPattern`: in undo group "Loop pattern", enables the loop if it is off, then `setLoopMarkers(patternStart, patternStart + bars × 4)` (`:396-406`)
- [ ] `togglePlay`: if stopped and not loop recording → `playPattern()`; otherwise stop. Stop calls `stopLoopRecording()` if recording, then `engine.stop()` (`:412-423`)
- [ ] `toggleRecord`: `stopLoopRecording()` if recording; otherwise `track()` (which ensures a drum track) then `startLoopRecording()` (`:424-431`)
- [ ] `setBpm` → `engine.setBpm`; `setLoopSettings(patch)` → `engine.setLoopSettings` (`:432-433`)
- [ ] `loadKit` / `loadFactoryKit`: `cancelPendingLoad()`, then `setKit(track, cloneKit(kit) | createFactoryKit(), 'user')` (`:439-446`)
- [ ] `saveKit(name)`: name is trimmed, or "My kit" if empty. A kit with the same name is overwritten (its id is reused); otherwise a new UUID is used. Saves a clone of the current kit via `kitStore.save` (`:447-452`)
- [ ] `deleteKit` → `kitStore.remove` (`:453`)
- [ ] `setActiveDrumTrack` → engine (`:457`). `newDrumTrack`: `addTrack('drums')`, then make it active (`:459-463`)
- [ ] `subscribePosition` → `engine.onPositionChange` (`:200`)
- [ ] Read-only hooks for the dock: `useActiveDrumTrackId`, and `useActiveKitPads` (pads and selected pad, refreshed on state or kit changes) (`:517-546`)

---

## 8. Beat sub-components

### 8a. DrumTrackMenu (`components/beat/DrumTrackMenu.tsx`)
- [ ] Trigger: colour dot, then the active track's label or "No track", then a chevron. aria "Drum track: <label|none>", title "Drum track the pads and steps edit". Max width 112px (`:27-37`)
- [ ] Popover: heading "Drum track", then a listbox of tracks (colour dot, label, check mark on the active one). Clicking a track → `onSelect`, then the popover closes (`:40-64`)
- [ ] When there are no tracks: "No drum tracks yet." (`:65`)
- [ ] Separator, then "**+ New drum track**" → `newDrumTrack`, then the popover closes (`:67-78`)

### 8b. KitMenu (`components/beat/KitMenu.tsx`)
- [ ] Trigger: Drum icon, the kit name, a chevron. aria "Kit: <name>". Max width 150px, or 96px while a pending-load chip is shown (`:72-76`; `BeatView.tsx:271`)
- [ ] Opening the menu prefills the save input with the kit name, or leaves it empty when the kit is "Factory" (`:66-69`)
- [ ] Groups:
  - **Built-in**: "Factory" (primary-colour swatch) and the classic built-in kits (each row: colour swatch, name, optional detail)
  - **Installed packs**: shown only if any exist
  - **Your kits**: user kits, each with a trash button that appears on hover (aria "Delete kit X"); when empty, "Saved kits appear here."
  - Scroll area max height `min(70vh, 540px)` (`:79-125`)
- [ ] Picking a built-in or pack choice calls `choice.create()`; if it returns a kit, `onLoadKit` runs. The menu closes either way (`:58-62`)
- [ ] "**Browse packs…**" (Package icon) closes the menu, then calls `onBrowsePacks` (`:126-141`)
- [ ] Save form: input (aria and placeholder "Kit name") plus a "Save kit" submit button; Enter also submits → `onSaveKit(name)`, then the menu closes (`:143-159`)

### 8c. MissingPacksStrip, GetPackButton, UnavailablePackActions (`components/beat/MissingPacks.tsx`)
- [ ] Strip is a `role=status` with aria "Packs not installed". Hidden when nothing is missing (`:67-69`)
- [ ] One row per pack:
  - PackageX icon, then "Not installed: " or "No longer available: ", then the **name** and " · N pad(s)"
  - Title "Pads A1, B3 play sounds from X"
  - Text wraps; it is not truncated (`:70-90`)
- [ ] Error line (`role=alert`): when the pack is available, its state is error and not paused → the error message or "Download failed" (`:71,91-95`)
- [ ] GetPackButton states come from `packButtonState`:
  - Get (aria "Get X")
  - Downloading: progress "NN%", or "i/n", or "…", plus a cancel button (tooltip "Cancel download")
  - Resume
  - Retry on error
  - Installed
  - Click calls `packs.get(id)`, which loads the catalog and then runs `install(id)`. Cancel calls `packService.cancel(id)` (`:14-24`; `packs/packsModel.ts:260-275`; `usePackAvailability.ts:67-69`)
- [ ] Unavailable pack:
  - "**Clear pads**" (tooltip "Empty this pad" / "Empty these N pads") → fix `'clear'`
  - "**Replace with built-in**" (tooltip "Put the built-in kit's sounds on …") → fix `'builtin'` (`:27-45`)
- [ ] `fixMissingPads`, in undo group "Clear pads" or "Replace pads":
  - clear: `setPadSample(null)`
  - builtin: the factory sample for the same pad index, then `updatePad({startSec:0, endSec:null, reverse:false})` (`missingPackActions.ts:20-42`)

### 8d. PendingLoadChip (`components/beat/PendingLoadChip.tsx`)
- [ ] Rendered only when `packService` has a pending load (`:34-39`)
- [ ] Downloading: a progress ring (aria "Downloading X"), "Getting " shown only when the chip is at least 150px wide, then "X…". Title: "Getting X: its kit loads on these pads when the download is done (choosing another kit cancels it)" (`:20-26,61,64`)
- [ ] Waiting for the take to end: a pulsing red dot, "After take: X". Title: "X is downloaded: its kit loads on these pads when recording stops" (`:16-19,58-59`)
- [ ] ✕ button (aria "Cancel loading X"; tooltip "Don't load this kit" or "Stop getting this kit"). While downloading it calls `packService.cancel(id)`; otherwise `cancelPendingLoad()` (`:44-47,67-76`)

### 8e. Pack availability and missing-pack data (`components/beat/usePackAvailability.ts`)
- [ ] "Missing" only counts once installed packs are known: `isMissing = loaded && !installed`. The pack id is read only from `{kind:'pack'}` sources (`:59`; `packs/packsModel.ts:318-321`)
- [ ] `missingPacks(pads, avail)` groups pads by pack id in pad order, as `{id, name, pads[], state, available}` (`:78-91`)
- [ ] `missingPackTitles(missing, n)`: per pad, "Pack not installed: X" or "Pack no longer available: X", else null (`:98-105`)

### 8f. Pad buffer loading (`components/beat/usePadBuffer.ts`)
- [ ] Statuses: empty, loading, ready, missing. A cached buffer makes the state ready immediately. A load failure gives missing. Pack-sound buffers reload when the pack is invalidated (`:5-61`)

---

## 9. InstrumentDock and DockPads (`components/InstrumentDock.tsx`)

- [ ] Header: "Play" label, then the **Pads** toggle (only if `padsAvailable`; false in Beat view) and the **Keys** toggle (`aria-pressed`) (`:148-158`)
- [ ] "MIDI route" with a RoutingModeSelector (Auto / Keys / Split). Item titles:
  - "Auto: notes mapped to pads in your controller profile trigger pads; other notes play the synth" (was "low keys trigger drums, the rest play the synth")
  - "Keys: every key plays the synth"
  - "Split: keys below the split point (C4 by default) trigger the 16 pads; the rest play the synth" (was "left half drums, right half synth"; Split implemented in both apps, phase-5 review #11)
  - Choosing one calls `setRoutingMode` (`:161-162`; `RoutingModeSelector.tsx:9-13`)
- [ ] Live readout (`aria-live`): chord display or "—" (title "Detected chord"), then up to 8 held note names (sharps, C4 = 60), or "no notes held" (`:165-184`)
- [ ] Collapse button: when collapsed, tooltip "Show instruments" and aria "Expand instrument dock", and a click shows both pads and keys. When expanded, "Hide instruments" / "Collapse instrument dock", and a click hides both (`:186-203`)
- [ ] `collapsed = !showPads && !showKeys`. `defaultCollapsed` starts both off. The body is 156px tall (`:137-142,207-221`)
- [ ] **DockPads** (`:52-106`):
  - Bank radiogroup: B on top, A below. aria "Bank A (pads A1–A8)", title "Bank A: pads A1–A8"
  - Persisted in `micro-daw:dock-pad-bank`, default A
  - Compact PadGrid for one bank: no key hints, no context menu, no drop target
  - `onSelect` → `engine.setSelectedPad`
  - Missing-pack dots, and the catalog loads while anything is missing

---

## 10. beatHelpers.ts: pure functions (exact semantics)

| Function | Semantics |
|---|---|
| `padRows(layout)` (`:25-38`) | `'2x8'` gives `[[4,5,6,7,12,13,14,15],[0,1,2,3,8,9,10,11]]`. Otherwise (4x4) gives `[[12..15],[8..11],[4..7],[0..3]]`. Rows are listed top to bottom. |
| `padBank(pad)` (`:40`) | `pad < 8` gives `'A'`, else `'B'`. |
| `padLabel(pad)` (`:43`) | `${bank}${pad%8+1}`, e.g. pad 9 → "B2". |
| `padColor(settings, pad)` (`:45-47`) | `settings?.color ?? PAD_COLORS[pad % 16]`. The 16 oklch values are listed in `audio/sampler/factoryKit.ts:74-91`. |
| `padName(settings)` (`:49-57`) | No sample → `''`. Otherwise `sample.name` if non-empty. Otherwise factory source → `FACTORY_VOICE_LABELS[voice]`, classic → `classicSampleName(machine, voice)`, pack → `displayName(file)`, file → `displayName(path)`. `displayName` strips the last extension unless the dot is at index 0. |
| `hardwareNoteLabel(note)` (`:60-63`) | Null or non-finite → "—". Otherwise `midiToNoteName(round(note))`: sharps, octave = `floor(n/12) - 1`, so 36 → "C2". |
| `velocityFromPointer(y, top, h)` (`:69-73`) | `h <= 0` or NaN → 0.8. Otherwise `t = clamp((y-top)/h, 0, 1)` and the result is `round((1 - 0.7t) × 100) / 100`: 1.0 at the top edge, 0.3 at the bottom. |
| `KEY_CODE_TO_PAD` / `KEY_TO_PAD` (`:83-94`) | Z X C V → 0–3; A S D F → 4–7; Q W E **T** → 8–11 (R is reserved for record); 1 2 3 4 → 12–15. |
| `PAD_KEY_HINTS` (`:97-99`) | Upper-case key cap per pad: `['Z','X','C','V','A','S','D','F','Q','W','E','T','1','2','3','4']`. |
| `padForKeyEvent(e)` (`:102-107`) | Meta, Ctrl or Alt held → null. Otherwise the pad for `e.code` if it is in the code map; else the pad for `e.key.toLowerCase()` if in the key map; else null. Shift is not checked here; the caller rejects it. |
| `isTypingTarget(t)` (`:110-115`) | True if the target is contentEditable or sits inside `input, textarea, select, [contenteditable=""\|"true"], [role=combobox], [role=textbox]`. |
| `STEP_GRIDS` (`:122`) | `['1/8','1/16','1/32']`. |
| `stepBeatsFor(g)` (`:124-126`) | 1/8 → 0.5; 1/32 → 0.125; anything else → 0.25. |
| `stepCount(bars, sb)` (`:128-130`) | `max(1, round(bars × 4 / sb))`. |
| `stepsPerBeat(sb)` (`:133`) | `max(1, round(1/sb))`. |
| `patternStepAt(beat, sb, steps)` (re-exported, `utils/arrangementHelpers.ts:86-89`) | `round(clean(max(0,beat)/sb))`, clamped to `[0, steps-1]`. `clean` rounds to 1e-9. |
| `normVelocity(v)` (`:142`) | `clamp(v > 1 ? v/127 : v, 0, 1)`. |
| `buildStepMatrix(region, patternStart, sb, steps)` (`:150-171`) | Returns a Float32Array of 16 × steps, indexed `pad × steps + step`. Null region → all zeros. For each note: `pad = note - 36`, skipped if outside 0..15. `beat = region.start + note.start - patternStart`, skipped if `beat < -1e-4` or `beat >= steps × sb - 1e-4`. Step from `patternStepAt`. Cell value = `max(existing, max(0.02, normVelocity(vel)))`. |
| `playheadStep(pos, start, patBeats, sb)` (`:174-179`) | `patBeats <= 0` → null. `rel = pos - start`; if `rel < 0` or `rel >= patBeats` → null; otherwise the fractional step `rel / sb`. |
| `cellAt(x, y, cellW, rowH, steps)` (`:182-189`) | Null if x < 0, y < 0, or a size is not positive. `step = floor(x/cellW)`, `row = floor(y/rowH)`; null if `step >= steps` or `row >= 16`. Otherwise `{pad: 15 - row, step}`. |
| `rowForPad(pad)` (`:191`) | `15 - pad`. |
| `paintModeFor(v)` (`:196`) | `v > 0` → `'erase'`, else `'draw'`. |
| `needsToggle(mode, v)` (`:199-200`) | draw → `v <= 0`; erase → `v > 0`. |
| `adjustVelocity(start, deltaUp, px=120)` (`:203-206`) | `round(clamp(start + deltaUp/px, 0.05, 1) × 100) / 100`. |
| `dragHasSample(dt)` (`:220-224`) | True if `dt.types` includes `'application/x-micro-daw-sample'` or `'Files'`. |
| `readDroppedSample(dt, getPath?)` (`:230-253`) | First tries the MIME JSON. If its `path` is a non-empty string it returns `{path, name: name if a non-empty string, else displayName(last path segment)}`; invalid JSON falls through. Then the first file whose name ends in an audio extension (wav, aif, aiff, mp3, flac, ogg, m4a; case-insensitive) and whose `getPath(file)` is non-empty gives `{path, name: displayName(file.name)}`. Otherwise null. |
| `fileSample(s)` (`:255`) | `{source: {kind:'file', path}, name}`. |
| `planChop(from, ranges)` (`:268-276`) | `ranges[i]` goes to pad `from + i`; stops at pad 16. |
| `fromLog01(t, min, max)` (`:283-284`) | `min × (max/min)^clamp(t, 0, 1)`. |
| `toLog01(v, min, max)` (`:285-286`) | `clamp(ln(max(min, v)/min) / ln(max/min), 0, 1)`. |
| `formatSeconds(s)` (`:288-290`) | Below 1 s: `${round(s × 1000)}ms`; otherwise `${s.toFixed(2)}s`. |
| `formatHz(hz)` (`:292-294`) | ≥ 10000 → `${(hz/1000).toFixed(0)}k`. ≥ 1000 → `toFixed(1)` + "k". Otherwise `${round(hz)}`. |
| `beatsLabel(beats)` (private, `:300-307`) | If `beats/4` is within 1e-4 of a whole number → "N bar" or "N bars". Otherwise `${+beats.toFixed(2)} beats`. |
| `describeLoopMismatch({loop, loopEnabled, playing, patternStartBeat, bars})` (`:317-334`) | Rules are checked in order:<br>1. Playing and loop off → "Loop is off: playback runs past the pattern".<br>2. No loop → null.<br>3. Same start (within 1e-4) and same length (within 1e-4 of `bars × 4`) → null.<br>4. Length differs → "Loop is {beatsLabel(loopLen)}, pattern is {beatsLabel(bars × 4)}".<br>5. Otherwise → "Loop range starts away from the pattern". |

### Related pure helpers
- [ ] `stepKeyAction(e, cell, steps, perBeat)` (`stepKeys.ts:26-72`). Rows are clamped to 0..15 and steps to 0..last:
  - **→ / ←**: ±1 step. With Shift, jumps to the next beat line `(floor(s/pb)+1) × pb`, or the previous one `(ceil(s/pb)-1) × pb`.
  - **↑ / ↓**: pad ±1, or ±4 with Shift.
  - Arrows with Alt, Cmd or Ctrl → null.
  - **Home**: step 0; with Cmd/Ctrl, also pad 15. **End**: last step; with Cmd/Ctrl, also pad 0.
  - **Space / Enter** without modifiers → toggle.
  - **+ = ]** → +0.1 and **- _ [** → −0.1, without Alt, Cmd or Ctrl.
- [ ] `nudgeVelocity(v, d)` = `round(clamp(v+d, 0.05, 1) × 100) / 100` (`stepKeys.ts:75-77`)
- [ ] `describeStep(name, step, v)`: "{name}, step {step+1}, on, velocity {round(v × 100)}%" or "{name}, step {step+1}, off" (`stepKeys.ts:80-83`). The name used is `padName` or "Pad A1" (`StepSequencer.tsx:327`)
- [ ] `isScopedKeyTarget(t)`: a typing target, or inside `[data-keys-scope], [role=dialog|alertdialog|menu|listbox]` (`keyScope.ts:11-29`)
- [ ] `chopRanges(buffer, trim, mode, fromPad)` (`chop.ts:30-51`):
  - Trim is clamped to the buffer: `start = clamp(startSec || 0, 0, total)`, `end = clamp(endSec ?? total, start, total)`. A non-positive length gives no ranges.
  - At most `16 - fromPad` slices.
  - Equal mode: `equalSlices(dur, min(count, max))`. Transients mode: `detectTransients` on a mono mix of the trimmed range, with the given sensitivity and max.
  - Results are absolute seconds.
- [ ] `swingGridLabel` gives "8ths" or "16ths" (`BeatTransport.tsx:125`)
- [ ] Waveform helpers (`waveform.ts:21-99`):
  - `computePeaks`: per column, min/max over all channels, sampling with stride `floor(span/512)`.
  - `drawPeaks`: mirrored bars, at least 1px tall, `colW - 0.25` wide.
  - `prepareCanvas`: sizes the backing store by devicePixelRatio.

---

## 11. useDrumPads (`hooks/useDrumPads.ts`)
- [ ] `padFlash.flash(id)` lights a pad for **150ms**. Another hit on the same pad restarts the timer. It is a module-level store, so only `useActivePads` subscribers re-render (`:5,22-45`)
- [ ] `useActivePads()` and the legacy `useDrumPads()` return `{activePads, flashPad}` (`:48-55`)

## 12. useNoteRepeat (`hooks/useNoteRepeat.ts`)
- [ ] Mirrors the `noteRepeat` service as `{enabled, rate, toggle, setRate}`. Default rate is 1/16. Rate beats: 1/4 = 1, 1/8 = 1/2, 1/8T = 1/3, 1/16 = 1/4, 1/16T = 1/6, 1/32 = 1/8 (`:10-16`; `services/NoteRepeat.ts:23-33`)
- [ ] Held pads retrigger on that grid while playing; tooltips say "while playing". When stopped, the service runs its own free-running grid (`services/NoteRepeat.ts:18-21`)

---

## 13. DrumPads (legacy 8-pad) (`components/DrumPads.tsx`)
- [ ] 4×2 grid of the 8 `DRUM_SOUNDS`: Kick/KCK, Snare/SNR, Closed HH/CHH, Open HH/OHH, Clap/CLP, Tom/TOM, Crash/CRS, Ride/RDE (`utils/constants.ts:41-50`)
- [ ] `looks` overrides per pad: name is `look.name || 'Empty'`; short name is `shortPadName(name)` or "—" (`:44-57`)
- [ ] Left pointer down → `onHit(id, 0.8)` and restarts the `drum-pad-hit` CSS animation. Right-click is suppressed (`:77-93,120`)
- [ ] States: active (filled with the pad colour mix, white text), highlighted target (inset ring plus dot), idle. Exposed as `data-state` (`:98-133`)
- [ ] `shortPadName(name)`:
  - Split the trimmed name on whitespace, `_` or `-`. No words → "—".
  - Squeeze the last word: words longer than 2 characters keep their first letter and drop the vowels a/e/i/o/u after it.
  - Prefix the initials of the earlier words.
  - Take the first `min(4, max(3, heads + 2))` characters and upper-case them.
  - Examples: "Kick" → "KCK", "Closed HH" → "CHH" (`:25-33`)

---

## 14. TransportBar (Song view only) (`components/TransportBar.tsx`)
- [ ] **Play**: disabled while recording. Tooltip "Playing" or "Play", shortcut Space, `aria-pressed = playing` (`:167-182`)
- [ ] **Stop**: disabled when stopped. While recording it calls `onStopRecording`, otherwise `onStop` (`:183-194`)
- [ ] **Record** (wrapped in a span so the tooltip works while disabled). Tooltip: "Stop recording" / "Arm a track to record" / "Punch record (loop)" / "Record", shortcut R. Disabled when not recording and `recordDisabled` (no armed track) (`:195-218`)
- [ ] **Loop** toggle (only if `onLoopToggle`): tooltip "Loop on" / "Loop off", shortcut L (`:220-233`)
- [ ] **Metronome**: tooltip "Metronome on" / "Metronome off", shortcut M (`:234-245`)
- [ ] Position LCD (aria "Song position"):
  - Bars.beats.sixteenths: bar = `floor(b/4) + 1`, padded to 3 characters with spaces; beat = `floor(b%4) + 1`; sixteenth = `floor((b%1) × 4) + 1`; negative positions clamp to 0.
  - Time `m:ss.s`, shown only at 1440px window width or wider.
  - A "REC" badge while recording, or "PUNCH" (warning colour) when recording with the loop on (`:37-50,249-267,283-297`)
- [ ] BpmField: same rules as §2 (40–240, wheel ±1, arrows ±1/±10, drag `dy/2` or `dy/8`). Tooltip "Tempo: drag, scroll or type"; 13px text (`:63-140`)
- [ ] Marker jump dropdown, only if markers exist and `onSeekToMarker` is passed (`:271-273`)

## 15. TopBar (`components/TopBar.tsx`)
- [ ] Window drag region, with no-drag on the controls. Left padding 76px on macOS in Electron when not fullscreen (for the traffic lights) (`:56-57,155-165`)
- [ ] **Project/Save** button: app mark, project name (truncated at 120px, 180px from a 1280px window), and a warning dot (aria "Unsaved changes") when dirty. Tooltip "Unsaved changes. Save" or "Save project", shortcut ⌘S → `onSave` (`:170-182`)
- [ ] DAW mode only:
  - **Undo** / **Redo**: disabled when there is nothing to undo/redo; shortcuts ⌘Z and ⌘⇧Z (`:183-196`)
  - **Sample browser** toggle: `aria-pressed`; tooltip "Hide sample browser" / "Show sample browser", shortcut B (`:198-211`)
  - **View switch** Beat | Song: radiogroup with aria "View"; tooltip "Switch Beat / Song view", shortcut Tab (`:101-129,212`)
- [ ] Centre: in Beat view, `LiveInputReadout` shows the chord or "—", then up to 8 note names or "Play something…" (`:73-99,216-218`)
- [ ] Right side:
  - MIDI device selector (only at 1060px window width or wider)
  - Controller setup button (SlidersHorizontal icon), tooltip "Controller setup: map pads and knobs"
  - InstrumentSelector (preset and synth-editor toggle)
  - VolumeControl (only at 1180px or wider)
  - Fullscreen toggle: tooltip "Full screen" / "Exit full screen", aria "Fullscreen" / "Exit fullscreen"; synced from `electronAPI.onFullscreenChanged` (`:143-153,221-247`)

---

### Inconsistencies to keep (or decide on deliberately)
1. Pad velocity depends on input: computer keys 0.85, on-screen Enter/Space 0.8, mouse 0.3–1.0 by click height, editor audition 0.85, legacy DrumPads 0.8.
2. Choke groups: the context menu offers 0–4; the editor's select offers 0–8.
3. Context-menu colour and choke changes call `updatePad` directly, outside any undo group. The same edits in PadEditor are grouped.
4. R toggles record even with Shift held; Shift plus any pad key is ignored.
5. Knob wheel changes never call `onChangeEnd`, so the "Edit pad" undo group relies on its 700ms idle timer to close.
6. The step-sequencer wheel only acts on filled cells and only when vertical scroll dominates. Alt-click on an empty cell does nothing.