# Engine parity checklist: micro-daw (Tone.js) → Rust (cpal + sample-accurate scheduler)

All paths are relative to ``. Cites are `file:line`. **AE** = `src/services/ArrangementEngine.ts`. **LR** = `src/services/arrangement/LoopRecorder.ts`. **AH** = `src/utils/arrangementHelpers.ts`.

Where the code says "Tone default", the Rust port has to copy Tone's behaviour on purpose. Nothing in `src/` configures `lookAhead`, `updateInterval` or `PPQ`. The tests assume PPQ = 192 (`NoteRepeat.test.ts:13`) and lookAhead = 0.1 s (`src/test/toneMock.ts:419`).

---

## 0. Global timing model

- **Two clocks.**
  - `Tone.immediate()` is the raw context time: what you hear now.
  - `Tone.now()` is immediate + lookAhead (0.1 s).
  - Transport/Part callbacks get a `time` already shifted by the lookAhead.
- **Live paths skip the lookAhead.**
  - Live synth: `immediate + 4 ms` (`LIVE_OFFSET`, `AudioEngine.ts:18,118-120`).
  - Live drum pads: `immediate` (`AE:75-78`, `DrumEngine.ts:75`).
  - Piano-roll previews: `immediate` (`AE:2294`).
- **Musical position comes from transport ticks, not seconds × BPM.** `transportBeatAt(time)` returns `ticks/PPQ` (`AE:323-338`).
  - Exception: before a scheduled start it returns the remembered start beat (`AE:327-328`, fix B1).
  - Fallback when there is no tick clock: `seconds × bpm/60`.
- **Times are placed in seconds at the current tempo.** Part events, loop points and automation are converted with `beat × 60/bpm` (`AE:1398, 1127-1128, 1897`). Tone turns them into ticks when they are added.
  - Rust: store beats/ticks directly.
  - Loop points are seconds, so they are re-applied on every BPM change (`AE:1270-1276`).

---

## 1. Public API per service

### 1.1 ArrangementEngine (singleton `arrangementEngine`, `AE:2845`)

**Undo conventions.**
- `pushUndoSnapshot(label)` clones the whole arrangement before the edit (`AE:1555-1562`).
- Inside an undo group it calls `markDirty()` instead (no clone).
- `undoSnapshot()` + `commitUndo()` is the deferred variant (`AE:1565-1572`).
- Quirk: most mutators push the snapshot **before** they validate the target. A missing track/region therefore still leaves a no-op undo entry. Examples: `setTrackEffect` `AE:561`, `addRegion` `AE:648`, `stopRecordingToTrack` `AE:910` (pushes even when nothing was captured). Decide whether to keep this.

**Getters and session**

| Method | Semantics |
|---|---|
| `getSessionId()` | Bumped by `loadProject` / `resetToStarter` / `loadFromTemplate` / `loadFromLocalStorage`, and by `restoreFromSnapshot` when the snapshot id differs. Never bumped by edits or undo. `AE:149,193-210,1589` (N2) |
| `getArrangement / getTracks / getState / getBpm` | Raw state. `getState` returns `'stopped'\|'playing'\|'recording'` |
| `getPosition()` | 0 when stopped, else `transportBeatAt()` (`AE:312-315`) |
| `getActiveLoopRange()` | Precedence: loop-rec take `[patternStart, +loopBeats)`, then pattern playback `[patternStart, +bars*4)`, then Song loop markers if the loop is on, else null (`AE:358-367`) |
| `isPatternPlayback / isMetronomeEnabled / isLoopEnabled / getLoopMarkers / getArmedTrackId / getRecordingTrackId / isPunchRecordingActive` | Flags |
| `getLiveRegion()` | While recording: a synthetic `__live__` region built from captured notes plus held notes (held notes get duration so far, min 0.125). Punch mode uses marker range/length; otherwise start = recordingStartBeat, length `max(0.25, now - start)` (`AE:386-426`) |
| `getUnavailableAudioRegions / isSampleUnavailable / isAudioRegionUnavailable / getSampleRevision / onSamplesChanged` | N5 sample status (`AE:219-255`) |

**Tracks**

| Method | Undo | Semantics |
|---|---|---|
| `addTrack(type='synth', presetIndex=0)` | 'Add Track' | Volume −6 dB, pan 0, colour cycles `TRACK_COLORS`. Name: drums `uniqueTrackName('Drums')` (e.g. "Drums 2"), audio "Audio", synth = preset name. Drums get a factory kit, and the first drum track becomes active. Builds audio nodes and syncs the drum sink (`AE:430-466`) |
| `removeTrack(id)` | 'Remove Track' | Stops a loop take on that track first. Disposes nodes, clears the arm, removes the track from groups and drops empty groups (`AE:468-484`) |
| `setTrackInstrument(id, inst)` | 'Change Instrument' | Drums keep the existing kit unless one is given (normalised). A change to drums renames the track to unique "Drums"; any synth change renames it to the preset name. Rebuilds nodes through `withPlaybackRestart` (`AE:486-511`) |
| `setTrackSynthOverrides(id, o)` | 'Edit Synth Params' | Shallow-merges into `synthOverrides`. Synth tracks only. Rebuilds nodes (playback restarts) (`AE:513-523`) |
| `setTrackVolume / setTrackMute / setTrackPan` | **none** | Set immediately (`AE:525-558`) |
| `setTrackSolo` | **none** | Solo logic: if any track is soloed, every non-solo track is muted; otherwise each track's own mute applies (`AE:543-549,1456-1467`) |
| `setTrackEffect(id, name, params)` | 'Change Track Effect' | `Object.assign` into the effect, then `applyTrackEffect` (`AE:560-643`, see §4.3) |
| `getTrackLevel / getMasterLevel` | – | Tone.Meter dB. The master meter is created lazily on Destination (`AE:1320-1334`) |

**Regions**

| Method | Undo | Semantics |
|---|---|---|
| `addRegion` | 'Add Region' | push + autoExtend (`AE:647-654`) |
| `removeRegion` | 'Remove Region' | (`AE:656-662`) |
| `moveRegion(t,r,start)` | 'Move Region' | start clamped ≥0, autoExtend (`AE:664-673`) |
| `moveRegionToTrack` | 'Move Region to Track' | No type check, no autoExtend (`AE:675-685`) |
| `splitRegion(t,r,beat)` | 'Split Region' | No-op if the split is outside `(0, len)`. Audio: right region gets `offsetSeconds += localSplit*60/bpm`. MIDI: notes spanning the split are cut into two. Names get " (L)"/" (R)". No autoExtend (`AE:687-763`) |
| `duplicateRegion` | 'Duplicate Region' | Placed at `start+len`, name " (copy)" (`AE:765-782`) |
| `resizeRegion(t,r,start?,len?)` | 'Trim Region' | Audio: left trim shifts `offsetSeconds`, length min 0.25. MIDI: left trim shifts notes, drops/clips notes before 0. Right trim drops notes starting ≥ len and clips their ends. Min length 0.25 (`AE:1677-1739`) |
| `updateRegionNotes` | 'Edit Notes' | Replaces notes. **No live reschedule** (`AE:1667-1675`) |
| `editRegionNotes(t,r,notes,label,newLen?)` | label | Replaces notes. May only grow the region. Reschedules the running Part (`AE:2262-2280`) |
| `quantizeRegionNotes(t,r,snap)` | 'Quantize Notes' | `quantizeNotes` (`pianoRollHelpers.ts:648-663`) (`AE:1966-1974`) |
| `quantizeSelectedNotes` | 'Quantize Selected Notes' | `round(start/snap)*snap` on the selected indices (`AE:1976-1987`) |
| `copyRegion` / `pasteRegion(t, beat)` / `hasClipboard` | paste: 'Paste Region' | Deep clone, new id (`AE:1991-2018`) |
| `addAudioRegion(t, sampleId, start)` | 'Add Audio Region' | Audio tracks only. `lengthBeats = dur/60*bpm`, `gainDb 0`, `offset 0` (`AE:786-816`) |
| `importAudioFile()` | per addTrack/addRegion | Electron dialog. One new audio track per file (`AE:818-831`) |
| `autoExtendLength()` | – | `ceil(max(64, max(start+len+4))/4)*4` (`AE:1495-1503`) |

**Linear record (Song)**

- `armTrackForRecording(id)`: arming a drum track also makes it the active drum track (`AE:836-840`).
- `startRecordingToTrack(id?)`:
  - Falls back to the armed track. With no tracks at all, it auto-creates a synth track.
  - Needs state 'stopped'.
  - Punch mode when the loop is on and markers exist: transport loops the markers, recording starts at marker start, and at `loopEnd - 50 ms` each pass held notes are closed with duration `max(0.125, loopLen - start)`.
  - Otherwise starts from beat 0.
  - Metronome starts only when the global toggle is on (`AE:842-906`).
- `stopRecordingToTrack()` (undo 'Record', pushed even when empty):
  - Closes open notes.
  - Punch: region = marker range, and **deletes overlapping regions** (replace mode).
  - Otherwise: region starts at 0, `len = max(1, ceil(endBeat))`.
  - Returns null if nothing was captured (`AE:908-988`).
