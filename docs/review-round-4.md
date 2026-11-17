# Review round 4 (commit 9df2c52) — consolidated findings

Scores — GPT-6.1 Sol: code 8, UI 8.8, UX 8. Fable 5.2: code 8.5, UI 9.2, UX 8.5.
Both: not merge-ready until the High items are fixed; both would approve after.
Probe evidence: Sol /tmp/micro-r4-own; Fable /private/tmp/claude-501/-Users-yemirhan-air--t3-worktrees-micro-daw-t3-6fda2bfd/5093bee6-9ea5-479b-bb61-2a3a3360d80d/scratchpad (probes/r4g.log, r4e.log, r4d.log for B1; eapp/ Electron file:// harness; inst/ installer fault injection).

## Blockers (High)
B1. [Fable N1] Stale transport position ~100 ms after every start following an earlier run. play() calls transport.stop() while already stopped → Tone TickSource.stop cancels the earlier stop event → tick lookups before the new start return the old run extrapolated over the idle gap. Effects: (a) Song Play with loop off auto-stops immediately after ~30 s idle (stale beat > lengthBeats); (b) playhead/LCD/step cursor jump for ~5 frames each Play; (c) hits within 100 ms of Record stored at the old position, gate lengths wrong. Sites: ArrangementEngine.ts:1009 (also 729, 1038, 2543), read at :196, auto-stop :1292; LoopRecorder.ts:255, :336. Fix: never stop an already-stopped transport; remember scheduled start time+beat and have transportBeatAt return the start beat for earlier times. Real-Tone regression test: play → stop → idle → play.
B2. [Sol N1] Immediate Retry can report an incomplete pack as installed: Promise.all rejects before aborted sibling workers finish; their cleanup deletes the retry's new files. installer.ts:222. Fix: abort, then await settlement of every worker before cleanup/error/lock release. Concurrent-retry regression test.
B3. [Sol N2 + Fable N6] Pack invalidation removes SampleManager entries/buffers under the pack folder → Song audio regions using those files go silent in playback AND WAV export. bufferCache.ts:102. Fix: keep sample identities; reload affected SampleManager buffers in place (same sampleId) and rebuild affected track players; never silently drop.

## Medium
M1. [Sol N3] Failed crash recovery deletes the last valid backup (restore throws → cleanup deletes .old-*). installer.ts:369. Keep recovery candidates when restore fails; delete backups only after a valid final install is confirmed.
M2. [Sol N4 + Fable N2] "Get & load" intent: latest request must win (load-intent token), and the load must survive leaving the Packs page (pending load lives in packService/app level; only navigation depends on mount).
M3. [Sol N5] Pre-install kit preview uses a separate BufferVoices path missing choke/decay/reverse/pan/filter and playback-rate-aware duration. Reuse SamplerKit with an injected buffer loader (remote preview buffers).

## Low
L-a. [Fable N3] Each recorded hit re-renders BeatTransport, PadGrid, PadEditor, StepSequencer — give the pattern its own store slice so only StepSequencer re-renders.
L-b. [Fable N4] Every PadThumb subscribes to pack availability (10 Hz re-render of 16 pads during downloads) — compute once in BeatView, pass booleans.
L-c. [Fable N5] Withdrawn/unknown pack (not in catalog) offers "Get" forever — say "no longer available" and offer clear/replace (load a built-in kit's pad or empty).
L-d. [Fable N7] Missing-pack strip truncates at 1360 w/ browser; two-line pad name collides with "no pack" badge; uzu/Sonic Pi/FreePats sound lists show raw file names → add PackFile.label for those packs (derived, human-friendly); stray indentation App.tsx:617.
L-e. [Fable N8] PackInstaller.install throws synchronously on invalid manifest (should reject); quitting mid-install aborts as cancel and isn't awaited (should keep resumable staging and await on before-quit); LoopRecorder.wraps grows unbounded when no gate held.
L-f. [Fable top-5 #5] Move the Electron file:// smoke test (CSP, navigation/window-open guards, IPC sender guard) and installer fault-injection cases into the repo as runnable scripts/tests (npm script `test:electron`), so these regressions are caught automatically.
