# Piano samples

Salamander Grand Piano V3 by Alexander Holm, licensed under
[Creative Commons Attribution 3.0](https://creativecommons.org/licenses/by/3.0/).

Source: https://archive.org/details/SalamanderGrandPianoV3 (this reduced MP3 set,
one sample per minor third from A0 to C8, is the one redistributed with Tone.js at
https://tonejs.github.io/audio/salamander/).

The files are unmodified copies of `src/audio/samples/piano/` (the Electron app's set). They are
embedded in the `mdaw-engine` binary (`src/piano/samples.rs`, `include_bytes!`). At load time the
engine trims the leading encoder silence, as the app does (`src/audio/pianoSamples.ts`).
