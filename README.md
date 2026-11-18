<p align="center">
  <img src="docs/images/icon.png" alt="Micro DAW" width="128" height="128" />
</p>

<h1 align="center">Micro DAW</h1>

<p align="center">
  A focused desktop beat maker for MIDI pad controllers (built around the AKAI MPK Mini Play).
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-1.3.0-blue" alt="Version" />
  <img src="https://img.shields.io/badge/electron-44-47848F?logo=electron" alt="Electron" />
  <img src="https://img.shields.io/badge/react-19-61DAFB?logo=react" alt="React" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="License" />
</p>

<p align="center">
  <img src="docs/images/screenshot-daw.png" alt="Micro DAW - Arrangement View" width="800" />
</p>

## Features

### Beat view (home screen)

Plug in a pad controller, load samples onto pads, finger-drum over a loop, then fix it in the step grid.
A fresh project opens here with a "Drums" track on the built-in factory kit, a 2-bar loop,
1/16 quantize and the recording metronome on, so you can play straight away.

- 16 pads (bank A/B, 4×4 or 2×8 layout) played from the controller, the mouse or the computer keys
  `Z X C V / A S D F / Q W E T / 1 2 3 4`
- Drag samples from the sample browser (or Finder) onto pads; chop a sample to pads by transients or equal slices
- Per-pad editor: trim, gain, pan, pitch, decay, filter, one-shot/gate, reverse, choke groups
- Loop recording with count-in, quantize (with swing) and overdub/replace
- Note repeat (`N`, rate 1/4 to 1/32): hold a pad to retrigger it on the grid while the transport runs; repeats are recorded too
- Step sequencer (1/8, 1/16, 1/32) with per-step velocity
- Kits per drum track; save and load your own kits
- Controller setup: learn the pad notes and map knobs to pad pitch/decay/filter/gain, swing, tempo and volumes

### Sample browser

A file browser for your sample folders, docked on the left in both views (`B` toggles it).
Click to audition, arrow keys move and preview, `Enter` loads the file onto the selected pad.

### Packs

