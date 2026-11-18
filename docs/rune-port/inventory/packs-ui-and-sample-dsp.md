# Parity checklist: Packs page UI and sample processing/generation DSP

This file was added after the phase 0+1 reviews: Fable M4 found the Packs page UI missing, and Sol #4 found the sample DSP missing. Paths are relative to `src/`.

## A) Packs page UI (`components/packs/**`)

### Mounting and navigation
- [ ] Rendered when `mode === 'packs'` inside an ErrorBoundary panel. `onOpenBeat` switches to daw/beat (`app/App.tsx:111-115, 717-721`).
- [ ] Ways in: Cmd/Ctrl+3 (`e.code === 'Digit3'`), the `browse-packs` command, and BeatView's `onBrowsePacks`.
- [ ] App registers `packService.setProjectKitsProvider(tracks' kits)` and clears it on unmount (`App.tsx:174-177`).

### Header (`PacksView.tsx:104-140`)
- [ ] Package icon; title "Packs"; subtitle "Drum kits for your pads. Built-in machines load instantly; free sample packs download once."
- [ ] Search field:
  - 260px wide, leading search icon, placeholder "Search packs, sounds, styles", aria "Search packs".
  - An X button ("Clear search") appears only when the query is non-empty.
  - Escape clears a non-empty query and prevents the default action.
- [ ] Tag chips form a radiogroup labelled "Filter by tag":
  - Chips, in order: "All" (count of all items), then "Installed" (`__installed`, only when at least one pack is installed), then `topTags(all, 9)`.
  - Each chip shows a mono count.
  - Clicking the active chip does not clear it.
- [ ] `topTags` normalises tags (lowercase, NFKD, accents stripped), counts each once per item, sorts by count descending then alphabetically, and caps at the limit (`packsModel.ts:165-182`).

### Search and filter (`packsModel.ts:137-159`, `PacksView.tsx:57-63`)
- [ ] Every whitespace-separated word must appear in name + tagline + tags + description (remote items also match author + licence).
- [ ] Tag matching is exact after normalisation.
- [ ] The "Installed" filter hides built-ins.
- [ ] `filtering = query.trim() !== '' || tag !== null`.

### Layout and states (`PacksView.tsx:142-256`)
- [ ] Scroll column with max width 1400; an optional 380px detail aside on the right.
- [ ] Grid is `auto-fill, minmax(320px, 1fr)`.
- [ ] "Built-in kits" section, note "Synthesized in the app, no download". Shown when it has matches or no filter is active.
- [ ] "Downloadable packs" section, note "Free recordings with open licences". The count shows only when the catalog is ready.
- [ ] Catalog states:
  - **Loading with items:** a 13px spinner, "Loading catalog", "Updating catalog".
  - **Loading, no items:** 4 skeleton cards, aria "Loading packs".
  - **Error, no items:**
    - CloudOff icon, "Can't reach the pack catalog".
    - Body `{error || 'Check your internet connection.'} Built-in kits still work offline.`
    - "Try again" button, which calls `retryCatalog`.
  - **Error with items:** "Offline: showing installed packs only." plus "Try again".
  - **Filtered, no results:** `No packs match "<q>"` (or "No packs with this tag") and a "Show all packs" button that clears both the query and the tag.
  - **Empty catalog:** "No downloadable packs yet" / "The catalog is empty right now. Built-in kits are ready to play above."
  - **Non-desktop:** error defaults to "Downloading packs needs the desktop app."
- [ ] Status mapping (`usePacksController.ts:65-69`):
  - `error` if the catalog status is error, or the API is unavailable and there is no catalog.
  - Otherwise `ready` if a catalog exists, else `loading`.

### Items (`packsModel.ts:61-135, 184-202`)
- [ ] Remote items = catalog packs (deduplicated) + installed packs the catalog no longer lists.
- [ ] The Packs page has no "withdrawn" state; that UI lives on the Beat side.
- [ ] Built-in items:
  - id `builtin:<machine>`.
  - Tags `[808|909|707|606|linn|cr-78, 'built-in', 'drum machine']`.
  - Default description `${name} sounds synthesized right here in the app: an original take on the classic machine's character, not samples of it.`
- [ ] Remote items:
  - `soundCount` excludes `.txt`/`.md` files.
  - `sizeBytes` is the sum of known sizes, or null if none is known.
