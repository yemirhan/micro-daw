# Wave A — delivered APIs (as built)

## WP1 sampler

### API
Contract, implemented exactly:
- factoryKit.ts: defaultPadSettings(): PadSettings (decay 0.05, filterCutoff 20000, mode 'oneshot'); createFactoryKit(): KitState (id 'factory', name 'Factory', returns fresh objects on every call); FACTORY_VOICE_LABELS: Record<FactoryVoiceId,string>; PAD_COLORS: string[] (16 oklch colors, the first 8 match the old pad colors).
- bufferCache.ts: loadPadBuffer(source: PadSampleSource): Promise<AudioBuffer>; getCachedPadBuffer(source): AudioBuffer | null.
- SamplerKit.ts: class SamplerKit { constructor(kit: KitState); get ready(): Promise<void>; connect(node: Tone.InputNode): this; setKit(kit: KitState): void; trigger(pad, velocity, time): void; release(pad, time): void; stopAll(time?): void; dispose(): void }.
- slicer.ts: detectTransients(buffer: AudioBuffer | Float32Array, sampleRate: number, opts?: { sensitivity?: number; maxSlices?: number; minGapSec?: number }): number[]; equalSlices(durationSec: number, count: number): number[].
- drumEngine: start(): Promise<void>; hit(pad, velocity): void; release(pad): void; setSink(sink: DrumSink | null): void; setFallbackKit(kit): void; preview(source): Promise<void>; stopPreview(): void.
- kitStore: list(): KitState[]; save(kit): void (insert or replace by id); remove(id): void.

Additional exports (nothing in the contract was renamed or removed):
- factoryKit: FACTORY_KIT_ID; FACTORY_VOICE_IDS; FACTORY_PAD_VOICES (pad layout: kick, snare, chh, ohh, clap, tom-mid, crash, ride | kick-sub, rim, phh, shaker, snap, tom-lo, tom-hi, perc; the hats share choke group 1); FACTORY_VOICE_GM_NOTE: Record<FactoryVoiceId, number> (for MIDI export); gmNoteToFactoryVoice(note): FactoryVoiceId | null; factoryPadForGmNote(note): PadIndex | null (for GM migration); isFactoryVoiceId; factorySample(voice): PadSample; normalizePad(raw: unknown): PadSettings; normalizeKit(raw: unknown): KitState (always 16 pads, values clamped); cloneKit(kit): KitState.
- bufferCache: padSourceKey(source): string; preloadKitBuffers(kit): Promise<void> (never throws); invalidatePadBuffer(source); getReversedBuffer(buf): AudioBuffer; trimTail(channels, sampleRate, opts?): Float32Array[]; clearPadBufferCache().
- SamplerKit: readonly output: Tone.Gain; getKit(): KitState; isPadReady(pad): boolean; exported computeSlice(pad, bufferDuration) and the constants MAX_VOICES_PER_PAD=8, CHOKE_FADE=0.005, FILTER_BYPASS_HZ=20000.
- slicer: slicesToRanges(starts, durationSec): { startSec; endSec }[] (for chop to pads).
- DrumEngine: exported interface DrumSink { hit(pad: PadIndex, velocity: number): void; release(pad: PadIndex): void }; getSink(); getFallbackKit(); hitByMidiNote(gmNote, vel) is kept as a legacy method (it maps through factoryPadForGmNote); mute, unmute, setVolume, isStarted and dispose are kept.
- KitStore: get(id); subscribe(cb): () => void.
- drumKit.ts: DrumVoice now also includes 'kick-sub' | 'shaker' | 'perc' | 'snap'; new FACTORY_VOICE_RENDER: Record<FactoryVoiceId, { voice; note?; seconds }>; new playFactoryVoice(kit, id, velocity, time); GM_DRUM_MAP gained 54/69/70/82 → shaker, 56/76/77 → perc, 75 → snap. The existing DrumKit API is unchanged.

### Deviations
- The sink receives velocity already converted to 0..1, because drumEngine.hit normalises it before calling sink.hit.
- `ready` is a getter. It resolves once all loads pending at that moment have settled. A failed load, such as a missing file, is logged and does not reject.
- How decay works:
  - One-shot: the fade-out lasts min(decay, slice length) and finishes exactly at the slice end.
  - Gate: on release the voice fades over decay, but the fade never runs past the slice end.
  - A bigger decay therefore fades a one-shot earlier; it does not lengthen it.
- sampleId: loadPadBuffer writes the SampleManager id onto the source object only when that object is not frozen. SamplerKit keeps its own copy of the kit, so the id is not written back into the arrangement's state. If WP4 needs the id, it can get it from sampleManager by path.
- drumEngine.mute() now also drops hits sent to the sink and stops any preview.
- Velocity curve: velocityGain(normVelocity(v), 0.85), the same curve the old DrumKit used.
- Factory voices are rendered as mono at velocity 1. Peaks are scaled down to 0.98 only if they would otherwise clip.

