# Beat mode plan (shared contract for all work packages)

Goal: micro-daw becomes a focused beat-making DAW. Plug in an AKAI **MPK Mini Play**
(25 keys, 8 pads with bank A/B → 16 pad notes, 4 knobs), drag samples onto pads,
finger-drum over a looping 1–8 bar pattern that records quantized, fix it in a step
grid, then arrange in the Song view and export.

Shared types live in `src/types/sampler.ts` — read it first. Do not change them without
need; if you must extend, add (don't rename/remove) and say so in your final report.

Commands: `npm run typecheck`, `npm test` (vitest; Tone is mocked in `src/test/toneMock.ts`),
`npm run lint`. Other agents may be editing other files at the same time: only touch files
you own (listed below) unless a tiny change elsewhere is unavoidable — then report it.
If typecheck errors are in files you don't own and caused by others' in-progress work,
ignore them. Do NOT git commit.

## Core model decisions

- **Kits are per drum track**: `TrackInstrument` gets `kit?: KitState` (type 'drums').
  Drum region notes use `padToNote(pad)` (36..51). Old projects/templates whose drum notes
  are General MIDI numbers are migrated on load via the existing `GM_DRUM_MAP`
  (`src/audio/drumKit.ts`) → nearest factory pad.
- **Active drum track**: `Arrangement.beat?: { activeDrumTrackId: string | null; loop: LoopRecordSettings; selectedPad: PadIndex }`.
  Live pads (MIDI + on-screen) play through the active drum track's audio chain (so its FX
  apply) and record into it. If there is no drum track, one is created on first need.
- **Factory kit**: the existing synthesized voices are rendered offline to AudioBuffers once
  and used as pad samples (`{kind:'factory'}`), so a fresh project makes sound immediately.
- **Hardware mapping is separate from the kit**: `ControllerProfile` (pad note map + CC
  mappings) is app-level (persisted in localStorage), not per project.
- **Beat view is the home screen**. The arrangement becomes the "Song" view (tab toggle in
  TopBar). Settings and Samples (existing processing tools) stay in the sidebar.

## APIs between packages (implement exactly these names)

### WP1 — Sampler engine (`src/audio/sampler/**`, `src/services/DrumEngine.ts`, `src/services/KitStore.ts`, `src/audio/drumKit.ts`)
```ts
// src/audio/sampler/factoryKit.ts
export function defaultPadSettings(): PadSettings;
export function createFactoryKit(): KitState;                 // 16 pads, factory voices
export const FACTORY_VOICE_LABELS: Record<FactoryVoiceId, string>;
export const PAD_COLORS: string[];                            // 16 oklch colors
// src/audio/sampler/bufferCache.ts
export function loadPadBuffer(source: PadSampleSource): Promise<AudioBuffer>; // cached; factory voices rendered via Tone.Offline once; files via SampleManager
export function getCachedPadBuffer(source: PadSampleSource): AudioBuffer | null;
// src/audio/sampler/SamplerKit.ts  — works in realtime AND inside Tone.Offline
export class SamplerKit {
  constructor(kit: KitState);
  readonly ready: Promise<void>;                 // all pad buffers loaded
  connect(node: Tone.InputNode): this;
  setKit(kit: KitState): void;                   // diff + (re)load changed pads
  trigger(pad: PadIndex, velocity: number /*0..1*/, time: number): void; // honours trim, pitch, reverse, gain, pan, filter, choke, decay
  release(pad: PadIndex, time: number): void;    // only matters for mode 'gate'
  stopAll(time?: number): void;
  dispose(): void;
}
// src/audio/sampler/slicer.ts  (pure, unit-tested)
export function detectTransients(buffer: AudioBuffer | Float32Array, sampleRate: number, opts?: { sensitivity?: number /*0..1*/; maxSlices?: number; minGapSec?: number }): number[]; // slice start times (s), first is 0
export function equalSlices(durationSec: number, count: number): number[];
// src/services/DrumEngine.ts — live pad facade
drumEngine.start(): Promise<void>;
drumEngine.hit(pad: PadIndex, velocity: number): void;   // velocity 0..1 or 0..127
drumEngine.release(pad: PadIndex): void;
drumEngine.setSink(sink: { hit(pad, vel): void; release(pad): void } | null): void; // WP4 registers the active drum track; fallback = own SamplerKit with factory kit
drumEngine.setFallbackKit(kit: KitState): void;
drumEngine.preview(source: PadSampleSource): Promise<void>; // audition a sample (browser); stops previous preview
drumEngine.stopPreview(): void;
// src/services/KitStore.ts — user kits saved to localStorage ('micro-daw:kits')
kitStore.list(): KitState[]; kitStore.save(kit: KitState): void; kitStore.remove(id: string): void;
```
`DrumPadId` (0..7) in `src/types/drums.ts` is superseded by `PadIndex`; WP1 widens/aliases it.