- [ ] `formatBytes`: "N B" / rounded "N KB" / "x.x MB" below 10 / rounded "N MB" at 10 and above / "" for invalid input.
- [ ] Licence labels:
  - CC0-1.0 → "CC0"; public-domain → "Public domain" (the card shows "PD"); CC-BY-4.0 → "CC BY 4.0".
  - Public domain has no URL.

### PackCard (`PackCard.tsx`)
- [ ] The whole card is a button: aria-pressed = selected, aria-label `name: tagline`.
- [ ] Clicking toggles selection and stops another item's kit preview.
- [ ] Selected style: a 1.5px ring in the pack colour plus an 18% halo.
- [ ] Contents:
  - Small Faceplate, name (13.5px, truncated), tagline (2 lines).
  - Badge: the licence label, or "Synth" for built-ins.
  - "N sounds", with ", size" appended only in the `get` state.
  - InstallButton on the right, on top of the card button.

### InstallButton (`InstallButton.tsx`, state derivation `packsModel.ts:254-275`)
- [ ] Button states:
  - **builtin:** Zap icon + "Ready".
  - **get:** "Get" with a Download icon, aria `Get ${name}`.
  - **downloading:** a ProgressRing (18px, stroke 2.5, spins at 70% dash when progress is unknown), a mono percentage (aria-live), and an X ("Cancel download", aria `Cancel downloading ${name}`).
  - **installed:** Check + "Installed".
  - **resume:** outline "Resume", tip "Download paused: continues where it stopped".
  - **error:** red outline "Retry", tip = the message or "Download failed".
- [ ] Every click stops propagation. Height is h-7 (sm) or h-8 (md).
- [ ] Progress = bytes if the total is known, else file index / file count, else null.
- [ ] Label: "NN%", else "i/n", else "…".
- [ ] State derivation:
  - An error with message "Download paused" → resume.
  - Any other error → error.
  - installed → installed.
  - Otherwise → get.

### Faceplate (`Faceplate.tsx`)
- [ ] 4×4 grid in MPC layout. Sizes: sm 68px (3px gap, 5px padding); lg 132px (5px gap, 8px padding).
- [ ] Lit pads mix the pack colour with surface-1: 62% for pads below 8, 46% for the rest.
- [ ] Hit pads mix the pack colour 70% with white and add a glow. Unlit pads use surface-2.
- [ ] Which pads are lit:
  - Built-in: `machine.pads`, else the first N voices.
  - Remote: the first kit's pads, else the first `soundCount` pads.
- [ ] Preview flash: a rAF loop lights each pad for one step at `padsAt(session, now)`.

### PackDetail (`PackDetail.tsx`), the 380px aside with aria `${name} details`
- [ ] Head:
  - Radial tint in the pack colour.
  - Close button (tip "Close", shortcut Esc, aria "Close details").
  - Large Faceplate, name (17px), tagline.
  - Badges: licence or "Synth"; "N sounds"; size (remote only).
- [ ] Remote action row:
  - md InstallButton.
  - Hint text by state:
    - get: "Downloads once, then works offline."
    - resume: "Paused: picks up where it stopped."
    - error: the message, in red.
  - "Update" (RefreshCw) when installed and the installed version is lower than the catalog version.
  - Uninstall error alert: "Couldn't uninstall: {err}".
  - Uninstall flow:
    - First click arms "Confirm uninstall" (red); second click uninstalls.
    - Disarms after 4000 ms, or 9000 ms when the project uses the pack.
    - In-use warning: "Your open project plays sounds from this pack. Its pads go silent until you get the pack again."
- [ ] Kits section:
  - Title "Kits"; the aside shows "N kits" only when there is more than one.
  - Empty text: "This pack has no ready-made kit; play its sounds below."
  - Built-in shows one row `${name} kit`; remote shows one row per kit with its pad count.
  - Preview button (Play/Square):
    - Tips: "Stop preview" / "Play a short groove on this kit" / "Play a short groove (streams the sounds, nothing is installed)".
    - Has aria-pressed.
  - "Load on pads" (tip "Replace the pads of the current drum track"). The first row uses the filled variant, the others outline.
  - "Get & load" (remote, not installed):
    - Disabled while downloading; shows "Getting…" while pending.
    - Tips: "Loads on the pads when the download finishes" / `Download ${pack}, then load this kit on the pads`.
  - Inline preview error:
    - Shows the partial-failure text while playing, else `Preview failed: ${err}`.
    - Plus a "Retry" button.