### Known gaps
- Not touched (owned by other packages): the fake drumEngine in src/test/fakeAudioEngine.ts has no `release`, `setSink` or `preview`. WP3 or WP4 tests may need to add them there.
- For WP4 export: call `await preloadKitBuffers(kit)` before Tone.Offline, or create the SamplerKit and await `ready`. If an uncached factory voice is rendered while an export's Offline callback is running, it relies on Tone saving and restoring the global context. That nesting should work but has not been tested.
- Factory buffers render in the background when drumEngine.start() runs. Pad hits in the first few hundred ms after start are silent.
- No test has played real audio. Everything was checked against the Tone mock, so the sound and loudness of the new voices (kick-sub, shaker, cowbell, snap) still need to be judged by ear in the app.
- The transient detector uses an energy rise with 5 ms hops. It is tuned for drum loops, not tonal material.
- Other DrumKit-based code (trackAudioFactory, qa.ts, the existing tests) is left as it was. Replacing DrumKit with SamplerKit in tracks is WP4's job.
- The tone mock was not changed.

## WP2 browser

### API
ElectronAPI additions (src/types/electron.d.ts):
- `export interface FsDirEntry { name: string; path: string; isDir: boolean; ext: string /* lower-case, no dot, '' for dirs */ }`
- `fsListDir: (dirPath: string) => Promise<FsDirEntry[]>`: lists exactly one directory level (no recursion). Only absolute string paths are accepted; empty paths, paths over 4096 chars and paths containing NUL are rejected. It returns sub-folders plus audio files (wav, aif, aiff, mp3, flac, ogg, m4a). Dotfiles are hidden, symlinks are resolved (dangling ones skipped), and the order is folders first, then case-insensitive natural order. If the folder can't be read, the promise rejects. IPC channel: 'fs:list-dir'.
- `fsPickFolder: () => Promise<string | null>` (channel 'fs:pick-folder', openDirectory dialog)
- `fsHomeDir: () => Promise<string>` (channel 'fs:home-dir')
- `getPathForFile: (file: File) => string` (webUtils.getPathForFile; returns '' on failure)

Component: `export function SampleBrowser({ onAssign, className }: { onAssign: (path: string, name: string) => void; className?: string })`. `name` is the file name without its extension. It fills its parent (h-full, flex column).

Hook: `export function useSampleBrowser(): UseSampleBrowser`. State: available, roots, currentPath (null = root list), currentRoot, crumbs, entries (filtered), totalCount, loading, error, filter, selectedIndex, selected, previewPath, autoPreview, suggestions. Actions: setFilter, pickRoot(), addRoot(path), removeRoot(path), navigate(path|null), goUp(), refresh(), select(index, {preview?}), move(delta), preview(entry), stopPreview(), setAutoPreview(on).

Helpers (browserHelpers.ts):
- Constants: AUDIO_EXTENSIONS, SAMPLE_DRAG_MIME = 'application/x-micro-daw-sample', type SampleDragPayload {path, name}
- Names: extOf, isAudioFileName, isHiddenName, displayName
- Listing: compareEntries, sortEntries, filterEntries (every whitespace-separated term must match, case-insensitive)
- Paths (POSIX and Windows): separatorOf, trimTrailingSep, basename, joinPath, parentPath, isWithin, findRootFor, buildBreadcrumbs(root, current) → Crumb[]
- Other: normalizeRoots, moveIndex

Behaviour:
- Mouse: clicking a file previews it via `drumEngine.preview({kind:'file', path})`; clicking a folder opens it. Double-clicking a file assigns it.
- Keys: Up/Down/PgUp/PgDn/Home/End move the selection and auto-preview files (toggle in the header). Enter opens a folder or assigns a file. Right enters a folder. Left or Backspace goes up a level (and from a root back to the root list, re-selecting the folder just left). Space previews. Escape clears the filter, otherwise stops the preview. Typing a character jumps to the filter field; the filter field also handles Up/Down/Enter.
- Drag: file rows set `application/x-micro-daw-sample` = JSON {path, name} plus text/plain, with effectAllowed 'copy'.
- Storage: roots persist in localStorage 'micro-daw:browser-roots'.
- Empty state: a "Choose folder…" button plus one-click suggestions for ~/Music, ~/Downloads and ~/Splice where they exist in the home folder (found via fsHomeDir + fsListDir).
- Without Electron: shows a "Browser unavailable" state. Unmounting stops the preview.

