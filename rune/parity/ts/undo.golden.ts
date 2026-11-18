import { it } from 'vitest';
import { undoManager } from '@/services/UndoManager';
import type { Arrangement } from '@/types/arrangement';
import { writeFixture } from './fixture';

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Snapshots are plain numbers here (the manager never looks inside them). */
const snap = (n: number) => n as unknown as Arrangement;
const val = (a: Arrangement | null) => a as unknown as number | null;

it('dumps scripted UndoManager sessions', () => {
  const rnd = mulberry32(7);
  const um = undoManager as unknown as {
    undoStack: { label: string; snapshot: number }[];
    redoStack: { label: string; snapshot: number }[];
  };
  const sessions: unknown[] = [];
  for (let s = 0; s < 25; s++) {
    undoManager.clear();
    let emits = 0;
    const off = undoManager.onStateChange(() => emits++);
    let counter = 1000 * s;
    const steps: unknown[] = [];
    const labels = ['Edit Pad', 'Toggle Step', 'Pad Pitch', 'Swing', 'Tempo', 'Loop Record', 'Paste pad'];
    // Bias some sessions to long push runs (MAX_UNDO trimming).
    const pushBias = s % 5 === 0 ? 0.7 : 0.25;
    for (let i = 0; i < 160; i++) {
      const r = rnd();
      const label = labels[Math.floor(rnd() * labels.length)];
      let op: unknown[];
      let ret: unknown = null;
      if (r < pushBias) {
        const v = ++counter;
        op = ['push', label, v];
        undoManager.pushUndo(label, snap(v));
      } else if (r < pushBias + 0.08) {
        const v = ++counter;
        const lazy = rnd() < 0.5;
        let called = 0;
        op = ['begin', label, v, lazy];
        if (lazy)
          undoManager.beginUndoGroup(label, () => {
            called++;
            return snap(v);
          });
        else undoManager.beginUndoGroup(label, snap(v));
        ret = called;
      } else if (r < pushBias + 0.16) {
        const q = rnd();
        const mode = q < 0.3 ? null : q < 0.6 ? true : q < 0.8 ? false : 'even';
        let calls = 0;
        op = ['end', mode];
        if (mode === null) undoManager.endUndoGroup();
        else
          undoManager.endUndoGroup((before) => {
            calls++;
            return mode === 'even' ? val(before)! % 2 === 0 : mode;
          });
        ret = calls;
      } else if (r < pushBias + 0.2) {
        const v = ++counter;
        const q = rnd();
        const mode = q < 0.4 ? null : q < 0.7 ? true : false;
        let calls = 0;
        let currentCalls = 0;
        op = ['flush', v, mode];
        undoManager.flushGroup(
          () => {
            currentCalls++;
            return snap(v);
          },
          mode === null
            ? undefined
            : () => {
                calls++;
                return mode;
              },
        );
        ret = [calls, currentCalls];
      } else if (r < pushBias + 0.24) {
        const v = ++counter;
        op = ['rebase', v];
        undoManager.rebaseGroup(snap(v));
      } else if (r < pushBias + 0.27) {
        op = ['markDirty'];
        undoManager.markDirty();
      } else if (r < pushBias + 0.4) {
        // An app-style undo: pop, then push the current state onto redo.
        op = ['undo'];
        const lbl = undoManager.undoLabel() ?? null;
        const got = val(undoManager.undo());
        if (got !== null && rnd() < 0.85) {
          const v = ++counter;
          undoManager.pushRedo(lbl ?? 'x', snap(v));
          op = ['undo', v];
        }
        ret = got;
      } else if (r < pushBias + 0.5) {
        op = ['redo'];
        const lbl = undoManager.redoLabel() ?? null;
        const got = val(undoManager.redo());
        if (got !== null && rnd() < 0.85) {
          const v = ++counter;
          undoManager.pushUndoFromRedo(lbl ?? 'x', snap(v));
          op = ['redo', v];
        }
        ret = got;
      } else if (r < pushBias + 0.52) {
        op = ['clear'];
        undoManager.clear();
      } else {
        const v = ++counter;
        op = ['pushRedo', label, v];
        undoManager.pushRedo(label, snap(v));
      }
      steps.push([
        op,
        ret,
        {
          canUndo: undoManager.canUndo(),
          canRedo: undoManager.canRedo(),
          undoLabel: undoManager.undoLabel() ?? null,
          redoLabel: undoManager.redoLabel() ?? null,
          grouping: undoManager.isGrouping(),
          undoLen: um.undoStack.length,
          redoLen: um.redoStack.length,
          // Bottom entry + top 3 (full stacks at the end of each session).
          undoBottom: um.undoStack.slice(0, 1).map((e) => [e.label, e.snapshot]),
          undoTop: um.undoStack.slice(-3).map((e) => [e.label, e.snapshot]),
          redoTop: um.redoStack.slice(-3).map((e) => [e.label, e.snapshot]),
          emits,
        },
      ]);
    }
    off();
    sessions.push({
      steps,
      finalUndo: um.undoStack.map((e) => [e.label, e.snapshot]),
      finalRedo: um.redoStack.map((e) => [e.label, e.snapshot]),
    });
  }
  undoManager.clear();
  writeFixture('undo', { sessions });
});