- [ ] Sounds section:
  - Title "Sounds" with the count.
  - Summary alert: "Couldn't play {label}: {err}. Click it to retry."
  - Remote sounds are grouped by folder (root first, natural sort), each header with a Folder icon and a count.
  - Rows are a 2-column grid of buttons. Icon: Play / Volume2 while playing / TriangleAlert with a red "Retry" tag on error.
  - Labels: the file's `label`, else the basename without its extension.
  - Remote, not installed: footer "Previews stream from the source before you download."
- [ ] About section:
  - The description.
  - Remote:
    - "Sounds by" (author, linked to the source).
    - "Licence" (linked when a URL exists).
    - "Credit" (only when attribution exists).
  - Built-in: "Inspired by the machine's sound; not affiliated with or endorsed by its maker."
- [ ] Escape, with a selection open, stops the preview and closes the panel. It is skipped when the event is already prevented, a dialog is open, or the target is an input (`PacksView.tsx:68-80`).

### Service calls (`usePacksController.ts`)
- [ ] Unmount stops the kit preview and the sound preview; a pending Get & load survives.
- [ ] `onKitLoaded` navigates to the Beat view.
- [ ] Install, cancel and retry:
  - install / cancel go to `packService`; cancel also drops a pending load.
  - An error matching /cancelled/i resets the state; anything else becomes an error state.
  - retryCatalog calls `loadCatalog({force:true})`.
- [ ] `playSound` previews:
  - Built-in: a classic source.
  - Installed: the local file.
  - Otherwise: a remote URL.
  - Errors are stored per `${itemId}::${voice|path}`.
  - Playback: Volume −6 → BufferSource with gain 0.9 and a 10 ms linear fade-out. Remote buffers are cached.
- [ ] Kit preview (`kitPreview.ts`):
  - 2 bars of 32 sixteenth steps at 96 BPM, starting at now + 0.08, through a SamplerKit → Volume −6.
  - Auto-stops after `32·dur + 1.2` s. Only one preview runs at a time.
  - Pad roles come from regex classification of pad names.
  - Pattern:
    - kick: 0,6,10,16,22,26,27 at .95
    - snare (or clap): 4,12,20,28 at .9
    - clap: 28 at .7 when a snare exists
    - closed hat: even steps except 14 and 30 at .6, plus 7 and 23 at .35
    - open hat: 14 and 30 at .7
    - perc: 11 and 25 at .55
    - fallback: the first 8 sounding pads every 4 steps at .85
  - Errors:
    - "Couldn't load this kit's sounds (reason)"
    - `${failed} of ${total} sounds couldn't load (reason)`
    - "Preview failed"
  - Remote kits use `remoteKitPlan` + a preview loader (LRU of 32).
- [ ] `loadKit`: stop the preview → `cancelPendingLoad` → `setKit(ensureDrumTrack(), kit)` → navigate to Beat.
- [ ] `getAndLoad`: `packService.getAndLoad(id, kitId)`.
- [ ] `usedByProject` comes from `kitsUsePack`.

## B) Sample processing and generation (`services/SampleProcessor.ts`, `hooks/useSampleLibrary.ts`, `useSignalPreview.ts`, `components/samples/*`)

### processAudio
- [ ] **Order:** reverse (plain JS, before Offline) → Offline [filter → PitchShift → FeedbackDelay → Reverb] → normalize.
  - The UI says the stages "run top to bottom" in a different order. Keep the real DSP order.
- [ ] Offline runs only if filter, pitch, reverb or delay is on. Reverse-only or normalize-only runs keep the buffer's length and rate.
- [ ] Tail:
  - Starts at 0; plus `reverb.decay` if reverb is on; plus `delay.time / (1 − feedback)` if delay is on.
  - Then `min(tail, 3)`.
- [ ] Render length = duration + tail; channels = the source's; rate = the source's. Frames are truncated.
- [ ] Source playback: `Tone.Player` (no fades) at 0.
- [ ] Filter: Tone.Filter `{type, frequency, Q}`, rolloff −12. Types: lowpass, highpass, bandpass.
- [ ] PitchShift: Tone defaults (windowSize 0.1, delayTime 0, feedback 0, wet 1). It is two sawtooth-swept delay lines, crossfaded.
- [ ] FeedbackDelay `{delayTime, feedback, wet}`, max delay 1 s.
- [ ] Reverb `{decay, wet}`:
  - preDelay 0.01.
  - IR: stereo white noise of length `decay + preDelay`. Gain is 0 until preDelay, then 1, then an exponential approach to 0 over `decay`.
  - A mono render folds down.
