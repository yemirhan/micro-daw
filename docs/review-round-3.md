# Review round 3 (commit 1e3de4e) — consolidated findings

Scores — GPT-6.1 Sol: code 8, UI 8.5, UX 7.5. Fable 5.2: code 8.5, UI 9, UX 8.5.
Probe evidence: Sol /tmp/micro-round3-*.log; Fable /private/tmp/claude-501/-Users-yemirhan-air--t3-worktrees-micro-daw-t3-95ed3754/a2ee5388-0fc5-4570-8fc7-0051391e5e89/scratchpad/probes/ and .../etest/ (Electron checks).

## Licensing (decision: remove)
L1. [Sol] TR-8 pack (packs/sources/tr8-mckaudio.json) — Roland documents that TR-8 707/727 voices use original PCM wave data (and original 909 cymbals were PCM). Recordist's CC0 can't clear underlying rights. REMOVE the whole tr8-mckaudio pack from sources + catalog; document the rejection in docs/packs.md. Built-in 909/707/606-style kits cover it.
L2. [Fable] Sonic Pi pack ships README but no licence text file — include the upstream LICENSE.md (pinned) as a pack file. uzu: note in docs that Mot4i-contributed samples are covered by the repo's Unlicense per README (verify if cheap; otherwise drop those files).

## Security / main process
S1. [Fable] No window-open / navigation guards; non-pack IPC unguarded (src/main.ts). Add setWindowOpenHandler (https → shell.openExternal, deny otherwise), will-navigate block for anything but the renderer URL, assertTrustedSender on EVERY ipc handler; window-level dragover/drop preventDefault in renderer so a Finder drop outside a pad can't navigate.
S2. [Fable] Runtime validator weaker than curation: require sha256 + size + 40-hex commit-pinned raw.githubusercontent.com URL at runtime; verify byte count; PACK_ALLOWED_HOSTS → ['raw.githubusercontent.com'] (+ redirect targets actually needed, verify); remote catalog must not silently override with weaker data (compare version/updatedAt; reject invalid entries); restrict `source` links to https://github.com/.
S3. [Fable] Preview IPC fetches any allowlisted URL → accept only URLs present in the loaded catalog.
S4. [Fable] Catalog `color` unvalidated CSS → allow only #hex or oklch(...); add a CSP meta to index.html compatible with Vite dev + prod (no remote script; allow data:/blob: for audio if needed).
S5. [both] swapIntoPlace crash between renames loses the pack (cleanupStale deletes .old-*). Recovery: if <id> missing and a valid .old-<id>-* exists, restore it.
S6. [Fable] Timeouts surface as "Download cancelled" → distinct timeout error message.
S7. [Fable] Per-file retry (2–3 with backoff) and resume (keep verified files across Retry).

## Packs data model / caching
P1. [Fable] Pack kits store absolute paths → new PadSampleSource `{ kind: 'pack'; packId: string; file: string }` resolved against the packs dir at load; migrate absolute paths inside the packs dir to this kind on load; warn on uninstall when the open project uses the pack; missing pack → pad shows "Pack not installed — Get" action.
P2. [Sol] Pack update keeps cached old audio (bufferCache + SampleManager cache by path) → invalidate all buffers for that pack on install/uninstall/update and refresh active samplers (version in cache key).
P3. [Fable] Remove cross-layer imports (PackService importing @/main/packs/validate; main.ts importing a renderer component helper) → shared pure module under src/shared/.
P4. [Fable] Friendly sound names (808 list shows bd0010) → add optional `label` to PackFile, fill for the Fischer pack (e.g. "Kick · tone 0 · decay 10"), display it.

## UX / UI
U1. [Sol] Failed sound previews: no visible error → inline error per sound + retry.
U2. [Fable] Kit preview and Load on pads disabled until installed → pre-install kit preview from remote files via preview IPC; "Get & load" one-step action.
U3. [Fable] Beat transport wraps at 1360 px with browser open; swing-grid segment unlabeled next to quantize "1/16" → fold swing grid into the swing control (e.g. small popover / label "Swing 1/16"), no wrap at 1360 with browser open.
U4. [Fable] kitPreview.play can leak a SamplerKit (two calls before Tone.start resolves) → re-check token after await, dispose.
U5. [Fable] Metronome click peaks -4 dBFS vs kick -11 → lower click ~8–10 dB, keep audible.
U6. [Fable] playPattern rewrites user loop markers without undo → only rewrite when markers don't already cover the pattern range? Decide: if user markers differ, keep them unless the step-grid "Loop pattern" button is used; Beat Play uses the pattern range transiently without mutating stored markers if feasible.

## Engine
E1. [Sol] Gate length after LARGE tempo change invents loop passes (60→300 BPM: 1.417 beats stored as 4) → compute unwrapped musical elapsed time from transport ticks (track loop passes via transport loop events or tick deltas), no elapsed-seconds × final-BPM estimate.
E2. [Sol] Cold classic-kit synthesis blocks the main thread (14–59 ms, 61 ms timer gap > 40 ms repeat look-ahead) → render in a Web Worker (pure DSP in classic/dsp.ts should be worker-safe) or chunk with idle-time prewarm; never block during playback.
E3. [Fable] Dead exports stepAtBeat, loopGateDuration → remove. ArrangementEngine is 2,893 lines → extract LoopRecorder (+ count-in/metronome) into its own module behind the same public API (only if it can be done safely with tests green).
E4. [both, partial] Each hit still triggers App-wide re-render → split useArrangement subscriptions (useSyncExternalStore selectors) so pad hits/recorded notes only re-render the step grid + pad grid.
