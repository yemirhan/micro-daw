import { describe, expect, it } from 'vitest';
import { createChordJudge, CHORD_WINDOW_MS } from './chordJudge';
import type { NoteEvent } from '@/services/NoteEventBus';
import type { ExpectedChord } from '@/types/appMode';

const C: ExpectedChord = { root: 'C', quality: 'maj', display: 'C' };
const Am: ExpectedChord = { root: 'A', quality: 'm', display: 'Am' };

function driver() {
  const judge = createChordJudge();
  let t = 0;
  const results: (boolean | null)[] = [];
  const send = (e: NoteEvent, target: ExpectedChord) => {
    const r = judge.handle(e, target);
    if (r) results.push(r.correct);
  };
  return {
    judge,
    results,
    on: (note: number, target: ExpectedChord, dt = 10) =>
      send({ type: 'noteon', note, velocity: 0.8, channel: 1, time: (t += dt) }, target),
    off: (note: number, target: ExpectedChord) => send({ type: 'noteoff', note, channel: 1, time: (t += 10) }, target),
  };
}

describe('chord judge', () => {
  it('counts a held chord once and needs a release before the next attempt', () => {
    const d = driver();
    [60, 64, 67].forEach((n) => d.on(n, C));
    expect(d.results).toEqual([true]);
    d.on(72, C); // still holding: not a new attempt
    expect(d.results).toEqual([true]);
    [60, 64, 67, 72].forEach((n) => d.off(n, C));
    [64, 67, 72].forEach((n) => d.on(n, C)); // inversion
    expect(d.results).toEqual([true, true]);
    expect(d.judge.index).toBe(2);
  });

  it('a wrong chord is one wrong attempt', () => {
    const d = driver();
    [60, 63, 67].forEach((n) => d.on(n, C)); // C minor
    expect(d.results).toEqual([false]);
    expect(d.judge.index).toBe(0);
    [60, 63, 67].forEach((n) => d.off(n, C));
    [57, 60, 64].forEach((n) => d.on(n, Am)); // judged against the given target
    expect(d.results).toEqual([false, true]);
  });

  it('on-screen keyboard: notes clicked one after another count as a chord', () => {
    const d = driver();
    for (const n of [57, 60, 64]) {
      d.on(n, Am, 300);
      d.off(n, Am);
    }
    expect(d.results).toEqual([true]);
  });

  it('clicks too far apart do not combine', () => {
    const d = driver();
    d.on(57, Am);
    d.off(57, Am);
    d.on(60, Am, CHORD_WINDOW_MS + 100);
    d.off(60, Am);
    d.on(64, Am, 100);
    expect(d.results).toEqual([]);
  });
});