The Packs page (sidebar, `Ctrl+3`, or "Browse packs…" in the Beat view's kit menu) puts ready-made
16-pad kits one click away. "Load on pads" replaces the current drum track's kit and opens the Beat view.

- **Built-in kits** - six synthesized machines: 808-, 909-, 707-, 606-, LinnDrum- and CR-78-style.
  Every voice is generated in the app (`src/audio/sampler/classic/`), so they work offline and have no
  licensing strings attached. They are original syntheses in the style of those machines, not samples
  of them, and are not affiliated with or endorsed by the makers.
- **Downloadable packs** - free recordings from a curated catalog (`packs/catalog.json`, also fetched
  from GitHub so new packs appear without an app update). Each pack downloads once into
  `<userData>/packs/<id>`, shows up in the sample browser under "Installed packs", and adds its kits to
  the kit menu. Sounds can be auditioned before downloading. Downloads are restricted to GitHub hosts
  over https, size-capped and checked against the sha256 in the catalog.

Included packs (all CC0 or public domain; every pack ships the upstream licence/readme):

| Pack | By | Licence |
|------|----|---------|
| TR-808 Recordings | Michael Fischer / Technopolis (1994), via [tidalcycles/sounds-tr808-fischer](https://github.com/tidalcycles/sounds-tr808-fischer) | CC0 1.0 |
| 909 · 707 · 606 · 727 (TR-8 Recordings) | MckAudio, [MckAudio/MckSamplePacks](https://github.com/MckAudio/MckSamplePacks) | CC0 1.0 |
| uzu Drumkit | Mot4i, Switch Angel and Ludens (TidalCycles community), [tidalcycles/uzu-drumkit](https://github.com/tidalcycles/uzu-drumkit) | Public domain (Unlicense) |
| Sonic Pi Drums & Percussion | Sonic Pi sample library (freesound contributors), [sonic-pi-net/sonic-pi](https://github.com/sonic-pi-net/sonic-pi) | CC0 1.0 |
| FreePats Synth Percussion | Roberto (FreePats project), [freepats/synthesizer-percussion](https://github.com/freepats/synthesizer-percussion) | CC0 1.0 |

Licence evidence, rejected candidates and how to add a pack are in [docs/packs.md](docs/packs.md)
(`node scripts/pack-check.mjs --online` validates the catalog and checks every URL).

### Song view (arrangement)

Arrange patterns and other tracks into a song (`Tab` switches Beat / Song).

- Multi-track timeline with drag-and-drop regions
- Built-in synth presets and drum kits (powered by Tone.js)
- Piano roll editor with draw, select, and erase tools
- Velocity editing per note
- Loop and punch recording
- Timeline markers for navigation
- Track groups with collapse/expand
- Mixer with per-track volume, pan, mute, and solo
- Effects panel (reverb, delay, etc.)
- Undo/redo (up to 50 snapshots)

<p align="center">
  <img src="docs/images/screenshot-piano-roll.png" alt="Piano Roll Editor" width="700" />
</p>

### MIDI Support

Connect any USB MIDI controller and play in real-time.

- Auto-detect MIDI devices
- Zero-latency software instrument playback
- Pads and knobs mapped through a controller profile (default: MPK Mini, pads on notes 36–51, knobs on CC 70–77)
- The keys play the live synth (pick a preset in the top bar)
- "Keyboard" mode to use your controller's built-in sounds while still recording MIDI
- Record MIDI input directly into the arrangement

<p align="center">
  <img src="docs/images/screenshot-midi.png" alt="MIDI Device Connected" width="700" />
</p>

### Sample Library

Import, audition, and trim audio samples.

- Drag-and-drop audio file import
- Waveform visualization with trim handles
- Audition playback (independent of DAW transport)
- Send samples to the arrangement timeline

<p align="center">
  <img src="docs/images/screenshot-samples.png" alt="Sample Library" width="700" />
</p>

### Export

Export your work as audio or MIDI files.

- WAV export (offline render via Tone.js)
- MIDI export (with GM-standard drum mapping on channel 10)
- Respects muted tracks

### Other

- **Project templates** - Start from blank, rock, hip-hop, jazz, EDM, or ambient presets
- **Command palette** - Quick access to all actions (`Cmd+K`)
- **MIDI developer monitor** - Raw MIDI message inspector
- **Auto-update** - Automatic updates via GitHub Releases
- **Keyboard shortcuts** for everything

## Keyboard Shortcuts

| Shortcut | Action |
|----------|--------|
| `Space` | Play / Stop (the Beat view loops the pattern) |
| `R` | Beat view: record loop · Song view: record to the armed track |
| `M` | Toggle metronome |
| `N` | Toggle note repeat |
| `B` | Toggle sample browser |
| `Tab` | Switch Beat / Song view |
| `Shift+Backspace` | Clear pattern (Beat view) |
| `Z X C V / A S D F / Q W E T / 1 2 3 4` | Play pads 1–16 (Beat view) |
| `L` | Toggle loop (Song view) |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+E` | Export dialog |
| `Ctrl+K` | Command palette |
| `Ctrl+1-4` | Switch modes (DAW, Samples, Packs, Dev) |
| `Shift+M` | Add marker |
| `[` / `]` | Previous / Next marker |
| `Ctrl+G` | Create track group |
| `Cmd+,` | Settings |
| `Escape` | Close piano roll editor |

## Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | [Electron](https://www.electronjs.org/) 44 + [Electron Forge](https://www.electronforge.io/) 8 |
| Frontend | [React](https://react.dev/) 19 + [TypeScript](https://www.typescriptlang.org/) 6 |
| Build | [Vite](https://vitejs.dev/) 8 |
| Tests | [Vitest](https://vitest.dev/) + Testing Library + [Playwright](https://playwright.dev/) (UI snapshots) |
| Styling | [Tailwind CSS](https://tailwindcss.com/) v4 + [shadcn/ui](https://ui.shadcn.com/) |
| Audio engine | [Tone.js](https://tonejs.github.io/) |
| MIDI | [WebMidi.js](https://webmidijs.org/) |
| Animations | [Framer Motion](https://www.framer.com/motion/) |
| MIDI export | [midi-writer-js](https://github.com/grimmdude/MidiWriterJS) |
| Auto-update | [electron-updater](https://www.electron.build/auto-update) |

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) >= 22.13
- npm

### Install

```bash
git clone https://github.com/yemirhan/micro-daw.git
cd micro-daw
npm install
```

### Development

```bash
npm start
```

This launches the Electron app with hot-reload via Vite.

### Build

```bash
# Package the app
npm run package

# Create distributable (DMG on macOS, installer on Windows/Linux)
npm run make
```

### Checks

```bash
npm run typecheck   # tsc --noEmit
npm run lint        # ESLint 10 flat config (eslint.config.mjs)
npm test            # Vitest (jsdom); `npm run test:watch` for watch mode
```

Tests live next to the code (`*.test.ts(x)`). `src/test/setup.ts` replaces `tone`
with the Tone.js test double in `src/test/toneMock.ts` for every test, so audio
services and hooks can be tested without an AudioContext.

### UI snapshots (browser, no Electron)

```bash
npx playwright install chromium          # once
npm run snap -- --mode samples --out /tmp/samples.png
npm run snap -- --mode all --prefix baseline-      # every mode -> /tmp/micro-daw-snaps/
npm run snap -- --eval "window.__microDaw.simulateNoteOn(60, 0.8)"
```

`scripts/ui-snap.mjs` starts its own Vite dev server on a free port, drives the
renderer in headless Chromium and prints console/page errors. In dev builds,
`window.__microDaw` exposes the audio/MIDI singletons and `simulateNoteOn/Off`
(same code path as a real MIDI keyboard). See the header of the script for all options.

## Project Structure

```
src/
├── app/              # App root component
├── audio/sampler/classic/  # Built-in 808-/909-/…-style kit synthesis
├── components/
│   ├── arrangement/  # Timeline, tracks, regions, markers, groups (Song view)
│   ├── beat/         # Beat view: pads, pad editor, step sequencer, beat transport
│   ├── browser/      # Sample file browser
│   ├── controller/   # MIDI controller setup (pad/knob learn)
│   ├── dev/          # MIDI developer monitor
│   ├── mixer/        # Channel strips, level meters
│   ├── packs/        # Packs page (built-in kits + downloadable packs)
│   ├── piano-roll-editor/  # Note grid, velocity lane
│   ├── samples/      # Sample library browser
│   ├── settings/     # Settings view
│   ├── sidebar/      # Navigation sidebar
│   └── ui/           # shadcn/ui primitives
├── config/           # Command definitions
├── data/
│   └── templates/    # Project templates (rock, jazz, hip-hop, etc.)
├── hooks/            # React hooks (audio, MIDI, arrangement, etc.)
├── main/packs/       # Main process: pack catalog, validation, downloads, installer, IPC
├── services/         # Core engines (AudioEngine, DrumEngine, ArrangementEngine, etc.)
├── types/            # TypeScript type definitions
└── utils/            # Helpers, constants, encoders
packs/                # Pack catalog (generated) + curated sources (packs/sources/*.json)
scripts/              # ui-snap, pack-build / pack-check
```

## Architecture

Micro DAW follows an **imperative audio engine + declarative UI** pattern:

- **AudioEngine**, **DrumEngine**, and **ArrangementEngine** are singleton services that interact directly with Tone.js and the Web Audio API. Audio calls happen outside the React render cycle for zero latency.
- Drum tracks play a **SamplerKit** (`src/audio/sampler/`): 16 sample pads per track. Live pad hits go through the active drum track (so its FX apply) and into the loop recorder; **NoteRepeat** retriggers held pads on the transport grid and **KnobRouter** maps controller knobs to parameters.
- **React** drives the UI only. Hooks subscribe to engine state and translate user interactions into engine calls.
- **ArrangementEngine** owns `Tone.Transport` for scheduling. Each track has independent audio (an instrument from `src/audio/` or a sampler kit).
- **UndoManager** takes full snapshots of the arrangement state (`structuredClone`) before each mutation.

## Screenshots Setup

To add your own screenshots, create a `docs/images/` directory and place the following files:

| Placeholder | Description |
|-------------|-------------|
| `docs/images/icon.png` | App icon (128x128) |
| `docs/images/screenshot-daw.png` | Main arrangement view |
| `docs/images/screenshot-piano-roll.png` | Piano roll editor |
| `docs/images/screenshot-midi.png` | MIDI device connected / instrument dock |
| `docs/images/screenshot-samples.png` | Sample library |

## Sound Engine

Instruments live in `src/audio/` (all built from Tone.js primitives, no network assets):

- **Engines**: analog-style subtractive voice (2 oscillators with unison/supersaw, sub, noise, per-voice filter with its own envelope, key tracking, velocity to amp/filter, pitch envelope, LFO, mono/legato with glide), 2-operator FM with a decaying tine partial (EPs, bells, mallets), Karplus-Strong strings, and a sampled grand piano.
- **Voice allocation**: `src/audio/voiceAllocator.ts` maps every MIDI note to exactly one voice (re-strikes reuse it, noteOff always finds it, `releaseAll()` silences everything, oldest/released voices are stolen first). `audioEngine.getActiveVoiceCount()` / `getHeldNotes()` expose the state for tests and QA.
- **Presets**: `src/audio/presets.ts` (41 sounds, loudness-matched). Saved projects store a preset index; indices 0-35 keep the role of the previous bank's sounds.
- **Master bus**: glue compressor + zero-latency soft-clip ceiling on the destination, so chords never clip. Live notes bypass Tone's scheduling look-ahead.
- **QA**: `src/audio/qa.ts` renders presets offline and measures levels (load it from the dev server, e.g. through `scripts/ui-snap.mjs --eval`).

## Credits

- Drum packs: see [Packs](#packs) and [docs/packs.md](docs/packs.md) for each pack's author, source and licence. Thanks to Michael Fischer, MckAudio, the TidalCycles and Sonic Pi communities and the FreePats project for releasing their recordings freely.
- Piano samples: [Salamander Grand Piano V3](https://archive.org/details/SalamanderGrandPianoV3) by Alexander Holm, licensed [CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/) (reduced MP3 set as redistributed with Tone.js), bundled in `src/audio/samples/piano/`.

## License

[MIT](LICENSE) - Yusuf Emirhan Sahin