### WP2 — Sample browser (`src/main.ts`, `src/preload.ts`, `src/types/electron.d.ts`, `src/components/browser/**`, `src/hooks/useSampleBrowser.ts`)
- IPC: `fsListDir(path) → {name, path, isDir, ext}[]` (audio exts: wav, aif, aiff, mp3, flac, ogg, m4a), `fsPickFolder() → string|null`, `fsHomeDir()`, `getPathForFile(file: File) → string` (Electron `webUtils.getPathForFile`).
- Browser panel: user-added root folders (persisted, localStorage `micro-daw:browser-roots`), tree/list navigation, search filter within the current folder, click = preview via `drumEngine.preview`, arrow keys move + auto-preview, Enter = assign to selected pad (`onAssign(path, name)` prop).
- Drag source format: `dataTransfer.setData('application/x-micro-daw-sample', JSON.stringify({ path, name }))`.
- Export `<SampleBrowser onAssign={(path,name)=>void} />`.

### WP3 — MIDI controller mapping (`src/services/MidiService.ts`, `src/hooks/useMidiNotes.ts`, `src/services/ControllerMapping.ts`, `src/hooks/useControllerMapping.ts`, `src/components/controller/**`)
```ts
controllerMapping.getProfile(): ControllerProfile;
controllerMapping.setProfile(p: ControllerProfile): void;     // persisted 'micro-daw:controller'
controllerMapping.padForNote(note: number, channel: number): PadIndex | null;
controllerMapping.startPadLearn(pad: PadIndex | 'sequence'): void; // 'sequence' = "hit pads 1..16 in order" wizard
controllerMapping.startKnobLearn(target: KnobTarget): void;
controllerMapping.cancelLearn(): void;
controllerMapping.onKnob(handler: (target: KnobTarget, value01: number) => void): () => void;
controllerMapping.subscribe(cb: () => void): () => void;   // profile/learn-state changes
```
- Default profile "MPK Mini (default)": pad notes 36..51 in pad order, channel null (any), knobs CC 70..73 → pad pitch, decay, filterCutoff, gainDb; CC 74..77 → swing, bpm, trackVolume, masterVolume.
- Routing in `useMidiNotes`: a note that `padForNote` maps → pad hit (`drumEngine.hit`, NoteEventBus `{type:'drum', padId: pad, note: padToNote(pad)}`), note-off → `drumEngine.release`. Channel-10 notes that are NOT mapped must no longer be silently dropped: fall through to the keys (synth). Everything else → synth as today. While learning, the hit is consumed by the learner.
- CCs not handled by sustain/all-notes-off go to `controllerMapping` (learn or dispatch to `onKnob`).
- UI: `<ControllerSetupDialog open onOpenChange />` — pad grid showing mapped notes, "Learn pads (hit 1..16)" wizard with live highlight, per-pad learn, knob list with learn buttons + target select, reset to default, live MIDI activity indicator.

### WP4 — Arrangement + loop recording (`src/services/ArrangementEngine.ts`, `src/utils/trackAudioFactory.ts`, `src/services/ExportService.ts`, `src/types/arrangement.ts`, `src/services/ProjectManager.ts`, `src/utils/arrangementHelpers.ts`, `src/data/templates/**`, `src/hooks/useArrangement.ts`, tests)
```ts
arrangementEngine.getActiveDrumTrackId(): string | null;
arrangementEngine.setActiveDrumTrack(trackId: string): void;
arrangementEngine.ensureDrumTrack(): string;                  // returns active drum track id, creating "Drums" if needed
arrangementEngine.getKit(trackId: string): KitState;
arrangementEngine.setKit(trackId: string, kit: KitState): void;
arrangementEngine.updatePad(trackId: string, pad: PadIndex, patch: Partial<PadSettings>): void; // undoable; use begin/endUndoGroup for drags
arrangementEngine.setPadSample(trackId: string, pad: PadIndex, sample: PadSample | null): void;
arrangementEngine.getSelectedPad(): PadIndex; arrangementEngine.setSelectedPad(pad: PadIndex): void;
arrangementEngine.getLoopSettings(): LoopRecordSettings;
arrangementEngine.setLoopSettings(patch: Partial<LoopRecordSettings>): void; // also sets loop markers to [patternStart, patternStart + bars*4)
arrangementEngine.getPatternRegion(trackId: string): Region | null;        // region on that track covering the loop start
arrangementEngine.ensurePatternRegion(trackId: string): Region;
arrangementEngine.toggleStep(trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity?: number): void;
arrangementEngine.setStepVelocity(trackId: string, pad: PadIndex, step: number, stepBeats: number, velocity: number): void;
arrangementEngine.clearPattern(trackId: string, pad?: PadIndex): void;
arrangementEngine.startLoopRecording(): void;  // count-in (metronome clicks, UI can read getCountInRemaining()), then loop playback; incoming pad hits are quantized (grid+swing) and merged into the pattern region live, every pass (overdub) — or replace on first pass if overdub=false
arrangementEngine.stopLoopRecording(): void;
arrangementEngine.getCountInRemaining(): number;  // beats left, 0 when not counting in
arrangementEngine.onKitChange(cb: () => void): () => void;
```
- Drum tracks build a `SamplerKit` in their audio chain (replacing DrumKit). Register a `drumEngine.setSink` that hits the active drum track's SamplerKit (through its FX/volume). Swing applies to playback of drum regions too? — No: swing is applied at quantize time when recording and when toggling steps (step positions are stored already swung). Keep playback literal.
- Export (WAV) renders drum tracks with SamplerKit inside Tone.Offline (await `ready`). MIDI export: drum notes channel 10, mapped pad → GM note via the factory voice when possible, else padToNote.
- Project save/load: kits serialize with file paths; on load, `{kind:'file'}` pads reload from disk; missing files → pad shows as missing (keep path, name), don't crash.
- Migration of old drum notes (GM numbers) and old drum tracks without kit.
- `useArrangement` exposes the new state (activeDrumTrackId, kit, loop settings, selectedPad, countIn).

