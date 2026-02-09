<p align="center">
  <img src="docs/images/icon.png" alt="Micro DAW" width="128" height="128" />
</p>

<h1 align="center">Micro DAW</h1>

<p align="center">
  A lightweight desktop music workstation for learning, practicing, and creating music with MIDI.
</p>

<p align="center">
  <img src="https://img.shields.io/badge/version-1.3.0-blue" alt="Version" />
  <img src="https://img.shields.io/badge/electron-40-47848F?logo=electron" alt="Electron" />
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

### Learn Mode

Interactive, step-by-step music lessons.

- Piano lessons (scales, chords, melodies)
- Drum lessons (basic patterns, fills)
- Music theory (intervals, chord progressions)
- Ear training (interval recognition, chord quality, melody direction)
- Song tutorials (Twinkle Twinkle, Ode to Joy, Happy Birthday)
- Improvisation guides (pentatonic, blues)
- Real-time validation against your MIDI input
- Progress tracking with localStorage persistence

<p align="center">
  <img src="docs/images/screenshot-learn.png" alt="Learn Mode" width="700" />
</p>

### Practice Mode

Structured practice activities with feedback.

- Free play with chord detection
- Scale practice with highlighted keys
- Rhythm training with visual metronome
- Drum pattern practice
- Accuracy scoring and session statistics
- Practice streak tracking and charts

<p align="center">
  <img src="docs/images/screenshot-practice.png" alt="Practice Mode" width="700" />
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
- **Onboarding tour** - Guided walkthrough for first-time users
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
| `Ctrl+1-5` | Switch modes (DAW, Learn, Practice, Samples, Dev) |
| `Shift+M` | Add marker |
| `[` / `]` | Previous / Next marker |
| `Ctrl+G` | Create track group |
| `Cmd+,` | Settings |
| `Escape` | Close piano roll editor |

## Tech Stack

| Layer | Technology |
|-------|------------|
| Desktop shell | [Electron](https://www.electronjs.org/) 40 + [Electron Forge](https://www.electronforge.io/) |
| Frontend | [React](https://react.dev/) 19 + [TypeScript](https://www.typescriptlang.org/) 5.7 |
| Build | [Vite](https://vitejs.dev/) 5 |
| Styling | [Tailwind CSS](https://tailwindcss.com/) v4 + [shadcn/ui](https://ui.shadcn.com/) |
| Audio engine | [Tone.js](https://tonejs.github.io/) |
| MIDI | [WebMidi.js](https://webmidijs.org/) |
| Animations | [Framer Motion](https://www.framer.com/motion/) |
| Charts | [Recharts](https://recharts.org/) |
| MIDI export | [midi-writer-js](https://github.com/grimmdude/MidiWriterJS) |
| Auto-update | [electron-updater](https://www.electron.build/auto-update) |

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) >= 18
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

### Type Check

```bash
npm run typecheck
```

## Project Structure

```
src/
├── app/              # App root component
├── components/
│   ├── arrangement/  # Timeline, tracks, regions, markers, groups
│   ├── dev/          # MIDI developer monitor
│   ├── learn/        # Lesson player and step components
│   ├── mixer/        # Channel strips, level meters
│   ├── onboarding/   # Tour overlay and tooltips
│   ├── piano-roll-editor/  # Note grid, velocity lane
│   ├── practice/     # Practice activities and stats dashboard
│   ├── samples/      # Sample library browser
│   ├── settings/     # Settings view
│   ├── sidebar/      # Navigation sidebar
│   └── ui/           # shadcn/ui primitives
├── config/           # Command definitions
├── data/
│   ├── lessons/      # JSON lesson files (piano, drums, theory, ear-training, songs)
│   └── templates/    # Project templates (rock, jazz, hip-hop, etc.)
├── hooks/            # React hooks (audio, MIDI, arrangement, practice, etc.)
├── services/         # Core engines (AudioEngine, DrumEngine, ArrangementEngine, etc.)
├── types/            # TypeScript type definitions
└── utils/            # Helpers, constants, encoders
```

## Architecture

Micro DAW follows an **imperative audio engine + declarative UI** pattern:

- **AudioEngine**, **DrumEngine**, and **ArrangementEngine** are singleton services that interact directly with Tone.js and the Web Audio API. Audio calls happen outside the React render cycle for zero latency.
- **React** drives the UI only. Hooks subscribe to engine state and translate user interactions into engine calls.
- **ArrangementEngine** owns `Tone.Transport` for scheduling. Each track has independent audio (PolySynth or drum sampler).
- **UndoManager** takes full snapshots of the arrangement state (`structuredClone`) before each mutation.

## Screenshots Setup

To add your own screenshots, create a `docs/images/` directory and place the following files:

| Placeholder | Description |
|-------------|-------------|
| `docs/images/icon.png` | App icon (128x128) |
| `docs/images/screenshot-daw.png` | Main arrangement view |
| `docs/images/screenshot-piano-roll.png` | Piano roll editor |
| `docs/images/screenshot-midi.png` | MIDI device connected / instrument dock |
| `docs/images/screenshot-learn.png` | Learn mode with a lesson |
| `docs/images/screenshot-practice.png` | Practice mode |
| `docs/images/screenshot-samples.png` | Sample library |

## License

[MIT](LICENSE) - Yusuf Emirhan Sahin