- [ ] Normalize: global peak across all channels. If peak > 0 and peak ≠ 1, scale by 1/peak (target 0 dBFS).
- [ ] Trim is ignored; the full buffer is processed.
- [ ] UI defaults: lowpass 1000 Hz Q 1; pitch 0; reverb 2.5 s / wet .3; delay .25 s / feedback .3 / wet .3; every stage off.
- [ ] UI ranges:

  | Parameter | Range | Step | Display |
  |---|---|---|---|
  | Cutoff | 20–20000 | 1 | "x.xk" or "NHz" |
  | Q | 0.1–20 | 0.1 | 1 decimal |
  | Semitones | ±24 | 1 | "+N" |
  | Reverb decay | 0.1–10 | 0.1 | "x.xs" |
  | Wet | 0–1 | 0.01 | % |
  | Delay time | 0.01–1 | 0.01 | "Nms" |
  | Feedback | 0–0.95 | 0.01 | % |
- [ ] Stage subtitles: "Cut lows or highs", "Up or down in semitones", "Play the sample backwards", "Raise the peak to full scale", "Add room and tail", "Echoes".
- [ ] Intro: "Switch on the stages you want; they run top to bottom and create a new sample, leaving the original untouched."
- [ ] Button: "Apply to new sample" / "Processing…". Disabled while processing or with no stage on; the hint is "Switch on at least one stage".
- [ ] Results are named `${name} (processed)`, registered with an empty path, appended and selected. Samples with an empty path are not persisted.

### generateSignal
- [ ] Noise: uniform white `(rand·2−1)·amp`, mono, `ceil(sr·dur)` samples, no fades.
- [ ] Tones: Offline, mono at the context rate. `Oscillator{type, freq}` → `Volume(gainToDb(amp))`, phase 0, no fades.
- [ ] Defaults: sine 440 Hz, 1 s, 0.8.
- [ ] Names: `white-noise ${d}s` or `${type} ${f}Hz ${d}s` (JS number formatting).
- [ ] UI:
  - Waveform chips: Sine, Square, Saw, Triangle, Noise.
  - "Freq" 20–20000 (disabled for noise) with a note suffix.
  - "Duration" 0.1–30.
  - "Level" 0–1.
  - Intro: "Render a test tone or noise into a new sample. Hold Preview or play your MIDI keyboard to hear it; a played key also sets the frequency."
  - Hint: "MIDI keys preview at their pitch".

### Preview (`useSignalPreview.ts`)
- [ ] Mounting mutes the audio and drum engines; unmounting unmutes them.
- [ ] Synth:
  - Tones: PolySynth(Synth{osc type}). Noise: NoiseSynth white.
  - Envelope A .01 / D .1 / S .8 / R .3.
  - Output `Volume(gainToDb(amp))`, updated live.
- [ ] "Hold to preview": attack on pointer down at velocity 0.8, with the frequency quantised to the nearest note. Release on pointer up or leave.
- [ ] MIDI:
  - Note-on: attack at velocity/127. A tone also sets freq = `round(midiToFrequency)` and shows the note name.
  - Note-off (or velocity 0): release.

### Audition, trim, info, peaks, import
- [ ] Audition plays `Player.start(now, trimStart, trimLen)`. Progress is tracked by rAF and auto-stops. Toggle "Audition" / "Stop".
- [ ] Trim:
  - start ∈ [0, end − .01]; end ∈ [start + .01, duration]. Reset clears it. Trim is persisted.
  - Handles 2px wide with an 8×12 tab and a 10px hit zone.
  - Inputs "Start (s)" / "End (s)", step .01. Also "Reset trim" and "Drag the orange handles to trim".
- [ ] Info: "Length" (ms / s / m:ss.s), "Rate" (kHz, 1 decimal), "Channels" (Mono / Stereo).
- [ ] Send to arrangement: new audio track; offset = trim start; `lengthBeats = trimmed / 60 · bpm`.
- [ ] Peaks:
  - Max |x| per bucket, then max across channels.
  - SampleManager caches peaks by `${id}:${width}`.
  - Waveform drawing:
    - Mirrored at 0.92·midY.
    - Full waveform: ink at alpha .2.
    - Trimmed region: accent at .9.
    - Outside the trim: .35 black shade.
- [ ] Import:
  - Dialog "Import Audio File" with filter wav/mp3/ogg/flac/aiff/aif/m4a, multi-select.
  - Decoding resamples to the context rate.
  - Name = basename without extension (fallback "Sample"). Deduplicated by path.
  - Library restore keeps the saved name and trim.
