import { describe, expect, it } from 'vitest';
import { judgeOffset, RhythmScorer } from './rhythmScoring';

describe('judgeOffset', () => {
  it('grades by distance from the beat', () => {
    expect(judgeOffset(0)).toBe('perfect');
    expect(judgeOffset(-30)).toBe('perfect');
    expect(judgeOffset(60)).toBe('good');
    expect(judgeOffset(-120)).toBe('early');
    expect(judgeOffset(120)).toBe('late');
    expect(judgeOffset(400)).toBe('miss');
  });
});

describe('RhythmScorer', () => {
  const interval = 0.5; // 120 BPM quarters

  it('scores perfect playing as 100% and counts missed beats against you', () => {
    const s = new RhythmScorer({ startTime: 10, interval, totalBeats: 4 });
    for (let b = 0; b < 4; b++) s.recordHit(10 + b * interval + 0.01);
    expect(s.accuracy()).toBe(1);

    const lazy = new RhythmScorer({ startTime: 10, interval, totalBeats: 4 });
    lazy.recordHit(10.005); // only the first beat
    expect(lazy.accuracy()).toBe(0.25); // previously this counted as 100%
  });

  it('extra hits on an already-hit beat and off-grid hits are penalised', () => {
    const s = new RhythmScorer({ startTime: 0, interval, totalBeats: 2 });
    expect(s.recordHit(0).timing).toBe('perfect');
    expect(s.recordHit(0.02).timing).toBe('miss'); // double hit
    expect(s.recordHit(0.25).timing).toBe('miss'); // half-way between beats
    s.recordHit(0.5);
    expect(s.accuracy()).toBeCloseTo(2 / 4);
  });

  it('early/late hits count half', () => {
    const s = new RhythmScorer({ startTime: 0, interval, totalBeats: 2 });
    expect(s.recordHit(-0.12).timing).toBe('early');
    expect(s.recordHit(0.62).timing).toBe('late');
    expect(s.accuracy()).toBeCloseTo(0.5);
  });

  it('drum patterns: each expected pad of each beat is judged, wrong pads are misses', () => {
    const s = new RhythmScorer({ startTime: 0, interval, totalBeats: 2, pattern: [[0, 2], [1, 2]] });
    s.recordHit(0.0, 0);
    s.recordHit(0.01, 2);
    expect(s.recordHit(0.5, 0).timing).toBe('miss'); // kick on the snare beat
    s.recordHit(0.5, 1);
    // 3 of 4 expected pads (missed the hi-hat on beat 2) + 1 wrong hit
    expect(s.accuracy()).toBeCloseTo(3 / 5);
  });

  it('live accuracy only counts beats whose window has passed', () => {
    const s = new RhythmScorer({ startTime: 0, interval, totalBeats: 8 });
    s.recordHit(0);
    expect(s.accuracy(0.01)).toBe(1); // beat 0 still open but hit
    expect(s.accuracy(0.7)).toBe(0.5); // beat 1 window passed without a hit
  });

  it('endless free practice starts counting at the first hit', () => {
    const s = new RhythmScorer({ startTime: 0, interval });
    s.recordHit(5.0); // beat 10
    s.recordHit(5.5);
    expect(s.accuracy(5.7)).toBe(1);
  });

  it('eighth-note grids use the subdivision interval', () => {
    const s = new RhythmScorer({ startTime: 0, interval: 60 / 90 / 2, totalBeats: 4 });
    for (let b = 0; b < 4; b++) expect(s.recordHit(b * (60 / 90 / 2)).timing).toBe('perfect');
    expect(s.accuracy()).toBe(1);
  });
});