- `captureNoteOn(note, vel, isDrum, time?, {gridBeats})`:
  - Routed to LoopRecorder during a loop take.
  - Otherwise only while 'recording'.
  - Drum notes: fixed 0.25-beat duration.
  - Synth notes: open until note-off.
  - Punch mode stores the position as `(beat - loopStart) % loopLen` (`AE:995-1028`).
- `captureNoteOff(note, time?)`: duration `max(0.125, off-on)` (`AE:1031-1055`).

**Loop / transport** (semantics in §2)
- `setLoopEnabled(b)`: no undo. Turning it on with no markers, or markers covering the whole song, sets the markers to the note range (`getNoteRangeLoopMarkers`, `AE:1059-1083`), else `[0, length)` (`AE:1089-1107`).
- `setLoopMarkers(s, e)`: undo 'Set Loop Markers'. `start ≥ 0`, `end = min(length, max(s+1, e))` (`AE:1109-1118`).
- `play()` **always starts at beat 0** (`AE:1136-1138`).
- `playFrom(beat)` (`AE:2306-2308`), `seek(beat)` (only while playing, `AE:2311-2314`), `setPosition(beat)` (`AE:1250-1268`), `stop()` (`AE:1195-1206`), `setBpm(bpm)` (no undo, `AE:1270-1276`).
- `setMetronomeEnabled(b)` (`AE:1280-1285`), `playPattern()` (`AE:2558-2569`).

**Automation**
- `addAutomationLane`: 'Add Automation Lane', no duplicates (`AE:1743-1752`).
- `removeAutomationLane`: 'Remove Automation Lane', resets the effect nodes immediately (`AE:1754-1763`).
- `setAutomationPoint(t, p, beat, value, snap)`: 'Set Automation Point'. Snaps beat, clamps value 0..1, replaces a point within 0.001 beat, keeps the list sorted (`automationHelpers.ts`, `AE:1765-1773`).
- `deleteAutomationPoint`: 'Delete Automation Point' (`AE:1775-1783`).
- `toggleAutomationLaneVisibility`: no undo. **Hidden lanes are not played** (`AE:1785-1792, 1880, 1893`).

**Markers / groups / templates**
- Markers (`AE:2022-2090`):
  - Add, remove and update each push undo. Markers are kept sorted by beat, beat ≥ 0, colour cycles `MARKER_COLORS`.
  - `seekToNext/Previous` use a ±0.01 tolerance and call `setPosition`.
- Groups (`AE:2094-2184`):
  - A track belongs to at most one group. Empty groups are dropped.
  - `toggleGroupCollapsed` has no undo.
  - `setGroupMute/Solo` loop over the non-undoable track setters.
- `resetToStarter()` (`AE:2193-2202`):
  - New session, default arrangement, loop off.
  - Ensures a "Drums" track, default loop settings, pattern loop markers `[0, 8)`.
  - **Clears undo history.**
- `loadFromTemplate(arr)` (`AE:2204-2233`): new session; fresh UUIDs for project, tracks, regions, markers and groups; remaps `activeDrumTrackId` and group track ids.
- `loadProject(arr)` (`AE:207-210`).
- `saveToLocalStorage / loadFromLocalStorage` (`AE:1527-1551`).

**Undo / redo** (`AE:1574-1663`)
- `beginUndoGroup(label)` / `endUndoGroup()`: the end commits unless the state is JSON-equal to the start.
- `performUndo()`:
  1. `settleHistory()`: ends a loop take without writing history, and flushes any open group as its own step.
  2. Pops the undo stack.
  3. Pushes the current state to redo.
  4. Restores.
  5. Rebases the still-open group.
- `performRedo()` is symmetric but uses `pushUndoFromRedo`, which keeps the redo stack.
- `restoreFromSnapshot(s)`:
  - Never writes history.
  - Ends a running take.
  - Disposes and rebuilds all nodes inside `withPlaybackRestart`.
  - Runs `migrateArrangement`.
- `withPlaybackRestart(fn)` (`AE:1616-1629`): ends any loop take. While playing it remembers the beat and the pattern flag, calls `stopTransport()`, runs `fn`, then `playFrom(beat)`.

**Beat mode**
- `getActiveDrumTrackId()`: a stale id falls back to the first drum track (`AE:2359-2365`).
- `setActiveDrumTrack(id)`: stops a loop take on a different track, then `syncDrumSink` (`AE:2367-2374`).
- `ensureDrumTrack()` (`AE:2377-2379`).
- `getKit(id)` (`AE:2408-2412`).
- `setKit(id, kit, source='user')`: undo 'Change Kit'. Normalises, applies, fires `onKitSelected` (N2, `AE:2420-2446`).
- `updatePad(id, pad, patch)`: undo 'Edit Pad', `normalizePad` (`AE:2452-2460`).
- `setPadSample(id, pad, sample|null)`: undo 'Assign Sample'/'Clear Pad'. A different source key resets `startSec=0, endSec=null, reverse=false` (`AE:2463-2479`).
- `applyKit` calls `sampler.setKit` (`AE:2481-2486`). `onKitChange` (`AE:2488-2497`).
- `getSelectedPad / setSelectedPad`: no undo (`AE:2501-2509`).
- `getLoopSettings / setLoopSettings(patch)` (no undo, `AE:2522-2547`):
  - Normalises the settings.
  - **Only a change of `bars`** moves the markers to `[patternStart, +bars*4)`, resizes a running take and grows the pattern region.
  - A swing or swingGrid change reschedules every drum track.
  - Then updates the metronome.
- `getPatternRegion(id)`: the MIDI region covering the pattern start; prefers the one with the latest start (`AE:2593-2604`).
- `ensurePatternRegion(id)`: undo 'Add Pattern' only if it created or grew a region. A new region is named 'Pattern' (`AE:2607-2640`).
- `toggleStep(id, pad, step, stepBeats, vel=0.8)`: undo 'Toggle Step'. Stored straight at `step*stepBeats`, duration = stepBeats. Step mapping uses `patternStepAt` (nearest step, clamped) (`AE:2643-2688`, `AH:86-89`).
- `setStepVelocity`: 'Step Velocity'. `clearPattern(id, pad?)`: 'Clear Pattern' (`AE:2691-2714`).
- `isLoopRecording / getCountInRemaining / startLoopRecording / stopLoopRecording` (`AE:2718-2808`, §2.6).

**Migration on every load / restore** (`AE:2816-2843`)
- Defaults pan/effects.
- `velocity > 1` becomes `min(1, v/127)`.
- Legacy GM drum notes are mapped to pads (unmapped notes dropped; colliding notes merged, louder wins; `AH:239-274`).
- Beat state normalised.
- Missing or invalid loop markers become `[0, bars*4)`, and the length grows to at least that.

### 1.2 LoopRecorder (`LR`)

Host interface: `LR:46-63`.

- `begin(take)`: resets state and subscribes to the transport `'loop'` event to record wraps (`LR:106-127`).
- `setCountInEnd(t)`: also starts a per-beat UI tick timer (`LR:130-134,201-223`).
- `setLoopBeats`, `markStructureChanged` (`LR:137-143`).
- `end()`: closes held gates, then resets; returns the take (`LR:149-156`).
- `abort()`: resets without closing anything (`LR:159-161`).
- `getCountInRemaining()`: `min(countInBeats, ceil(left/spb - 1e-6))` (`LR:191-198`).
- `consumeSuppressedHit(beat, note, time)`: one-shot suppression keyed by `note@round(beat*1000)` (`LR:23,230-237`).
- `captureHit` / `captureRelease`: §2.6–2.7.

Undo: the take's pre-start snapshot is pushed as **one** 'Loop Record' entry on the first hit (`LR:292-295`). A take with no hits still pushes it if the pattern region was created or grown (`AE:2805`). Ending a take from undo/redo/restore never pushes anything.

### 1.3 DrumEngine (`src/services/DrumEngine.ts`)

- `start()`: master bus, a `Volume(-6)` to destination, and a fallback `SamplerKit(factory kit)` (`:57-65`).
- `hit(pad, vel, time?)` (`:71-89`):
  - Velocity normalised.
  - Time clamped to ≥ now.
  - Goes to the sink, or the fallback kit if there is none.
  - Remembers the target per pad, plus any future `VoiceHandle`s it created.
- `release(pad)` (`:97-120`):
  - Cancels the pad's pending scheduled voices (all modes).
  - Releases on **every target that got a hit since the last release** (not just the current sink).
- `setSink(sink)`: `fallbackKit.stopAll()`, then `prev.releaseAllLive()` (`:147-157`).
- `hitByMidiNote` (GM → factory pad, `:139-144`), `setFallbackKit`, `getSink`.
- `preview(source)`: one preview at a time, token-guarded. ToneBufferSource with gain 0.9, linear fade-out 10 ms (`:28-29,174-212`).
- `mute`/`unmute`/`setVolume`/`dispose`.

### 1.4 NoteRepeat (`src/services/NoteRepeat.ts`)

- `setEnabled/toggle`, `setRate`: both restart every held pad.
- `setTrigger(fn(pad, vel, time))`.
- `press(pad, vel)`: called after the normal hit. Releases a previous hold of the same pad first.
- `release(pad)`, `releaseAll`, `isHeld`, `isRepeating`, `subscribe`, `dispose`.

Scheduling is in §2.8.

### 1.5 UndoManager (`src/services/UndoManager.ts`)

