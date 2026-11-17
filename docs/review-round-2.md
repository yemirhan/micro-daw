# Review round 2 (commit ef9dff2) — consolidated findings

Scores — GPT-6.1 Sol: code 7.5, UI 8, UX 7. Fable 5.2: code 7.5, UI 8.5, UX 7.5.
Probe evidence: /tmp/micro-review/evidence.txt (Sol); /private/tmp/claude-501/-Users-yemirhan-air--t3-worktrees-micro-daw-t3-c4751118/0b447e24-bb1b-418a-9e6d-9763b5cf25e4/scratchpad/probe1-8.mjs (Fable).

1. [both] Pattern loop regression: Beat Play on projects/templates without loop markers loops the note range (hip-hop template loops 0–16 while grid shows 2 bars). useBeat.ts:~308 calls setLoopSettings({}) which is now a no-op. Fix: explicit engine `playPattern()`/`applyPatternRange()` used by Beat Play; migrate missing markers to [0, bars*4).
2. [both] Undo during count-in / mid-take with no hits corrupts history (redo lost, stray "Loop Record" entry). Also undo within an open 400 ms gesture group undoes both the group and the previous step. Fix: stop/cancel active take and close open undo groups BEFORE popping history; snapshot restore must never push history.
3. [both] Switching active drum track while a gate pad is held strands the voice on the old track. Fix: bind held pads to their originating sink (release goes to the sink that started it) and/or release old sink's live voices on sink change.
4. [both] Note repeat overshoots release: look-ahead repeats already handed to sampler/recorder still sound (1–2 hits up to ~94 ms late) and get recorded. Fix: cancellable scheduled repeats — on release cancel that pad's future voices (any mode), pending flashes, and remove recorded notes after the release time; shorten free-running look-ahead (~40 ms).
5. [both] Gate duration wrong when BPM changes during a hold (uses elapsed seconds × final BPM). Fix: measure musical ticks (getTicksAtTime at on and off, handle loop wrap).
6. [both] MIDI export ignores swing (WAV/live apply drumPlayBeat). Fix: apply the same transform in exportMidi.
7. [Fable] Swing grid follows record quantize (quantize 1/4 + swing → beats 2/4 half a beat late; changing Quantize re-times pattern). Fix: separate `swingGrid: '1/8' | '1/16'` in LoopRecordSettings (default 1/16), independent of quantize.
8. [both] Song dock pads: only onHit, no release → gates play full slice, no note repeat. Wire drumPress/drumRelease like Beat view.
9. [Fable] SamplerKit.prune drops voices that end before a FUTURE release time → gate notes longer than their slice become untracked (Stop/choke can't silence). Prune against current context time.
10. [Fable] ~12 ms avoidable live latency: per-track compressor inserted even when disabled (trackAudioFactory.ts:58) + master bus compressor (masterBus.ts:114, ~6 ms each). Bypass disabled compressor (rewire on toggle); replace master compressor with a zero-lookahead limiter/saturation or make it optional.
11. [Fable] Tempo change during playback keeps old note lengths: secPerBeat captured when the Part is built (ArrangementEngine ~:1217). Compute per callback.
12. [Fable] Beat-view metronome only clicks while recording; let it click during plain Beat playback when the metronome toggle is on.
13. [Fable] Cleanup: duplicate doc comment on setLoopSettings; unused `_swing` param in beatHelpers.ts:151 and dead `swing` prop on StepSequencer; every recorded hit rebuilds the whole Part + re-renders the app (~10 ms) — incremental add to the running Part if feasible.
14. [Fable] UI: two tracks both named "Drums" (auto-number new drum tracks "Drums 2"); step grid gives no hint when loop range ≠ pattern.
