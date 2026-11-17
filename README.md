<p align="center">
  <img src="docs/images/icon.png" alt="Micro DAW" width="128" height="128" />
</p>

<h1 align="center">Micro DAW</h1>

<p align="center">
  A lightweight desktop music workstation for making beats and music with MIDI.
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

### DAW / Arrangement

Create multi-track arrangements with MIDI instruments and drum patterns.

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
| `Space` | Play / Stop |
| `R` | Record |
| `M` | Toggle metronome |
| `L` | Toggle loop |
| `Ctrl+Z` / `Ctrl+Y` | Undo / Redo |
| `Ctrl+E` | Export dialog |
| `Ctrl+K` | Command palette |
| `Ctrl+1-3` | Switch modes (DAW, Samples, Dev) |
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
├── components/
│   ├── arrangement/  # Timeline, tracks, regions, markers, groups
│   ├── dev/          # MIDI developer monitor
│   ├── mixer/        # Channel strips, level meters
│   ├── piano-roll-editor/  # Note grid, velocity lane
│   ├── samples/      # Sample library browser
│   ├── settings/     # Settings view
│   ├── sidebar/      # Navigation sidebar
│   └── ui/           # shadcn/ui primitives
├── config/           # Command definitions
├── data/
│   └── templates/    # Project templates (rock, jazz, hip-hop, etc.)
├── hooks/            # React hooks (audio, MIDI, arrangement, etc.)
├── services/         # Core engines (AudioEngine, DrumEngine, ArrangementEngine, etc.)
├── types/            # TypeScript type definitions
└── utils/            # Helpers, constants, encoders
```

## Architecture

Micro DAW follows an **imperative audio engine + declarative UI** pattern:

- **AudioEngine**, **DrumEngine**, and **ArrangementEngine** are singleton services that interact directly with Tone.js and the Web Audio API. Audio calls happen outside the React render cycle for zero latency.
- **React** drives the UI only. Hooks subscribe to engine state and translate user interactions into engine calls.
- **ArrangementEngine** owns `Tone.Transport` for scheduling. Each track has independent audio (an instrument from `src/audio/` or the synthesized drum kit).
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

- Piano samples: [Salamander Grand Piano V3](https://archive.org/details/SalamanderGrandPianoV3) by Alexander Holm, licensed [CC-BY 3.0](https://creativecommons.org/licenses/by/3.0/) (reduced MP3 set as redistributed with Tone.js), bundled in `src/audio/samples/piano/`.

## License

[MIT](LICENSE) - Yusuf Emirhan Sahin
