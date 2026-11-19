# Sound Designer (the Samples page)

The Samples page used to be a sample library with Edit / Process / Generate tabs. Getting audio in was
hard, using it on a pad or in the song was not obvious, editing was split across two tabs, and Generate
only rendered test tones. It is now a **sound designer**: you build drums and one-shots from waveforms,
they stay editable, and you can put them on a pad or in the song with one click.

Screenshots (1440×900, `--snap`): `docs/parity/sound-designer/`:

- `samples-empty.png`: first open. The Kick 808 template is loaded in the editor.
- `designer-library.png`: saved sounds. A Snare that has been edited and sent to pad 1 is open.
- `designer-send.png`: the Send-to-pad picker.
- `designer-zap.png`: the Zap template (FM, filter envelope).
- `designer-sample.png`: an imported file with edits.
- `song-audio.png`: a sound added to the song.

## Layout

- **Left (library):** "Start from" lists the ten templates (Kick 808, Punchy Kick, Snare, Clap,
  Closed Hat, Open Hat, Tom, Zap, Bass Pluck, Sub). Below it, "My sounds" has a search field and
  one row per sound. Each row shows whether the sound is a synth or audio, its length, how many
  pads use it, and a dot when it has unsaved edits. Click a row to open and play it; ↑ / ↓ move
  through the list and play each sound.
- **Right (editor):**
  - The name (pencil to rename), a status badge (New — not saved / Edited / Saved), and
    Duplicate and Delete.
  - The rendered waveform, redrawn as you edit, with a playhead. Click it to play.
  - The action bar: **Play** (or Space), **Save**, **Send to pad N** (▾ picks the drum track and
    pad), and **Add to song**.
  - For a synth sound: three oscillator strips, then the Amp envelope (with its curve drawn),
    Filter, Character (drive, crush, click, FM route and amount) and Output (pitch, length, gain,
    trim tail) panels.
  - For imported audio: a single **Edit** panel with start, end, fade in, fade out, gain,
    Normalize, Reverse and Reset.
- **Import:** the "Import audio" button, or drop audio files anywhere on the page.

All controls are the app's own widgets (knob, Segmented, switch, Button, popover) with the
`theme.rs` tokens. No shared widget was restyled.

## Sound engine: `mdaw-dsp/src/sound_design.rs`

`Voice` is a plain struct: no heap, no locks, one sample at a time. Both the offline `render` and
the live preview synth run this same code, so a key played at C4 sounds exactly like the rendered
file. A test checks the two are bit-identical. Signal path:

- **Three oscillator layers.** Waveforms: sine, triangle, PolyBLEP saw, PolyBLEP pulse with
  width (DC-corrected), white noise and pink noise (Kellet filter). Each layer has level, coarse
  (±48 st) and fine (±100 ¢) tuning, and its own pitch envelope: a start offset in semitones that
  decays exponentially (`decay` = the time to fall to about 1 %).
- **FM:** phase modulation from layer `from` to layer `to`, with an index of up to 8 rad. The
  modulator is computed first in each frame.
- **Filter:** a TPT state-variable filter (LP/HP/BP) with cutoff and resonance, plus its own
  exponential envelope (± octaves, decay). A wide-open LP is bypassed. The filter states are
  flushed below 1e-20, so the output has no denormals.
- **Amp envelope:** AHDSR with a curve (−1..1). Each segment follows `1 − (1 − p)^(4^curve)`, so
  0 is linear and positive values are punchy. **Retrig** adds 0–4 extra attacks 11 ms apart,
  which is how the clap is built. A sound is a one-shot: the release starts `release` seconds
  before `length`. When the stages overlap (release ≥ length, or the attack still running at
  that point), the release starts when the (last) attack ends, but no later than halfway
  through the sound, and an attack that doesn't fit is shortened to end there. The attack always
  reaches its peak and the release always reaches 0 at the end.
- **After the envelope:** the click layer (a 1.5 ms noise burst), drive (normalised `tanh`),
  the bit crusher (2–16 bits, off by default) and output gain. Any non-finite sample becomes 0.