- `MAX_UNDO = 50`, oldest dropped (`:10,165-172`).
- `pushUndo`: inside a group it only marks dirty; otherwise commits and **clears redo** (`:32-38`).
- `beginUndoGroup(label, snap | fn)`: nests. Only the outermost call captures (`:54-59`).
- `endUndoGroup(isUnchanged?)`: the last end commits one entry unless `!dirty && isUnchanged` (`:68-78`).
- `flushGroup(current, isUnchanged)`: commits the group so far, keeps it open, re-snapshots (`:89-95`).
- `rebaseGroup` (`:114-118`).
- `pushRedo` (`:121-124`).
- `pushUndoFromRedo`: commit **without** clearing redo (`:131-133`).
- `undo`, `redo`, `canUndo/Redo`, `undoLabel/redoLabel`, `clear()` (also resets the group), `onStateChange`.

### 1.6 AudioEngine: live keyboard synth (`src/services/AudioEngine.ts`)

- `start()`: idempotent, promise-shared (`:54-60`). Builds the FX chain (§4.3), creates the instrument, starts loading piano samples in the background.
- `noteOn(midi, vel)` / `noteOff(midi)` at `immediate+4ms`. noteOff is honoured even while muted. `releaseAll()` (`:122-137`).
- `setPreset(i)` (`:174-179`):
  - Resets overrides.
  - Recreates the instrument.
  - The old one gets `releaseAll` and is disposed after 1200 ms (`RETIRE_MS`, `:20,103-112`).
- `setSynthParam(key | 'a.b', value)`: merged into overrides, applied live through `instrument.set` without rebuilding (`:186-190`).
- `getSynthOverrides / getCurrentPreset / getPresetIndex / getActiveVoiceCount / getHeldNotes / isPianoReady`.
- Global FX setters (`:210-372`):
  - Most ramp: 30 ms for wet/EQ/feedback, 20 ms for cutoff/Q, 50 ms for delayTime.
  - Compressor threshold/attack/release are set immediately.
  - Wet / amount / EQ / ratio setters call `rewire()`.
- `setVolume(db)`: 30 ms ramp. Default −12 dB (`constants.ts:17`).

### 1.7 ExportService (`src/services/ExportService.ts`): §6.

### 1.8 trackAudioFactory (`src/utils/trackAudioFactory.ts`)

- `createTrackAudioNodes(track, dest)` (`:42-150`).
- `compressorActive(enabled, ratio)` = `enabled && ratio > 1.001` (`:153-155`).
- `setTrackCompressorEnabled` rewires `eq → comp → chorus` vs `eq → chorus` (`:162-171`).
- `disposeTrackAudioNodes` (`:173-202`).
- `playDrumNote(sampler, note, vel, time, durSec?)` (`:209-222`):
  - Pad = note − 36 (36..51 only).
  - Triggers the pad.
  - **Gate pads only**: `release(pad, time + max(0.01, durSec))`.

---

## 2. Transport and timing semantics

### 2.1 Play / stop / seek

- **startPlayback(fromBeat)** (`AE:1141-1156`):
  1. Requires 'stopped'; sets 'playing'.
  2. `resetTransport()`, BPM set, `transport.seconds = beat*60/bpm` (placed *before* the start).
  3. `scheduleAllTracks()`, `applyLoopToTransport()`, metronome start if `shouldClick()`.
  4. `startTransportAt(beat)` at `Tone.now()` (now + 0.1 s), which records `transportStart = {time, beat}` (`AE:1189-1193`).
  5. Starts the RAF position loop.
- **resetTransport()** (`AE:1170-1186`):
  - If the transport is running: `stop()`.
  - Else if a start is still pending (`transportStart.time > immediate`, e.g. during count-in): `start(t); stop(t)` at the same `Tone.now()`.
    - Never call `stop()` on an already-stopped clock. In Tone that cancels the previous stop event and extrapolates the old run (B1 / R1).
  - Then `cancel()` all scheduled events.
- **stopTransport()** (`AE:1208-1248`):
  - Reset, `seconds = 0`, `transportStart = null`.
  - Every Part: `stop(0)` + dispose. Synths: `releaseAll()`. Samplers: `stopAll(immediate)` (5 ms fade, future voices cancelled). Audio players stopped and disposed.
  - Metronome stopped, count-in synth disposed (silences pending count-in clicks).
  - Position callbacks get 0.
  - `patternPlayback = false`; the Song loop is re-applied.
- **setPosition(beat)** (`AE:1250-1268`):
  - Playing, no loop take, and start still pending (N4): full `stopTransport()` + `startPlayback(beat)`, keeping the pattern flag.
  - Otherwise: `transport.seconds = beat*60/bpm`, and if a start is still pending, `transportStart.beat = beat`.
  - Stopped: it sets seconds, but `play()` resets to 0 anyway.
- **stop()**: during a loop take, `stopLoopRecording`; during linear recording, `stopRecordingToTrack`; else `stopTransport` (`AE:1195-1206`).
- **Auto-stop** (`AE:1471-1486`):
  - RAF loop.
  - Only in 'playing' with **no active loop range**, when `beat >= lengthBeats`, it calls `stop()`.
  - Never while recording.
- **Solo/mute at schedule time** (`AE:1338-1351`): a track that should not play gets **no Part or automation at all** (on top of the volume-node mute). Muting during playback mutes the volume node live but does not reschedule.

### 2.2 What gets scheduled (`AE:1338-1424`)

- For every track: `resetTrackEffects` (cancels automation curves, re-applies static FX, `AE:1794-1869`), then automation (§4.4).
- **Audio tracks**:
  - One `Tone.Player` per region (gain = `region.audio.gainDb`) into `audio.filter`.
  - Scheduled with `transport.schedule(start(time, offsetSeconds, regionDurSec), regionStartSec)`.
  - Known quirk: starting playback *inside* a region does not start it, because the event is in the past.
