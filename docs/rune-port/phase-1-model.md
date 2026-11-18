# Phase 1: model crate (`rune/crates/mdaw-model`) vs the Electron app

**Scope:** the pure data and logic of the Electron app, with no audio, UI or I/O:
- types and the `.mdaw` format
- migrations
- arrangement, beat, piano-roll, automation, note and chord helpers
- kits and classic-machine metadata
- the undo manager
- controller mapping, knob routing and soft takeover
- pack validation and the packs page model
- the localStorage keys
- the `ArrangementDoc`, which is the data and undo half of `ArrangementEngine.ts`

## Method: same inputs, same outputs

1. **Golden fixtures produced by the Electron code itself.**
   - Each `rune/parity/ts/*.golden.ts` imports the real TypeScript modules from `src/`, under the same Tone mock and `@` alias as `npm test`.
   - It runs them over a large deterministic input battery and writes `rune/parity/fixtures/<name>.json`.
   - The Rust tests replay every input and assert the same output: exact for strings, integers and orderings, 1e-9 for floats.
   - Command: `npx vitest run --config rune/parity/vitest.golden.config.mts`
2. **Ported vitest suites.** Each relevant `*.test.ts` case was rewritten as a Rust test with the same assertions.
3. **Drift check.** Regenerating all 16 fixtures from the current TS source reproduces them byte for byte (`git status` clean after regeneration). So the fixtures describe the Electron app as it is at `4d98422`.

| Golden fixture | TS source | Cases compared |
|---|---|---|
| `swing` | arrangementHelpers: `quantizeBeat`, `swingOffsetBeats`, `loopGateBeats`, `wrapBeat`, `patternStepAt` | 42,538 vectors (7 grids × 2 swing grids × 8 swing amounts × 214 beats, …) |
| `kit` | factoryKit (`normalizePad`, `normalizeKit`, with and without a packs dir), factory kit, 6 classic kits, GM migration and export mapping | 939 |
| `automation` | automationHelpers | 1,194 |
| `loop` | `normalizeLoopSettings` / `normalizeBeatState` | 99 |
| `migrate` | every template through `loadFromTemplate` and `loadProject`; 6 synthetic legacy projects; template id remapping. Produced by driving the real `arrangementEngine` | 20 whole-arrangement comparisons |
| `beat_helpers` | beatHelpers, stepKeys, chop, keyScope, missingPackActions, waveform | ≈22,000 |
| `notes` | noteHelpers, waveformHelpers, chordDetection | ≈3,000 |
| `piano_roll` | pianoRollHelpers (every export) | 34,793 |
| `knobs` | KnobRouter curves and inverses, KnobPickup, 40 random router sessions (6,040 steps, every engine call logged) | ≈47,000 |
| `controller` | ControllerMapping: serialization (byte-identical JSON), validation, lookups, 36 learn sessions (2,556 steps) | ≈12,100 |
| `undo` | UndoManager: 25 random sessions | 4,000 steps |
| `packs_validate` | packValidate on the real `packs/catalog.json`, about 270 broken manifests (error messages verbatim), predicates, merges | 519 |
| `packs_model` | packsModel, PackService pure parts, remote kit plans, kit-preview grooves | 1,932 |
| `settings` | every localStorage reader. The script cuts each reader's source out of its component and runs it, so the TS is not reimplemented | 336 |
| `fs_entries` | `fs:list-dir` filtering + `Intl.Collator` sort | ≈11,700 |
| `doc` | the real `arrangementEngine` driven through 4 scripted sessions (song editing, markers/groups/automation, beat, templates/legacy/storage/loop takes). After each step it records the arrangement JSON, undo/redo labels, canUndo/canRedo, session id and clipboard | 279 steps, all equal |

**Totals:**
- 412 Rust tests (`cargo test -p mdaw-model`), about 370 of them ports of vitest cases, plus the golden tests above.
- `cargo clippy -p mdaw-model --all-targets -- -D warnings` is clean.

## Project file compatibility

- `types.rs` uses the TS field names.
  - `T | null` serializes as `null`; optional fields are omitted.
  - Unknown keys on ProjectFile, Arrangement and Track are kept and written back.
- **Loading** goes through `migrate::load_project_json`, which runs the TS migration on the raw JSON before typing it. The migration is the same as the TS one:
  - velocity >1 → /127
  - GM drum notes → pads
  - kit normalisation
  - beat state
  - loop markers
  - `swingGrid` from an old 1/8 quantize
  - absolute pack paths → `{kind:'pack'}`
  - the 4 restored FX values
- **Evidence:** the `migrate` fixture holds all 6 templates and 6 legacy projects. Rust output equals the TS output as JSON.
- **Writing:** JSON numbers come out as `120.0` where JS writes `120`; both apps read either form.

## Deviations (deliberate, documented in code)

| Area | Electron | Rust | Why |
|---|---|---|---|
| JS prototype keys | `"toString"` passes `in` checks (voice ids, quantize names, key maps) | rejected | It's a JS accident, never valid data |
| `duplicateRegion` of an audio clip | both copies share one `audio` object, so trimming one shifts the other | independent copies | Shared mutable state is a TS aliasing bug; user approved independent copies (2026-11-18) |
| Integers beyond ±2^63 (`chokeGroup`, `size`) | kept as double | saturate | Unreachable in practice |
| `Intl.Collator` beyond U+024F | ICU script order | NFKD + code point | No ICU dependency; Latin, Latin-1 and Latin Extended match ICU exactly (table generated from Node's ICU) |
| Non-array `tracks`/`notes` in a corrupt file | walks a string's characters | error, file rejected | |
| Malformed `1e400` in stored controller JSON | drops one mapping | default profile | serde_json can't parse overflowing numbers |

**TS quirks kept on purpose** (each with a test):
- Undo snapshots are pushed before validation.
- `setLoopMarkers(-2, 0.5)` gives `[0, 0.5)`.
- `Number('') → 0` for master volume.
- A negative pitch class prints as `"undefinedm"`.
- `velocityBucket(v, 0) = -1`.
- Pad learn consumes invalid notes.
- …and the others listed in the agent reports, which were folded into the code comments.

## Not covered in this phase

Deferred to phase 3 (they need the transport), and listed case by case in `rune/crates/mdaw-model/src/doc/tests*.rs`:
- all of `ArrangementEngine.reload.test.ts`
- round-5 N1/N4
- the round-4 cases
- the timing halves of `ArrangementEngine.beat.test.ts`
- the render-count cases of `useArrangement` / `useBeat`