- **Randomness:** every random source is seeded in `Voice::start`, so a render depends only on
  (recipe, transpose, velocity), however it is split into blocks.
- **Offline output:** `render` cuts the silent tail when `trim_tail` is on. Silence is below
  −60 dBFS; 5 ms is kept past the last loud sample, then faded out, with a minimum length of
  10 ms.
- **Imported audio:** `apply_sample_edits` handles it, in this order: trim → reverse →
  normalize → gain → fades. It keeps the file's channels and rate (files are decoded at the
  context rate).

## Data model: `mdaw-model/src/sound_design.rs`

- `SoundRecipe` is `Copy`: an array of three `OscLayer`s, `FmRoute`, `AmpEnv`, `FilterParams`,
  drive, crush, click, note, length, trim_tail and gain_db. Every field has a serde default, and
  `sanitized()` clamps every value and replaces NaN/∞ with the default.
- `DesignedSound { id, name, recipe, sample: Option<SampleEdit> }`. With `sample` set, the sound
  is an imported file plus non-destructive edits (`path, start, end?, fade_in, fade_out, gain_db,
  normalize, reverse`), and the recipe is unused.
- **Project:** `Arrangement::sounds` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`).
  - Older projects load with an empty list. A project with no sounds is written exactly as
    before.
  - `open.rs` drops invalid entries with a warning and clamps recipes.
  - Sounds travel with undo snapshots and autosave like the rest of the arrangement.
- **Pads:** `PadSampleSource::Sound { soundId }` (`{"kind":"sound","soundId":…}`). Its cache key
  is `sound:<id>@<generation>`.
- **Doc ops** (`doc/sounds.rs`), one undo step each:
  - `save_sound` ("Save Sound"; saving an unchanged sound writes nothing).
  - `rename_sound` (pads named after the old name follow).
  - `remove_sound` / `delete_sound` (clears the pads that played it and removes the Song clips
    playing it; `delete_sound` puts a library-only sound in the undo snapshot so undo brings it
    back).
  - `send_sound_to_pad` (one "Send to Pad" step: save + assign).
- **User library:** settings key `micro-daw:sound-library` (`DesignedSound[]`, read through
  `read_sound_library` / `write_sound_library`). It loads at startup (the first frame), not when
  the Samples page first opens. Entries of the old `micro-daw-sample-library` are converted once
  into audio sounds: each gets a **fresh id** (the old `sample_id` may be what Song regions of
  existing projects play, with the trim applied by the region), keeps its source path, and its
  trim becomes start/end. The old key itself is not touched.
- **Sounds from elsewhere:** when a pad plays a `Sound` the project doesn't define (a saved kit
  loaded, a pad pasted from another project), the recipe is copied from the library into the
  project's `sounds` (`missing_sounds` / `adopt_sounds`, no undo step of its own: undoing the
  edit that brought the pad removes the pad too). A sound found in neither shows a "Sounds not
  found" card at the bottom right, naming the pads.
- **Manifest:** Song regions playing a designed sound are not written to `sampleManifest`, and
  the load check skips them (files saved earlier listed them with `path: ""`).

## App: `mdaw-app/src/designer.rs`, `sound_preview.rs`, `ui/p9/designer.rs`

- **List:** the library order first, then sounds that are only in the project. When a sound is
  in both, the project's copy wins, because that is what pads play.
- **Editing:** the knobs edit a *draft*. The preview synth's recipe updates at once; the
  render runs in the background and the waveform keeps the last result until the new one lands.
  **Save** writes the sound to the project (undo) and to the library. Leaving a draft with
  unsaved edits saves it: opening another sound, picking a template, leaving the page, closing
  the window, Send and Add to song all do this. A template you never touched is simply
  discarded, so browsing templates leaves no clutter.
- **The draft and the project:**
  - A draft with unsaved edits counts as an unsaved project change (title `*`, the
    unsaved-changes prompt).
  - Every project save (⌘S, Save As, the prompt's Save) saves the draft first.
  - The recovery checkpoint is armed by draft edits and stores the arrangement with the draft
    in it, without saving the sound (no undo step while you turn knobs).
  - Opening or creating another project saves the draft into the project being left (and the
    library) first, then the editor starts over on the new project's sounds.
- **Undo and the library:** library writes follow the project's undo/redo.
  - After an undo or redo, a library entry the project also has takes the project's version.
  - An entry removed by Delete comes back with the undo (at its old position) and goes again
    with the redo. This history lasts for the session and the current project.
  - A sound only in the library (never in this project) is not changed by undo, except for
    Delete.
  - When the open sound's stored version changes (undo, redo) and the draft has no unsaved
    edits, the editor reloads it: knobs, waveform and the preview synth.
- **Delete** asks for a second click ("Confirm delete", with a note of what goes) when the
  sound is on pads, in the Song, or only in the library. One undo step brings everything back:
  the project's copy, the pads, the Song clips and the library entry.
- **Rendering:** each time the doc's state version or the library changes, `sync_store` starts
  a render of every **project** sound whose definition changed (save, undo/redo, project open).
  Renders and file decodes run on the sample worker pool (inline in headless runs). Each job has
  a revision, and a result that isn't the latest for its sound is dropped. A save reuses the
  editor's render when it is of the same definition. The buffer is registered under the sound's
  id (`Samples::put_sound`) only when it succeeds; a file that can't be read is tried again
  every 5 s (`RENDER_RETRY`). Decoded source files are cached up to 256 MB (oldest dropped).
  Library-only sounds are rendered when opened.
  - An id that a Song region loads from a file is never registered or removed as a sound. Once
    an id is a sound, a file load for it that lands later is dropped, and Song sounds get no
    recovery WAV (the project keeps their recipe).
  - Pads load it through `FileLoader::load_sound`, after `PadBufferCache::invalidate_sound(id)`
    bumps the key and the live kits reload.
  - Audio regions refresh through `loaded_ids`.
  - So re-editing (or undoing) a sound that is on a pad updates the pad.
- **Send to pad:**
  - Default target: the active drum track and the selected pad.
  - The ▾ picker shows the drum tracks and a 4×4 pad grid (13–16 on top, as on the hardware);
    clicking a pad sends the sound there and remembers that choice.
  - A notice confirms where the sound went. The pad keeps its gain, pitch and other settings.
- **Add to song:** saves the sound, then adds a new audio track with a region at beat 0. The
  region's `sample_id` is the sound id with an empty path, so the store serves the render. Then
  the app switches to the Song view.
- **Live audition:**
  - Play or Space plays the render on the engine's audition slot.
  - While the page shows, `sound_preview` puts a polyphonic (8 voices) `SoundPreviewSynth` on the
    preview slot and mutes drums and synth with the preview's own mute. The user's mute is never
    changed.
  - MIDI keys play the sound at pitch: C4 plays it as designed. Pads (MIDI channel 10) play it as
    designed. A key release doesn't cut it (one-shot).
  - Audio sounds play their render when a key is pressed.
  - The recipe reaches the audio thread through a mutex the voice side only `try_lock`s.
- **Keys:** Space plays or stops; ↑ / ↓ browse the library (but not while a knob or segmented
  control has focus). The name field and search are text fields; Escape cancels a rename.

## Removed

- The Edit / Process / Generate tabs (`ui/p9/samples.rs` now only draws Song region waveforms),
  their state in `views.rs` (`SamplesTab`, `ProcessParams`, `SignalParams`, trim drafts and
  fields, `samples_tab`, `P9Menu::ProcessFilterType`), and `signal_preview.rs` (replaced by
  `sound_preview.rs`).
- The library side of `samples.rs`: `LibEntry`, import/restore, persistence, trim, audition and
  the process/generate jobs. `samples.rs` now keeps Song imports, region loading, pack reloads,
  peaks, and the designed sounds' buffers.
- `mdaw-engine::sample_processor` (only Process/Generate used it), its parity tests, and their
  fixtures and capture scripts (`parity/fixtures/sample_processor*.json`,
  `parity/chromium/capture-sample-processor*`).
- The old tests (`t_p9_samples.rs` Generate/Process/trim cases; the trim/rename/audition review
  tests in `t_p9_review.rs`) were removed or rewritten. The slider-geometry review test now runs
  on the Settings page.

## Tests

- **DSP** (`mdaw-dsp/src/sound_design/tests.rs`): envelope timing (exact AHDSR points), the curve
  bending the decay, retrig, pitch-envelope sweep (zero crossings: +2 octaves at the start,
  settling on the note, transpose), determinism and block-size independence for every template,
  no NaN or denormal output (extreme and NaN recipes), templates audible and bounded, tail trim
  (cut, fade to 0, minimum length, loud sounds untouched), FM, and the sample edits.
- **Model:**
  - `sound_design` tests: JSON round-trip, partial recipes, clamping, sample edits.
  - `doc/tests_sounds.rs`: save/undo, send to pad (one undo step), rename/delete with undo, the
    recipe round-tripping through the project file and autosave, older projects without sounds,
    bad entries dropped.
- **UI Runtime** (`mdaw-app/src/tests/t_p9_samples.rs`):
  - The page opens on a template.
  - A template plays when picked, and a knob edits the live render and the preview recipe.
  - Save writes the project and the library.
  - Send to pad: the pad's live buffer becomes the render; reopening the sound, editing it and
    saving updates the pad; undo brings the old one back.
  - The pad picker.
  - Autosave when leaving a draft.
  - Space, ↑ / ↓ with auto-preview.
  - Search, rename, duplicate, delete (with undo).
  - Importing a file (edits applied, the pad plays the edited audio).
  - Dropping files.
  - Add to song.
  - MIDI preview.
  - The preview synth matches the offline render bit for bit.
  - Migration from the old library.
  - A reopened project renders its sounds for the pads.
  - The user's mute is preserved.

## Review fixes (GPT-6.1 Sol + Opus 5.5 review)

Each fix has a regression test that fails without it. The tests are in
`mdaw-app/src/tests/t_p9_designer.rs` and `t_p9_samples.rs`, `mdaw-model/src/doc/tests_sounds.rs`
and `mdaw-dsp/src/sound_design/tests.rs`.

1. Migrated sounds get fresh ids, and ids that file-backed Song regions use are protected.
2. A dirty draft makes the project dirty, saves flush it, and the checkpoint includes it.
3. A project switch saves the draft into the old project, then resets the editor.
4. Undo/redo reloads a clean draft, and the library follows undo.
5. A late file load can't replace a sound's render, and Song sounds get no recovery WAVs.
6. Delete asks for confirmation, removes the Song clips too, and undo restores the library
   entry.
7. The library loads at startup, and pads pull missing recipes from it (with a warning when a
   sound can't be found).
8. Renders run in the background with revisions and coalescing, and the decode cache is bounded.
9. Overlapping attack and release stages are handled.
10. A failed render is retried, and a sound counts as rendered only when it succeeds.
11. Song sounds are left out of the manifest and the load check.
12. Templates peak around −2 dBFS (test: ≤ −0.5 dBFS).

## Not verified

- Nobody has listened to it: the templates were tuned from the maths, not by ear.
- The real MIDI device path (MPK mini) and real-time CPU load were not measured. Renders run on
  the sample worker pool. Importing a file still decodes it once on the UI thread, so an
  unreadable file is reported at once.

## Limits

- **Dragging sounds:** the Samples page is its own mode, so there is no drag from the library
  onto Beat-view pads or Song tracks. The explicit Send to pad / Add to song actions replace it.
- **Imported audio is not a synth layer.** An imported file is its own kind of sound (with
  edits); it cannot be used as an oscillator source.
- **Kits saved to the user kit store** (`micro-daw:kits`) can reference a sound by id. In
  another project, that sound plays as long as it is in your library.