- **MIDI/drum tracks**: one `Tone.Part` with all notes, `part.start(0)`. The transport loop re-fires events each pass.
  - Event time = `playBeat*60/bpm`.
  - Drum `playBeat = beat + swingOffsetBeats(beat, swingGrid, swing)`, using the absolute beat `region.start + note.start` (`AE:1394-1400`).
  - Callback:
    - `secPerBeat` = the **transport BPM at the event time** (`AE:1427-1435`, R2#11).
    - Drums: skip if `consumeSuppressedHit`, else `playDrumNote(..., dur*spb)`.
    - Synths: `triggerAttackRelease(name, dur*spb, time, vel)`.
- **refreshPlaybackSchedule(trackId)** (`AE:2322-2341`):
  - Only while playing or in a loop take.
  - `part.stop(immediate)` + dispose, then rebuild that one track's Part.
  - Other tracks and position are untouched.
- **addToRunningPart**: incremental `part.add` for a plain new loop hit; falls back to a rebuild (`AE:1442-1452`).

### 2.3 Loop

- `applyLoopToTransport()` (`AE:1121-1132`): `transport.loop = !!range`, `loopStart/End = beats*60/bpm`.
- Called by:
  - `setLoopEnabled`, `setLoopMarkers`, `setBpm`, `startPlayback`, `stopTransport`.
  - `setLoopSettings` (only when bars change).
  - `applyPatternLoop`, `startLoopRecording`.
- **Pattern playback (`playPattern`)** (`AE:2558-2569`):
  - Requires 'stopped' and no take.
  - Grows `lengthBeats` to cover the pattern.
  - `patternPlayback = true`, `playFrom(patternStart)`.
  - Loops `[patternStart, +bars*4)` **transiently**: the stored markers and the loop toggle are untouched (U6).
  - `patternStart` = loop-marker start, or 0 (`AE:2572-2574`).

### 2.4 Metronome (`src/services/arrangement/Metronome.ts`)

- Click synth (`:15-21`):
  - Sine, envelope A 1 ms / D 50 ms / S 0 / R 10 ms, volume **−15 dB** (`CLICK_VOLUME_DB`, `:8`, U5).
  - Note length `'32n'`.
  - Accent C6 (1046.5 Hz), plain G5 (784 Hz).
  - Connected to Destination. The master bus sits on Destination, so the click does go through master volume + clipper (`masterBus.ts:5-8`).
- Transport click (`:44-75`):
  - `Tone.Loop('4n')` started at 0.
  - Accent when `round(ticks(time)/PPQ) % 4 == 0`. The accent comes from the transport position, so it stays correct across loop wraps.
  - Skips any time ≤ the last click time (tempo-change and loop-wrap safety).
- `shouldClick()` = `metronomeEnabled || ((loopRec || patternPlayback) && beat.loop.metronome)` (`AE:1291-1294`, R2#12).
- `updateMetronome()` stops it, or starts it if the transport is not stopped (`AE:1296-1299`).
- Linear Song recording only uses the global toggle (`AE:901`).

### 2.5 Count-in (loop recording, `AE:2766-2775`)

- Count-in beats = `countInBars*4` (`countInBars ∈ {0,1,2}`, default 1).
- `t0 = immediate + 0.05 s`.
- Count-in clicks are pre-scheduled on a **separate** click synth at `t0 + i*spb`, accent on `i%4==0` (`Metronome.ts:88-95`).
- Transport start = `countInEnd = t0 + beats*spb`.
- During the count-in, `transportBeatAt()` returns `patternStart`.
- UI tick: a `setTimeout((left % spb || spb)*1000 + 5)` timer emits a state change each beat (`LR:201-218`).
- Stop or abort during the count-in cancels the pending start (N1) and disposes the count-in synth.

### 2.6 Loop recording / quantize / swing

**Constants** (`AH:37-60`, `LR:16-21`)

| Name | Value |
|---|---|
| `STEP_EPSILON` | 1e-4 beats |
| `MAX_SWING` | 0.75 |
| `DEFAULT_STEP_VELOCITY` | 0.8 |
| `COUNT_IN_PRE_ROLL_BEATS` | 0.5 |
| `SUPPRESS_MARGIN_SEC` | 0.15 |
| `WRAP_KEEP_SEC` | 1 |
| `MIN_GATE_BEATS` | 1/16 beat (a 1/64 note) |

Default `LoopRecordSettings`: bars 2, quantize '1/16', swing 0, swingGrid '1/16', countInBars 1, metronome true, overdub true. Allowed values:
- bars ∈ {1,2,4,8}
- quantize ∈ {off, 1/4=1, 1/8=0.5, 1/8T=1/3, 1/16=0.25, 1/16T=1/6, 1/32=0.125} (beats)
- swingGrid ∈ {1/8, 1/16}

Old data with no `swingGrid` gets '1/8' if the old quantize was '1/8', else '1/16' (`AH:191-212`).

**Swing formula** (`swingOffsetBeats`, `AH:138-146`)

1. `s = clamp(swing, 0, 0.75)`. `step = 0.25` for a 1/16 swing grid, `0.5` for 1/8.
2. `idx = round(beat/step)`. If `|idx*step - beat| > 1e-4` (off-grid), or idx is even, the offset is 0.
3. Otherwise the offset is **`s × step` beats** (rounded to 1e-9).
4. Triplet steps (non-power-of-two) are never swung.

Applied to the **absolute** beat, drum tracks only, at playback time:
- Live: `AE:1397`
- WAV: `ExportService.ts:31-34,163`
- MIDI export: `ExportService.ts:217`

It is never stored, and synth tracks are never swung. Example: swing 0.5 on the 1/16 grid delays the "e" and "a" 16ths by 0.125 beat.

**quantizeBeat(beat, grid, swing, swingGrid)** (`AH:98-120`)
- 'off' returns the beat unchanged.
- Otherwise it searches step k in `k0 ± span`, where `span = ceil(0.75*swingStep/step)+1`.
- It minimises `|beat - (k*step + swingOffset(k*step))|`, i.e. by **where the step sounds**, and returns the **straight** `k*step`.

**captureHit(note, vel, isDrum, time?, hintBeats?)** (`LR:245-336`)
- Drum notes only (pad 36..51). Synth notes are ignored.
- Grids:
  - `quantStep` = grid step of the record quantize.
  - `fineGrid = hintBeats` if `hintBeats < quantStep - ε` (or quantize is off), else none.
- **During the count-in**:
  - A hit more than 0.5 beat before the end is ignored.
  - Otherwise `quantized = 0`, `ahead = countInLeft/spb`, `onBeat = patternStart - ahead`.
- **After the count-in**:
  1. `onBeat = transportBeatAt(at)`, `played = wrap(onBeat - patternStart, loopBeats)`.
  2. Snap on the absolute grid: `fineGrid ? round(abs/fine)*fine : quantizeBeat(abs, quantize, swing, swingGrid)`.
  3. `quantized = wrap(snapped - patternStart)`.
  4. `ahead = (quantized + swingOffset) - played`, folded into `±loopBeats/2`.
- First hit of the take: push the undo snapshot and `ensurePatternRegion`.
- If `!overdub` and the take has not replaced yet, delete every pad note in the pattern window (once per take).
- Note fields:
  - Velocity: `clamp01(v>1 ? v/127 : v)`.
  - `startBeat = (patternStart - region.start) + quantized`.
  - Duration: `fineGrid ?? quantStep ?? 0.25`.
- Merge: same pad within ε → the louder velocity wins (`AH:182-189`).
- Gate pads are added to `heldGates` with `onTime` and `onBeat`.
- If `ahead > 0` (the hit was snapped forward into this pass): suppress the pattern replay of that note until `at + ahead*spb + 0.15 s`.
- Playback: incremental add if this was a plain new note and nothing was grown or cleared; otherwise reschedule the track.

**startLoopRecording()** (`AE:2736-2790`):
- Stops any linear recording or playback.
- `ensureDrumTrack`, snapshot, `ensurePatternRegionOn`.
- `loopRecorder.begin`, state 'recording', transport positioned at `patternStart` and looped over the pattern.
- `scheduleAllTracks()`, metronome, count-in, position loop.
- Transactional: on an exception it aborts and rolls back the structure.

**endLoopTake(recordStructure)** (`AE:2801-2808`): see the undo rule in §1.2.

### 2.7 Gate length (loop recording)

`captureRelease(note, time?)` (`LR:347-371`):
1. `offBeat` = transport beat (or the virtual count-in beat).
2. `wrapped` = sum of `loopBeats` for each transport `'loop'` event with `onTime < w.time ≤ off`.
3. `loopGateBeats(on, off, wrapped, loop)` (`AH:161-168`):
   - `beats = off - on + wrapped`.
   - If negative, add whole loops.
   - Clamp to `[1/16, loopBeats]`.
4. Only the matching note (same pad, start within ε) is updated.

The wrap list is pruned to entries newer than `min(now-1s, oldest held onTime)` (`LR:179-183`, L-e). Musical time comes from ticks, so tempo changes during the hold are exact (R2#5, E1).

One-shot pads keep their fixed length. `end()` closes held gates at the end time.

### 2.8 Note repeat scheduling (`NoteRepeat.ts`)

- Rates (beats): 1/4=1, 1/8=½, 1/8T=⅓, 1/16=¼, 1/16T=⅙, 1/32=⅛. Default 1/16 (`:23-34`).
- The velocity of every repeat is the initial hit's velocity.
- **Transport running** (state ≠ stopped, including count-in; `startOnTransport`, `:246-261`):
  - `interval = rateBeats × PPQ` ticks.
  - `pressTick = ticks(immediate)` (what is heard). `scheduledTick = transport.ticks` (the lookahead position).
  - `startTick = nextGridTick(pressTick + interval/2)`: the first grid line at least half a step after the press (`:55-58,77-82`).
  - `transport.scheduleRepeat(cb, "<interval>i", 0)`: the grid is anchored at tick 0, so it survives loop wraps.
  - Callbacks with `time < firstTime - 1 ms` are ignored.
  - **Catch-up**: grid lines from `startTick` up to `scheduledTick` (max 64) fire immediately at `pressTime + (tick - pressTick)*secPerTick`.
- **Transport stopped** (free-running, `:268-305`):
  - `step = 60/bpm × rateBeats` (bpm ≤ 0 → 120).
  - Grid reference: another held pad's `freeNext` if one exists, else the press time.
  - `freeNext = ref + ceil((press + step/2 - ref)/step)*step`.
  - Pump every **15 ms** (`setInterval`). Hits are handed out within a **40 ms** lookahead.
  - A step more than **30 ms** late is skipped. A less-late step plays at `max(t, now)`.
- `restartAll` on enable, rate change, and transport running↔stopped transitions (`watchClock`, `:331-340`).
- **Release path** (`useMidiNotes.ts:209-215`), in order:
  1. `noteRepeat.release`.
  2. `settlePending(pad)`: repeat hits whose `time ≤ immediate` are recorded/flashed now; later ones are dropped (`:148-158`).
  3. `drumEngine.release` cancels the pad's not-yet-started voices.
  4. `SamplerKit.release` also cancels gate voices that start after the release time.
  5. `captureNoteOff`.
- Scheduled hits are recorded at their audio time with `gridBeats = repeatStepBeats(rate)` (`useMidiNotes.ts:166-192`).

### 2.9 Linear-record and loop constants (summary)

| Item | Value |
|---|---|
| Tone lookAhead (Transport / `Tone.now`) | 0.1 s (Tone default, not overridden) |
| Live synth offset | 4 ms |
| Live pad hit | immediate (0 ms) |
| Count-in start margin | 50 ms |
| Punch loop-close callback | 50 ms before loop end |
| Free note-repeat lookahead / tick / late skip | 40 ms / 15 ms / 30 ms |
| Suppress margin | 150 ms |
| Linear-record drum note length | 0.25 beat |
| Linear-record synth note minimum | 0.125 beat |
| Click | sine, −15 dB, A 1 ms / D 50 ms, C6 accent / G5 |

---

## 3. SamplerKit voice semantics (`src/audio/sampler/SamplerKit.ts`)

**Pad model and defaults.** Types: `src/types/sampler.ts`. Defaults: `factoryKit.ts:146-160`. `normalizePad` clamps every field (`factoryKit.ts:225-248`).

| Field | Range | Default |
|---|---|---|
| gainDb | −60..12 | 0 |
| pan | −1..1 | 0 |
| pitch | −24..24 semitones | 0 |
| startSec | ≥0 | 0 |
| endSec | > start, or null | null |
| decay | 0.005..4 s | 0.05 |
| filterCutoff | 20..20000 Hz | 20000 |
| reverse | bool | false |
| mode | oneshot / gate | oneshot |
| chokeGroup | int ≥0 | 0 |

**Per-pad signal chain** (`:176-186,383-414`):
- `voice.source → voice.env (Gain) → pad amp (Gain = dB→lin(gainDb)) → [LP filter] → Panner(pan) → kit output`.
- Gain and pan changes ramp over 20 ms (immediate at build).
- Filter: lowpass, **−12 dB/oct, Q 0.707**. Bypassed (removed from the chain) when cutoff ≥ 20000.

**Slice / pitch / reverse** (`computeSlice`, `:74-89`):
- `start = clamp(startSec, 0, dur)`, `end = endSec ?? dur`.
- Silent (no trigger) if the length is ≤ 0.5 ms.
- `rate = 2^(pitch/12)`: plain playback-rate pitch, no time-stretch.
- `realDuration = length/rate`.
- Reverse plays a cached reversed copy of the buffer (`bufferCache.ts:278-289`) from `offset = dur - end`.

**Velocity** (`:246-267`, `common.ts:17-28`):
- Normalise: undefined → 0.8; >1 → /127; clamp 0..1.
- `vel ≤ 0` → no trigger.
- `gain = 0.15 + 0.85 × vel^1.7` (sensitivity 0.85).

**Envelope** (`voiceEnvelope`, `:54-68,265-299`):
- Fade-in 2 ms only if `slice.offset > 0`.
- Hold at gain.
- Linear ramp to 0 over the last `fadeOut = min(decay, realDuration)` seconds, ending at the slice end.
- `source.start(time, offset)` and `source.stop(end)`, called once only.

**One-shot.** Always plays the whole slice. Note-off is ignored.

**Gate.** `release(pad, t)` (`:317-330`) acts only on gate voices:
- Voices starting after `t` are cancelled.
- Voices already in their final fade are skipped.
- Otherwise: `cancelAndHoldAtTime(t)`, then a linear ramp to 0 over `min(decay, end - t)`.

Arrangement playback releases gate pads at `noteStart + max(0.01, noteDurSec)`.

**Choke** (`:251-255`):
- `chokeGroup > 0`: every *other* pad in the same group fades over **5 ms** (`CHOKE_FADE`) at the hit time.
- Only voices started ≤ that time are affected. Future voices are left alone.
- A pad never chokes itself.
- The factory kit and every classic machine put the hats in group 1.

**Voice limit** (`:257-263`):
- Max 8 voices per pad.
- The oldest is stolen: cancelled if it has not started, else a 5 ms fade at the new hit time.
- Retriggering the same pad stacks voices (no self-cut).

**stopVoice** (`:466-481`): `cancelAndHoldAtTime` + linear ramp, so a voice never jumps back to full level. The end/stopAt times are updated.

**stopAll(time = now)**: 5 ms fade on every voice, future voices cancelled (`:333-336`).

**setKit** (`:212-226`):
- A pad whose sample key changed is stopped (5 ms fade, including future voices) and reloaded.
- A stale buffer key (pack generation) is reloaded.
- The channel is re-applied with ramps.

**Pruning** uses `context.currentTime` (what is heard now), never an event time (`:423-430`, R2#9).

**Offline renders** never dispose voices or prune (the offline clock sits at 0). Cancelled voices are muted with gain 0 instead (`:160-166,485-519`).

**VoiceHandle / collectVoices** (`:96-122`): synchronous capture of the voices a `trigger` created, with `cancelIfPending()` (start > currentTime).

**Buffers** (`bufferCache.ts`):
- Shared cache keyed by source:
  - `factory:<id>`
  - `classic:<m>:<v>`
  - `pack:<id>@<gen>:<file>`
  - `file:<path>` or `file@<gen>:<path>` (`:65-78`)
- Pack invalidation bumps the generation, drops cached buffers, reloads SampleManager samples **in place**, and notifies samplers (`:95-115`).
- Preview LRU holds 8 entries (`:194`).
- Missing pack → `MissingPackSampleError`.
- Hits before a buffer has loaded are silent.

---

## 4. Instruments, presets, FX, automation

### 4.1 Preset bank (`src/audio/presets.ts`)

The index is the persisted key (`presetIndex`). Indices 0–35 are legacy slots; new sounds start at 36. Overrides are deep-merged with type checks and legacy key translation (`presetParams.ts:15-115`). Locked keys: id, name, engine, category, vibe, description, sample.

**Engine defaults**

| Engine | Defaults |
|---|---|
| subtractive | poly 12, oscB off, filter LP −24 cutoff 2000 Q 1 keyTrack .5 env 2 oct vel .5, fEnv .005/.4/.3/.4, amp .004/.3/.8/.3 vel .7, LFO 5 Hz (`:65-94`) |
| fm | poly 16, ratio 1, index 2, velToIndex .7, keyScale .5, modEnv .001/.8/.2/.4, amp .002/2/0/.4, ping off (`:105-123`) |
| pluck | poly 8, attackNoise 1, dampening 4000, resonance .97, release 1, velocity .7 (`:125-138`) |
| sampler | poly 48, release .45, velocityCurve 1.6 (`:140-151`) |

Character defaults: drive 0, chorus 0, tremolo 0 @ 4.5 Hz, tone 20000, presence 0.

**Preset table**

| # | id | engine | poly | gain dB | notes |
|---|---|---|---|---|---|
| 0 | grand-piano | sampler | 48 | 3.5 | rel .5, curve 1.5, tone 16k |
| 1 | saw-lead | sub | 8 | 2.5 | unison 3 / 14 ct, sub sq .25, vib 12 ct delay .45 |
| 2 | fm-keys | fm | 16 | −2.5 | idx 3.2, ping ×14, chorus .3 |
| 3 | warm-pad | sub | 10 | 2 | cutoff 850, LFO filter 300 ct @ .25 Hz |
| 4 | sync-lead | sub | 8 | −2 | Q 9, −12 dB filter, drive .35 |
| 5 | synth-pluck | sub | 12 | 5 | cutoff 480, env 4 oct |
| 6 | supersaw-pad | sub | 8 | 1 | unison 7 / 36 ct |
| 7 | analog-bass | sub | 8 | −2 | sub .6 |
| 8 | sub-bass | sub | 8 | −5 | |
| 9 | drawbar-organ | sub | 16 | 4 | partials, noise click .006, tremolo .18 |
| 10 | square-lead | sub | 8 | −2.5 | |
| 11 | tubular-bell | fm | 16 | −2 | ratio 3.5 |
| 12 | string-ensemble | sub | 10 | 1 | pulse oscB width .3 |
| 13 | rhodes | fm | 16 | 6 | tremolo .35 |
| 14 | synth-brass | sub | 10 | 1.5 | |
| 15 | bright-piano | sampler | 48 | 3 | presence +5 |
| 16 | marimba | fm | 16 | 1.5 | ratio 4 |
| 17 | music-box | fm | 16 | −1 | ratio 7 |
| 18 | vibraphone | fm | 16 | 3.5 | tremolo .45 @ 5.2 |
| 19 | fm-bass | fm | 4 | −3.5 | |
| 20 | acid-bass | sub | 8 | −2.5 | Q 10 |
| 21 | pulse-lead | sub | 6 | −10 | |
| 22 | theremin | sub | 8 | −8.5 | vib 28 ct |
| 23 | airy-pad | sub | 10 | 3.5 | pink noise |
| 24 | reese-bass | sub | **mono** legato | −1 | glide .03 |
| 25 | pluck-bass | sub | **mono** no legato | 0.5 | |
| 26 | mono-lead | sub | **mono** legato | −1 | glide .04 |
| 27 | glide-lead | sub | **mono** legato | −8.5 | glide .12 |
| 28 | nylon-guitar | pluck | 8 | −2.5 | |
| 29 | harp | pluck | 8 | −1.5 | |
| 30 | kalimba | fm | 16 | 0.5 | |
| 31 | timpani | sub | 6 | 0.5 | pitchEnv 1 st / .12 s, brown noise |
| 32 | steel-drum | fm | 16 | 1 | |
| 33 | wobble-bass | sub | 8 | −1 | LFO filter 3000 ct @ 2.5 Hz |
| 34 | sweep-pad | sub | 8 | −0.5 | fEnv attack 3.5 s |
| 35 | laser-zap | sub | 12 | −5 | pitchEnv 24 st, env −2 oct |
| 36 | wurli | fm | 16 | 6 | |
| 37 | rock-organ | sub | 16 | 5.5 | |
| 38 | trance-lead | sub | 6 | −1.5 | |
| 39 | brass-section | sub | 10 | 3.5 | |
| 40 | glass-pad | fm | 10 | −7 | |
| – | FALLBACK_PIANO | fm | 16 | −11 | used until the Salamander samples decode (`:584-591`) |

Full parameter values: `presets.ts:162-577`. Port them verbatim. Factory: `src/utils/synthFactory.ts` (engine → PolyInstrument / SampledPiano; an unknown engine falls back to preset 0; `presetAt` truncates out-of-range indices to 0).

### 4.2 Voice DSP (for the Rust synth)

**PolyInstrument** (`src/audio/PolyInstrument.ts`)
- Allocator (`voiceAllocator.ts`):
  - A re-struck note reuses its voice.
  - Otherwise: free voice (least-recently-finished), then grow the pool, then steal.
  - Steal order: a releasing voice (released longest ago), else the oldest held voice, sparing the lowest note.
- A stolen voice gets `hardStop` (15 ms fade); the new attack starts at `+9 ms` (`STEAL_GAP`, `:14,100-106`).
- Mono engines use `MonoNoteStack`: last-note priority, legato glide, and releasing back to a still-held key always glides (`voiceAllocator.ts:242-295`).
- One LFO per instrument: −1..1 at `clamp(rate, .05, 30)`. Vibrato cents → osc detune; filter cents → filter detune. Delayed vibrato ramps 0→depth over `lfo.delay` when playing from silence (`:65-72,253-260`).
- `triggerAttackRelease` duration ≥ 24 ms. `MIN_GATE` = 12 ms (`common.ts:9-11`).

**SubtractiveVoice** (`voices/SubtractiveVoice.ts`)
- Sources: oscA + oscB (Tone OmniOscillator: fat-stack of `unison` voices at `spread` cents; `custom` partials; pulse width clamp ±.95, default .4), sub osc (detune −1200/−2400), and noise.
- Mixer normalisation `1/max(1, Σlevels)`; sustained noise is counted at level×0.45 (`:239-255`).
- Filter (Tone.Filter, rolloff −12/−24, Q clamp .1..30):
  - Cutoff = `cutoff × 2^(keyTrack(midi−60)/12) × 2^(fvel(vel−.75)·1.5)`, clamped 20..18000 (`:257-262`).
  - Filter envelope in cents into detune. Depth octaves = `envAmount × (1−fvel+fvel·vel)`.
    - Negative: `max(−8, oct) × 1200`.
    - Positive: `min(oct, log2(20000/base)) × 1200` (`:264-271`).
- Amp: ADSR (attack ≥ 1 ms, release ≥ 5 ms). Peak = `0.15`-style velocityGain with `amp.velocity` sensitivity.
- Pitch envelope: starts at `f·2^(amt/12)`, exponential ramp to f over `decay`.
- Glide: exponential ramp to the new frequency over `glide`; legato also ramps the cutoff (`:111-143`).
- Noise envelope:
  - Sustained: A 1 ms, S 1, R = amp release.
  - One-shot: A 1 ms, D = decay, S 0.
- Sources stop at release end + 50 ms.

**FMVoice** (`voices/FMVoice.ts:73-96`)
- Modulator frequency = `f·ratio` (ratio ≥ .01).
- Depth (Hz) = `index·velIdx·keyIdx·f·ratio`, where:
  - `velIdx = 1−v2i+v2i·vel`
  - `keyIdx = clamp(2^(−keyScale(midi−60)/24), .15, 2)`
- Depth × modEnv (0..1) is added to the carrier frequency.
- Ping: sine at `min(f·max(.5, pingRatio), 18000)`, env A 1 ms / D d / S 0 / R min(d, .2), level `velocityGain(vel, .6)`, volume = ping.level.
- Carrier volume `1/(1+.5·ping)`, carrier detune in cents.
- Legato glide: 30 ms exponential ramp.

**PluckVoice** (`voices/PluckVoice.ts`)
- Tone.PluckSynth (Karplus-Strong). Dampening = `clamp(d·2^(velocity(v−.7)·2), 300, 16000)`, resonance ≤ .999.
- Output gain = `velocityGain(v, vel) × 3.5`.
- No glide.

**SampledPiano** (`src/audio/SampledPiano.ts`, `pianoSamples.ts`)
- Tone.Sampler over 30 Salamander MP3s, one every minor third from A0 to C8. Nearest sample is repitched.
- Leading silence trimmed (threshold 0.002, keeps 1 ms).
- Makeup ×2 (linear).
- Velocity → gain `0.06 + 0.94·v^curve` (curve clamp .5..3). Release clamp .05..5, exponential curve.
- Allocator with max(8, poly). A re-strike or steal releases the old sample.
- Before the samples decode, notes go to the FM fallback piano, and their note-offs are routed back to it.

**CharacterChain** (`src/audio/CharacterChain.ts`)
- `input → [drive] → [presence] → [tone] → [tremolo] → [chorus] → Volume(gain dB)`.
- A stage is present only when it does something.
- drive (> .001): `k = 1+9·drive`, curve `tanh(kx)·0.35/tanh(0.35k)`, 2048 points, 2× oversampled.
- presence (|p| > .05): high-shelf 3.2 kHz, gain dB.
- tone (< 19 kHz): LP −12, Q .3, clamp 200..20k.
- tremolo: depth, rate, spread 140.
- chorus: 0.6 Hz, 3.2 ms, depth `.25+.45a`, spread 180, wet `.2+.35a`.

### 4.3 Effect chains

**Per track** (`trackAudioFactory.ts:37-111`):

`instrument / sampler / players → Filter → Distortion(2× oversample) → EQ3 → [Compressor] → Chorus → FeedbackDelay → RoomReverb → Panner → Volume → Meter → Destination`

| Effect | Defaults (`effects.ts:73-81`) | Enabled | Disabled |
|---|---|---|---|
| filter | LP, cutoff 18000, Q 1, LFO 1 Hz depth 0 sine | `type`, `cutoff`, `Q=resonance`; LFO if depth > 0, range `[max(60, c(1−.75d)), min(18000, c(1+.75d))]` | 18000 Hz, Q 1 |
| distortion | amount .3, wet .5 | amount, wet | 0 / 0 |
| eq (Tone EQ3, bands at Tone defaults 400 / 2500 Hz) | 0/0/0 dB | low/mid/high | 0 |
| compressor (Tone default knee) | −24 dB, 4:1, 3 ms / 250 ms | threshold/ratio; inline only if ratio > 1.001 | threshold 0, ratio 1, **bypassed** |
| chorus (0.7 Hz, 3.5 ms, spread 180) | depth .5 | depth, **wet fixed 0.5** | depth 0, wet 0 |
| delay (FeedbackDelay) | time .25, fb .3, wet .3 | wet | wet 0 (time/fb still set) |
| reverb (RoomReverb defaults) | wet .25 | wet | 0 |

Track volume default −6 dB (set at `addTrack`); pan −1..1.

Quirks to decide on:
- `resetTrackEffects` uses `fx.delay.time || 0.25` and `fx.delay.feedback || 0.3`, so feedback 0 becomes 0.3 at every play start (`AE:1822-1823`).
- On reset, an existing LFO's min/max are not updated (`AE:1836-1854`).

**Live synth (AudioEngine)**:

`instrument → Gain → [Distortion] → Filter → [EQ3] → [Compressor knee 6] → [Chorus] → [PingPongDelay] → [RoomReverb(decay 1.8, pre .022, damp .55)] → Volume(−12) → Destination`

- Stage on when (`AudioEngine.ts:388-418`):
  - dist: wet > 0 and amount > 0
  - eq: any band ≠ 0
  - comp: ratio > 1.01
  - chorus: depth > 0
  - delay: wet > 0
  - reverb: wet > 0
- Chorus mapping: depth `amount·0.6`, wet `.15+.35·amount` (`:457-463`).
- Defaults (`effects.ts:32-53`): reverbWet .12, cutoff 18000, Q 1, delay .25 s / fb .3 / wet 0, compressor −24 dB ratio 1 (off), attack 3 ms, release 250 ms.
- Parameter ranges (comments, `effects.ts:4-25`):
  - cutoff 60..18000 Hz, Q 0..20, LFO 0.1..20 Hz
  - delay 0..1 s, all wets 0..1
  - EQ ±12 dB, threshold −60..0 dB, ratio 1..20, attack/release 0..1 s
- MIDI CC map (`constants.ts:26-38`): CC 1/70 cutoff, 2/71 resonance, 3/72 reverb, 4/73 chorus. Volume CCs 5 and 74.

**RoomReverb** (`src/audio/RoomReverb.ts`)
- IR: stereo, `len = pre + decay·sr`, decay clamp .2..8, pre clamp 0..0.2.
- Per channel: xorshift noise (seeds `0x2545f491` / `0x9e3779b9`), one-pole LP with coefficient `a = exp(−4.5·damp·t/decay)·.9+.05`, envelope `exp(−6.91t/decay)`, 4 ms fade-in.
- Early-reflection taps:
  - L: 7, 13, 21, 34, 47 ms
  - R: 9, 17, 26, 39, 53 ms
  - Gains `.55−.09k`, alternating sign.
- Send: HP 200 Hz (−12) into a convolver (normalize = true). Equal-power crossfade dry/wet.

**Master bus** (`src/audio/masterBus.ts`), installed on Destination, so everything passes through it:
- `Volume(master)`, where master gain = `fader − (−12)`. Fader range −40..0 dB, default −12, persisted in localStorage `micro-daw:masterVolume`, 30 ms ramp.
- Then `Gain(10^(2/20)/2)`, then `WaveShaper(softClip(u·2))`, 4096 points, no oversampling.
- softClip: linear up to 0.85, above that `0.85 + 0.13·tanh((|x|−.85)/.13)`, ceiling 0.98.
- No compressor anywhere on the bus.

**DrumEngine fallback kit**: Volume −6 dB → Destination.

### 4.4 Automation (`AE:1871-1962`, `automationHelpers.ts`)

**Parameters** (`types/arrangement.ts:100`) and the mapping from the stored 0..1 value:

| Parameter | Range |
|---|---|
| volume | −40..0 dB |
| pan | −1..1 |
| filterCutoff | 60..18000 Hz, linear in Hz |
| eqLow / eqMid / eqHigh | −12..12 dB |
| reverbWet, delayWet, chorusDepth, distortionWet | 0..1 |

**Playback**
- Each point is a `transport.schedule` at `beat·60/bpm`.
- It does `setValueAtTime(v)`, then `linearRampToValueAtTime(next, time + Δbeats·60/bpm)`, so the ramp duration is in seconds at the BPM when scheduling happened.
- `chorusDepth` is set immediately inside the callback (not sample-accurate); wet becomes .5 if > 0.
- Lanes with a `distortionWet` lane force distortion amount ≥ 0.4. `delayWet` lanes force default time/feedback.
- Only visible lanes with points are scheduled.
- **WAV export does not render automation** (`ExportService.ts` never calls it).

Value interpolation for the UI: `getAutomationValue` (linear between points; empty lane = 0.5).

---

## 5. Factory and classic drum voices

### 5.1 Factory kit

Pad layout (`factoryKit.ts:65-68`):
- Pads 0–7: kick, snare, chh, ohh, clap, tom-mid, crash, ride
- Pads 8–15: kick-sub, rim, phh, shaker, snap, tom-lo, tom-hi, perc (cowbell)

Hats are in choke group 1 (`:71`). GM export notes and GM→voice mapping: `:94-136`.

**Rendering** (`bufferCache.ts:349-404`):
- `Tone.Offline`, **1 channel**, at the **current context sample rate**, velocity 1.
- Length per `FACTORY_VOICE_RENDER` (`drumKit.ts:36-53`): kick 1, kick-sub 2, snare .7, rim .5, clap .8, chh .35, phh .4, ohh 1.2, toms 1.4 / 1.2 / 1.1, crash 2.6, ride 2, shaker .4, perc .7, snap .5 s.
- Renders are serialised through a queue.
- `trimTail`: cut after the last sample > 0.001 (+10 ms), min 10 ms, 5 ms linear fade-out, scale to peak ≤ 0.98.

**DrumKit synthesis** (`src/audio/drumKit.ts`). Shared parts:
- White noise.
- "Metal" bank: 6 squares at 205.3 / 304.4 / 369.6 / 522.7 / 540 / 800 Hz, mixed at 1/6.
- `pulse(peak, tau, hold=2 ms, attack=.8 ms)` = linear rise to peak, hold, then exponential decay `setTargetAtTime(0, tau)` (`:66-70`).
- `v = velocityGain(vel, .85)`.

| Voice | Algorithm |
|---|---|
| kick | Sine 190→56 Hz (exp, 45 ms) →46 Hz (350 ms), stop at .9 s. Amp pulse .8v τ .1 hold 30 ms. Click: noise BP 3200 Hz Q .8, pulse .45v τ 4 ms hold 1 ms. Bus → `tanh(1.6x)/tanh(1.6)` |
| kick-sub | 140→52 Hz (70 ms) →41 Hz (1.2 s), stop at 2 s. Pulse .85v τ .4 hold 60 ms. Click .2v |
| snare | Triangle 185 + sine 330, each starting at 1.35×f and ramping exp to f in 30 ms. Body pulse .75v τ 45 ms. Noise HP 1400 → LP (5000+4000·vel), pulse .85v τ 75 ms hold 6 ms. Bus `tanh(1.4x)` shaper |
| rim | Snare with v×.7 and noise ×.35 |
| clap | Noise BP 1150 Q 1.5. Bursts at 0/11/22 ms: set 2.6v, then target .02 τ 3 ms. At 33 ms: set 2.8v, target 0 τ 70 ms |
| chh / phh | Metal → BP 10 kHz Q .9 → HP 7 kHz −24, plus noise HP 8.5 kHz ×.3. Closed amp pulse 1.5v (phh .9v) τ 16 ms. **Chokes the open amp** (10 ms ramp) |
| ohh | Same source, open amp pulse 1.25v τ .14 hold 10 ms |
| toms | Sine 1.6f → f (50 ms) → .9f (500 ms), stop at 1.2 s. τ = clamp(.2√(118/f), .12, .3), pulse .62v hold 10 ms. Noise BP 5f, pulse .25v τ 15 ms. f = 82 / 118 / 158 Hz (lo / mid / hi) |
| crash | Metal BP 5200 Q .35 + noise HP 4500. Ramp to .42v in 3 ms → target .19v τ 60 ms → from 150 ms target 0 τ .6 |
| ride | Metal BP 5000 Q 2.5 + noise HP 7500 ×.25. Pulse 1.05v τ .28 |
| shaker | Noise HP 5200 → BP 8500 Q .7. Pulse 1.1v τ 30 ms, hold 6 ms, attack 12 ms |
| perc (cowbell) | Squares 587 + 845 → BP 1200 Q .9. Ramp .7v in 1 ms → target .25v τ 12 ms → from 30 ms target 0 τ 90 ms |
| snap | Noise BP 2400 Q 2.5. Set 2.2v → target .1v τ 2 ms. At 7 ms set 3v → target 0 τ 30 ms |

Tone.Filter defaults where not given: Q 1, rolloff −12.

### 5.2 Classic machines (`src/audio/sampler/classic/*`)

The DSP is pure, deterministic and Tone-free. **Port `dsp.ts` (349 lines) and `voices.ts` (622 lines) literally** and use `classic.test.ts` / `render.test.ts` as golden-vector tests.

**Rendering** (`synth.ts`, `dsp.ts:8`, `render.ts`):
- **44.1 kHz mono**, independent of the device rate.
- `finalize` (`dsp.ts:334-349`): DC block at 10 Hz, normalise to **−1 dBFS**, trim after the last sample > 0.001 (+10 ms), cap at the recipe's `maxSec`, 6 ms fade.
- Runs in a Web Worker, falling back to the main thread after a `setTimeout(0)` yield (E2).

**DSP primitives** (`dsp.ts`):

| Primitive | Behaviour |
|---|---|
| `makeRng` | mulberry32 |
| `noise(n, seed)` | white in [−1, 1) |
| `env` | linear attack (≥ 1 sample), hold, `exp(−ln1000·t/t60)`, optional delay |
| `envSum` | sum of envelopes |
| `clapEnv` | bursts + delayed tail |
| `osc` | sine/triangle with two exponential sweep terms and a drift factor; phase in cycles |
| `square` | polyBLEP square |
| `metal(freqs, seed)` | squares with seeded random phases, each at 1/N |
| `partials` | `[f, a, t60]` sines at phase `i·.137` |
| `biquad` | RBJ lp / hp / bp (constant 0 dB peak) / peaking. Default Q = √½, f clamped to .49·SR |
| `drive(x)` | `tanh(a·x/peak)/tanh(a)·peak` |
| `lofi` | 2× LP at .45·rate when rate < SR; sample-and-hold; quantise to `bits` (optionally μ-law 255) at full scale; LP at min(.48·rate, .45·SR) |
| `mixInto`, `mul` | `mul` zero-fills the tail |

**Machines and voices** (metadata `machines.ts:37-219`):
- Default pad gains: hats −3 dB with choke 1; others as listed.
- `createClassicKit` (`kit.ts`): pad names are bare labels; all pads one-shot; gains and choke from the metadata.

| Machine | Pads 0–15 | Character (recipes in `voices.ts`) |
|---|---|---|
| tr808 | kick snare chh ohh clap tom-mid cymbal cowbell / kick-long rim maracas clave tom-lo tom-hi conga-mid conga-hi (also conga-lo) | Metal = 205.3…800 Hz. Kick: 50 Hz sine with sweep .55/6 ms + .12/60 ms, t60 1.15, 1400 Hz click .12, drive 1.2, LP 5k. Kick-long: 46 Hz, drift .94, t60 2.6, drive 2.2, LP 1.8k. Snare: 238 + 476 Hz bodies + noise HP 1.8k / LP 11k. Hats: metal BP 10k Q1 → HP 7k, t60 .09 / .75. Cymbal: two metal bands. Cowbell 540/800 → BP 700. Toms 85/125/170, congas 170/250/370, clave 2500 |
| tr909 | kick snare chh ohh clap tom-mid crash ride / kick-long rim phh snare-tight kick-hard tom-lo tom-hi snare-low | Metal = 263…845 Hz. Kicks: steep sweep (3.5×/8 ms), noise and 1200 Hz click, drive 1.8–3.5. Snares: two bodies + noise, drive 1.3. Hats: metal + noise → **6-bit** lofi → HP 5k → VCA envelope. Crash/ride: 6-bit. Toms 95/135/190 |
| tr707 | kick snare chh ohh clap tom-mid crash ride / kick-2 rim snare-2 tambourine cowbell tom-lo tom-hi ride-bell | Everything 8-bit at 25 kHz (hats/tambourine 32 kHz, cymbals 30 kHz). Hold + fast-decay envelopes. Metal 312…921 Hz |
| tr606 | kick snare chh ohh clap tom-mid cymbal phh / kick-long rim snare-snappy shaker zap kick-short tom-lo tom-hi | Thin: kicks 64/58/70 Hz with HP 35 / LP 4k. Hats: metal 245…619 → BP 11k → HP 8k, very short. Zap: 180 Hz sine with sweep 8× |
| linn | kick snare chh ohh clap tom-mid crash ride / sidestick cabasa tambourine cowbell tom-lo tom-hi conga-lo conga-hi | Everything **μ-law 8-bit at 28 kHz**. Membrane = fundamental + 1.59× overtone + head noise. Clap has a +4 dB peak at 2.5k. Metal 327…1203 Hz |
| cr78 | kick snare chh ohh clave conga-lo cymbal cowbell / rim maracas guiro tambourine metal bongo-lo bongo-hi conga-hi | Soft attacks (1–3 ms), no lofi. Guiro: 10 accelerating scrapes. Metal beat: 5 partials 1250–5100 Hz. Hand drums: f + 1.6f |

MIDI export uses each classic voice's `gm` note (`AH:280-287`).

### 5.3 Slicer (`src/audio/sampler/slicer.ts`): chop-to-pads helper

- `detectTransients`:
  - 5 ms hop (≥ 32 samples), window = 2 hops, frame energy in dB.
  - Rise threshold `14 − 11·sensitivity` dB above the minimum of the previous 3 frames.
  - Gate `max − (24 + 36·sensitivity)` dB.
  - Local maximum over ±2 frames.
  - Onset refined to the first sample ≥ 10% of the local peak.
  - Strongest first, minimum gap 50 ms, up to 16 slices, always including 0.
- `equalSlices`.
- `slicesToRanges`: end = next slice start.

---

## 6. Export

**WAV** (`ExportService.ts:100-188`, `utils/wavEncoder.ts`)
- Throws "Nothing to export" if the end time is ≤ 0.
- Before rendering: loads piano samples if any unmuted sampler preset is used; preloads every unmuted drum kit's buffers.
- Render length = `getArrangementEndSeconds + 3 s` (`EXPORT_TAIL_SECONDS`). Per unmuted track, the latest of:
  - audio region ends
  - synth note ends
  - drum notes: swung start + audible length (one-shot: whole slice; gate: `min(slice, note + decay)`; unknown buffer: note length)
  - plus the delay tail: `time × max(1, ln(1e-3)/ln(fb))`, capped at 12 s (`:41-96`)
- `Tone.Offline`: **2 channels** (Tone default) at the **current context sample rate**.
- Master bus installed with fader 0 dB (no user master volume; drive + clipper still applied).
- Fixed BPM throughout. Song loop ignored (linear render from 0). **Solo ignored; only `muted` skips a track.** **No automation.**
- Same track chain as live. Drums use `playDrumNote` with swing; synths use `triggerAttackRelease`.
- Encoding: RIFF PCM **16-bit**, interleaved, clamped to ±1, `neg×0x8000 / pos×0x7fff`, no dither, 44-byte header.

**MIDI** (`ExportService.ts:192-259`, midi-writer-js)
- One track per unmuted non-audio track: tempo + track name.
- Channel 10 for drums, 1 otherwise.
- Ticks: **128 per beat**. Start = `round(beat·128)`. Duration `T max(1, round(dur·128))`.
- Velocity `clamp(round(v·100), 1, 100)` (midi-writer's percentage scale).
- Drums: swung beat, pad → GM note of the factory/classic voice, else the pad note 36..51.
- Notes sorted by start. Throws if no tracks.

---

## 7. Bug fixes from `docs/review-round-2..5.md` and where they live now

| Review | Fix | Code location (preserve in Rust) |
|---|---|---|
| R2-1 | Beat Play loops the pattern range; projects without markers get `[0, bars*4)` | `AE:2558-2569` (playPattern), `AE:2836-2841` (migrate) |
| R2-2 | Undo/redo during count-in or mid-take: end the take without history first; flush an open gesture group as its own step; restore never pushes | `AE:1637-1663`, `AE:1590-1591`, `AE:2801-2808`; `UndoManager.ts:89-95,114-118,131-133` |
| R2-3 | Held gate pad survives an active-drum-track switch: release goes to every target that got the hit, and the old sink releases all its live pads | `DrumEngine.ts:48-53,97-120,147-157`; `AE:83-111,2386-2403` |
| R2-4 | Note-repeat overshoot: cancellable scheduled voices, cancel on release, free lookahead 40 ms, pending captures dropped | `NoteRepeat.ts:126-129`; `DrumEngine.ts:55,97-108,122-133`; `SamplerKit.ts:96-122,317-330,439-443`; `useMidiNotes.ts:146-215` |
| R2-5 / R3-E1 | Gate length in musical ticks plus counted loop wraps, not seconds × BPM | `LR:106-127,347-371`; `AH:161-168` |
| R2-6 | MIDI export applies swing | `ExportService.ts:31-34,211-221` |
| R2-7 | `swingGrid` independent of quantize (default 1/16; migration from old 1/8 quantize) | `types/sampler.ts` (SwingGrid); `AH:128-130,191-202` |
| R2-8 | Song dock pads use press/release (gates, note repeat) | `components/InstrumentDock.tsx:22-24` (UI wiring) |
| R2-9 | `SamplerKit.prune` uses `context.currentTime`, not the event time | `SamplerKit.ts:416-436` |
| R2-10 | Latency: disabled track compressor bypassed; no master compressor; live FX stages bypassed when idle | `trackAudioFactory.ts:20-26,103-105,152-171`; `AE:605,1864`; `masterBus.ts:15-21`; `AudioEngine.ts:387-418` |
| R2-11 | Note lengths use the tempo at event time | `AE:1410-1435` |
| R2-12 | Beat-view metronome clicks during plain pattern playback | `AE:1291-1294` |
| R2-13 | Incremental Part add per recorded hit | `AE:1442-1452`; `LR:330-335` |
| R2-14 | Unique "Drums N" names | `AE:434-435,460-466,500` |
| R3-E2 | Classic synthesis in a Web Worker (main-thread fallback yields) | `classic/render.ts`, `classic/classic.worker.ts` (Rust: render off the audio thread; cache) |
| R3-E3 | LoopRecorder and Metronome extracted; dead helpers removed | `LR`, `Metronome.ts` |
| R3-U4 | Kit-preview token re-checked after each await (no leaked SamplerKit) | `components/packs/kitPreview.ts` `play()` (`superseded()`) |
| R3-U5 | Click at −15 dB (≈ −13 dBFS) | `Metronome.ts:3-8` |
| R3-U6 | Pattern loop is transient; stored markers untouched; back to the Song loop on stop | `AE:166-170,358-367,1245-1247` |
| R3-P1 | `pack` source kind; absolute packs-dir paths migrated | `factoryKit.ts:181-200`; `bufferCache.ts:80-86` |
| R3-P2 | Pack generation in cache keys; samplers reload stale pads | `bufferCache.ts:37-38,65-78,95-131`; `SamplerKit.ts:187-191,221-234` |
| R4-B1 | Never `stop()` a stopped transport; remember the scheduled start; `transportBeatAt` returns the start beat before it | `AE:170-186` (resetTransport), `AE:177,1189-1193,323-328` |
| R4-B3 | Pack invalidation reloads SampleManager samples in place (same id); running audio players get the new buffer | `bufferCache.ts:102-107`; `SampleManager.ts:172-233`; `AE:186-188,264-282` |
| R4-M3 | Pre-install preview uses SamplerKit with an injected loader (choke/decay/reverse/pan/filter/pitch) | `SamplerKit.ts:124-135,363-371`; `kitPreview.ts:113-121,186-190` |
| R4-L-e | Loop-wrap list pruned | `LR:19-21,179-188` |
| R5-N1 | Stop during count-in cancels the pending transport start (`start(t)+stop(t)`, not a stop on a stopped clock); count-in clicks disposed | `AE:1158-1186,1242` |
| R5-N2 | Kit selection source / session tracking so a pending "Get & load" is dropped on user choice or project change | `AE:44-55,149,193-210,1589,2420-2446` |
| R5-N3 | SampleManager per-sample reload generation; latest request wins | `SampleManager.ts:16-23,172-233` |
| R5-N4 | Seek before the scheduled start restarts playback from the new beat | `AE:1250-1268` |
| R5-N5 | Unavailable-sample warnings and waveform revision | `AE:212-255`; `SampleManager.ts:24,236-266` |
| "R1" (in code comments) | No stray pattern hit on count-in abort/restart | `AE:1176-1183` |

Not engine-related (packs installer, security, UI layout), so no audio port work: R3 L1/L2, S1–S7, P3/P4, U1–U3; R4 B2, M1, M2, L-a…L-d, L-f; R5 N6–N8.

---

## 8. Quirks to decide on before porting (keep or fix deliberately)

1. `play()` always starts at beat 0 (`AE:1136`).
2. An audio region does not sound if playback starts inside it (`AE:1382-1384`, `ExportService.ts:150-152`).
3. Undo entries are pushed before validation, so no-op entries are possible (e.g. `AE:910`).
4. `resetTrackEffects` turns delay feedback 0 into 0.3 and time 0 into 0.25 (`AE:1822-1823`).
5. Hidden automation lanes are silent (`AE:1880,1893`). WAV export has no automation and ignores solo (`ExportService.ts:127-128`).
6. Punch recording appends notes from every pass, although the code comment says "latest pass wins" (`AE:880-894`).
7. Track LFO min/max are not refreshed by `resetTrackEffects` when an LFO already exists (`AE:1836-1849`).
8. The `chorusDepth` automation step is applied immediately in the callback, not at the scheduled time (`AE:1934-1937`).
9. Factory voices render at the device sample rate; classic voices always render at 44.1 kHz. Rust needs resampling or rate-aware rendering.