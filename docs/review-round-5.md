# Review round 5 (commit b6afe8e) — consolidated findings

Scores — GPT-6.1 Sol: code 8.2, UI 9.0, UX 8.2. Fable 5.2: code 8.8, UI 9.3, UX 8.7.
Verdict (both): not merge-ready only because of N1; both approve once N1 (+ N2) are fixed with probes green.
Probes: Sol /tmp/micro-r5-own (own-audio.mjs/.log, load-intent.log). Fable /private/tmp/claude-501/-Users-yemirhan-air--t3-worktrees-micro-daw-t3-67dcd6f3/dead7d1b-9418-4ef2-a2c4-65c7a66bdc10/scratchpad/probes (r5-countin.log, r5-countin-r4.log baseline), installer harness .../scratchpad/inst/dbg.mjs.

## Must fix
N1 [both, High]. Stop during count-in leaves the pending transport start alive: resetTransport (ArrangementEngine.ts:~1073) skips transport.stop() when state reads 'stopped', but a future start(countInEnd) (created ~:2619) is pending. Record→Stop mid count-in→Record again: pattern starts early, hit stored at beat 1/1.5; →Play: playhead jumps back; →wait: Tone transport starts while engine says stopped. Fable's verified fix: `const pending = this.transportStart !== null && this.transportStart.time > Tone.now(); if (transport.state !== 'stopped' || pending) transport.stop();` Add count-in-abort cases (restart record, play, wait) to scripts/probes + test:audio.
N2 [both, Medium]. Pending Get & load overrides later user choices: Beat-view loadKit/loadFactoryKit don't invalidate the intent token; also applies to a newly opened project, and swaps kit mid-take. Fix: any explicit kit selection (any view) cancels pending intent; pending load is bound to the target track id + kit identity + project session at request time and dropped if any changed; defer while loop-recording (apply after stop) ; show a small "Getting <pack>… ✕" chip in the Beat pads header with cancel.

## Follow-ups (do in same round)
N3 [Sol]. Overlapping SampleManager reloads: first-completion-wins keeps older audio. Per-path generation token; latest request wins; same for failure/unavailable updates.
N4 [Fable]. Seek within ~100 ms of Play: one playhead frame ≈0 and beat-0 step fires (ArrangementEngine.ts:~1142). Update transportStart on seek before start takes effect and reschedule the part from the new position.
N5 [Fable]. sampleManager.isUnavailable unused: Song audio clips from a removed pack get no warning; Song waveforms not refreshed after in-place reload. Show a warning badge on affected audio regions + refresh waveform peaks on reload.
N6 [Fable]. Withdrawn pack shows raw id: keep a small "withdrawn packs" name map in catalog (or remember pack name in the kit's pad sample name/pack ref at load time) so the strip shows the human name. Per-pad "no pack" badge hidden under 56 px: use a compact dot indicator instead.
N7 [Fable]. will-quit awaits pauseAll with no cap: add a 3 s timeout; fix the misleading comment (no auto-resume — or implement auto-resume of paused installs on next start; pick the simpler: fix the comment and show "Resume" state on the card).
N8 [Fable]. Export button wraps to its own row at 1440 px with browser open: keep transport one row at all widths ≥1360 with browser open.