### WP5 — Beat UI (`src/components/beat/**`, `src/hooks/useBeat.ts`)
- `<BeatView>` composed of:
  - `PadGrid`: 4×4 MPC-style (bank A = bottom two rows, bank B = top two rows) or 2×8 toggle matching hardware; each pad shows name, color, a tiny waveform, mapped hardware note, flashes on hit (from NoteEventBus), click = hit + select (velocity from vertical click position), drop target for `application/x-micro-daw-sample` and Finder files (`window.electronAPI.getPathForFile`), right-click menu (clear, rename color, copy/paste pad, chop to pads, choke group).
  - `PadEditor` for the selected pad: waveform with draggable start/end trim handles + slice markers, Knobs (`src/components/ui/Knob.tsx`) for gain, pan, pitch, decay, filter; toggles mode one-shot/gate, reverse, choke group select; "Chop" button: transient/equal (4/8/16) → assigns slices to consecutive pads starting at the selected pad (same file, different start/end).
  - `StepSequencer`: rows = 16 pads (labels + mute-less), columns = steps for loop length (16 per bar at 1/16; switchable 1/8, 1/16, 1/32), click toggles, drag paints, alt/option-drag or wheel adjusts velocity (shown as cell opacity), beat-group shading, live playhead column, selected-pad row highlighted.
  - `BeatTransport`: play/stop, record (loop record w/ count-in), BPM, bars (1/2/4/8), quantize grid, swing knob, metronome, count-in, overdub/replace, kit menu (Factory, Save kit…, load user kit), "Clear pattern".
- `useBeat()` hook gathers engine state (via `useArrangement` or direct subscriptions) + actions; components stay presentational.
- Keyboard: computer keys `1–8 / q–i` (or `zxcv asdf qwer 1234` 4×4) hit pads 0..15 when the beat view is focused and not typing.
- Visual language: reuse existing Tailwind tokens (`bg-surface-0`, `border-border`, etc.) and shadcn components in `src/components/ui`.

### WP6 — Integration + phase 4 (App.tsx, TopBar, Sidebar, commands, InstrumentDock, `src/services/NoteRepeat.ts`)
- Beat view = default home; TopBar view toggle Beat | Song; Song = existing arrangement. Sample browser as a collapsible left panel in Beat view (and Song view).
- Wire `controllerMapping.onKnob` → `updatePad(active track, selectedPad, …)`, swing, bpm, track/master volume (map value01 to ranges; pitch ±24 semis, decay log 0.005–4 s, cutoff log 20–20k Hz, gain -40..+6, swing 0..0.75, bpm 60..180).
- Note repeat: toggle (button + shortcut `N`) and rate select; while on and the transport is playing, holding a pad retriggers it at the rate, locked to the transport grid, recorded if loop recording.
- Controller setup dialog reachable from TopBar MIDI device area and command palette.
- Remove dead wiring left by the rip.

## Rip (WP0) scope
Delete Learn, Practice, onboarding tour, lessons/practices data, practice stats (recharts), ear-training
prompt player, the Learning panel, chord/scale learning helpers only used by those, and their
hooks/tests/settings fields (`hasCompletedOnboarding`, lesson/practice reset). Keep: DAW,
arrangement, piano roll, mixer, effects, synths, samples view, settings, dev MIDI monitor,
export, templates, command palette, undo, project save/load. Remove `recharts` from
package.json if unused. AppMode becomes `'daw' | 'samples' | 'settings' | 'dev'`.