### Deviations
- I added the exported type `FsDirEntry` to src/types/electron.d.ts (additive). src/types/sampler.ts is unchanged.
- I added two extra localStorage keys beyond 'micro-daw:browser-roots': 'micro-daw:browser-last-path' (reopens the last folder if it's still inside a root) and 'micro-daw:browser-auto-preview'.
- src/main.ts imports the pure, DOM-free helpers from ./components/browser/browserHelpers, so the main process and the renderer share one copy of the sorting, hidden-file and audio-extension rules.
- Clicking a folder opens it straight away (DAW-style) rather than only selecting it.
- Auto-preview on arrow keys is on by default, as the plan asks. The task text says Space previews, and Space works too.

### Known gaps
- Typecheck depends on WP1: until `preview` and `stopPreview` exist on DrumEngine, useSampleBrowser.ts has three TS2339 errors. The only other typecheck error is in useMidiNotes.ts (drumEngine.release, WP3/WP1 work in progress).
- Long folders are not virtualised. Rows are memoised with stable handlers, but a folder with thousands of files renders thousands of rows.
- previewPath marks the last-previewed row; there is no "preview ended" event in the contract, so the marker stays until another preview or stopPreview.
- The Splice suggestion only checks ~/Splice. It does not look for custom Splice library locations.
- Not yet placed in any layout: mounting it as a collapsible left panel and wiring onAssign to the selected pad is WP6's job.
- I tested the main-process IPC handlers through typecheck only, not by running Electron.

## WP3 midi

### API
src/services/ControllerMapping.ts
Contract methods on `controllerMapping`:
- getProfile(): ControllerProfile
- setProfile(p: ControllerProfile): void (validates, persists, notifies)
- padForNote(note: number, channel: number): PadIndex | null
- startPadLearn(pad: PadIndex | 'sequence'): void
- startKnobLearn(target: KnobTarget, index?: number): void
- cancelLearn(): void
- onKnob(handler: (target: KnobTarget, value01: number) => void): () => void
- subscribe(cb: () => void): () => void

Extra methods on `controllerMapping`:
- resetToDefault(): void
- reload(): void
- clearPad(pad: PadIndex): void
- setPadChannel(channel: number | null): void
- setCCMapping(index: number, patch: Partial<CCMapping>): void
- removeCCMapping(index: number): void
- getLearnState(): LearnState
- isLearning(): boolean
- skipPad(): void
- previousPad(): void
- handleNoteOn(note: number, velocity: number, channel: number): boolean (true = used by the learner)
- handleCC(cc: number, rawValue: number, channel: number): boolean (true = learned or mapped)
- getLastActivity(): MidiActivity | null
- onActivity(cb: () => void): () => void

Exported pure helpers:
- createDefaultProfile()
- cloneProfile()
- validateProfile(v: unknown): ControllerProfile | null
- serializeProfile()
- parseStoredProfile(raw: string | null): ControllerProfile
- findPadForNote()
- findCCMappings()
- assignPadNote(profile, pad, note)
- assignKnobCC(profile, target, cc, index?)
- KNOB_TARGET_OPTIONS
- knobTargetKey()
- knobTargetFromKey()
- knobTargetLabel()
- sameKnobTarget()

Exported constants: CONTROLLER_STORAGE_KEY = 'micro-daw:controller', CONTROLLER_STORAGE_VERSION = 1, DEFAULT_PROFILE_ID.

Exported types:
- LearnState = {kind:'idle'} | {kind:'pad'; pad; sequence} | {kind:'knob'; target; index?}
- MidiActivity

src/hooks/useControllerMapping.ts
- useControllerMapping(): { profile: ControllerProfile; learn: LearnState }
- useMidiActivity(enabled = true): MidiActivity | null

src/components/controller/ControllerSetupDialog.tsx
- export function ControllerSetupDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (open: boolean) => void })

src/hooks/useMidiNotes.ts (changed signatures)
- drumHit(pad: PadIndex, velocity: number): accepts pads 0..15; anything else is ignored
- onDrumFlash?: (pad: PadIndex) => void

src/services/MidiService.ts
- simulateControlChange(cc: number, rawValue: number, channel = 1)

### Deviations
1. One edit outside my package: in src/types/midi.ts, `MidiCallbacks.onControlChange` now takes an optional channel, `(cc, value, channel?: number) => void`. Without it a CC mapping's channel could never be checked. Existing callers are unaffected.
2. `startKnobLearn` has an extra optional `index` argument: it rebinds that mapping row, and passing `ccMappings.length` adds a new row ("Add knob").
3. Learned CCs are saved with channel = null (any channel).
4. Knob dispatch: CCs that the profile maps (70..77 by default) go to `onKnob` and no longer drive the old filter, resonance, reverb, chorus and volume effects on CC 70..74. CC 1..5 still do.
5. In `useMidiNotes` the old `DRUM_NOTE_TO_PAD`, `DRUM_CHANNEL` and `DRUM_SOUNDS` lookups are gone. On-screen `drumHit` now records and emits `padToNote(pad)` instead of the General MIDI note.
6. 'split' routing is handled like 'auto' (pads come from the mapping), which matches how it behaved before.
7. Behaviour beyond the plan: a finished wizard whose hits all came from one channel limits pads to that channel.

### Known gaps
- `drumEngine.release` is not in WP1's DrumEngine yet, so typecheck fails at src/hooks/useMidiNotes.ts:135 until WP1 adds it.
- src/test/fakeAudioEngine.ts (not mine) has no `release` on its fake drum engine. Current tests never call it there; my routing tests use their own drum mock.
- No soft takeover for knobs: values are sent as soon as a knob moves. Mapping them to ranges is WP6's job.
- Nothing opens the dialog yet; WP6 needs to wire it from the TopBar and command palette.
- Not tested on the real MPK Mini Play hardware.
