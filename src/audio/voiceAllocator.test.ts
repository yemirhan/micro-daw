import { describe, expect, it } from 'vitest';
import { MonoNoteStack, PolyVoiceAllocator } from './voiceAllocator';

describe('PolyVoiceAllocator', () => {
  it('assigns distinct voices to distinct notes and releases them', () => {
    const a = new PolyVoiceAllocator(8);
    const v1 = a.noteOn(60, 0);
    const v2 = a.noteOn(64, 0);
    expect(v1.voice).not.toBe(v2.voice);
    expect(a.heldNotes()).toEqual([60, 64]);
    expect(a.noteOff(60, 1, 0.5)).toEqual([v1.voice]);
    expect(a.heldNotes()).toEqual([64]);
    expect(a.activeVoiceCount(1.2)).toBe(2); // 60 still in its release tail
    expect(a.activeVoiceCount(1.6)).toBe(1);
  });

  it('re-triggering a held note reuses the same voice (no leak)', () => {
    const a = new PolyVoiceAllocator(8);
    const first = a.noteOn(60, 0);
    const again = a.noteOn(60, 0.1);
    expect(again.voice).toBe(first.voice);
    expect(again.retrigger).toBe(true);
    expect(a.poolSize).toBe(1);
    expect(a.noteOff(60, 0.2, 0.3)).toEqual([first.voice]);
    expect(a.heldNotes()).toEqual([]);
    expect(a.activeVoiceCount(1)).toBe(0);
  });

  it('re-triggering a note in its release tail reuses that voice', () => {
    const a = new PolyVoiceAllocator(8);
    const first = a.noteOn(60, 0);
    a.noteOff(60, 0.5, 2);
    const again = a.noteOn(60, 1);
    expect(again.voice).toBe(first.voice);
    expect(again.retrigger).toBe(true);
  });

  it('noteOff for an unknown note is a no-op', () => {
    const a = new PolyVoiceAllocator(4);
    a.noteOn(60, 0);
    expect(a.noteOff(61, 0.1, 0.2)).toEqual([]);
    expect(a.heldNotes()).toEqual([60]);
  });

  it('noteOff twice only releases once', () => {
    const a = new PolyVoiceAllocator(4);
    a.noteOn(60, 0);
    expect(a.noteOff(60, 0.1, 0.2)).toHaveLength(1);
    expect(a.noteOff(60, 0.2, 0.2)).toHaveLength(0);
  });

  it('releaseAll returns every busy voice and clears state', () => {
    const a = new PolyVoiceAllocator(8);
    a.noteOn(60, 0);
    a.noteOn(64, 0);
    a.noteOn(67, 0);
    a.noteOff(67, 0.1, 5); // still ringing
    const busy = a.releaseAll(0.2);
    expect(busy.sort()).toEqual([0, 1, 2]);
    expect(a.heldNotes()).toEqual([]);
    expect(a.activeVoiceCount(0.2)).toBe(0);
    // A late noteOff after releaseAll does nothing.
    expect(a.noteOff(60, 0.3, 0.2)).toEqual([]);
  });

  it('reuses free voices before growing the pool', () => {
    const a = new PolyVoiceAllocator(8);
    a.noteOn(60, 0);
    a.noteOff(60, 0.1, 0.1);
    const next = a.noteOn(62, 1);
    expect(next.voice).toBe(0);
    expect(next.stolen).toBe(false);
    expect(a.poolSize).toBe(1);
  });

  it('steals a releasing voice before a held one', () => {
    const a = new PolyVoiceAllocator(2);
    a.noteOn(60, 0); // voice 0 held
    a.noteOn(64, 0.1); // voice 1
    a.noteOff(64, 0.2, 5); // voice 1 releasing (long tail)
    const third = a.noteOn(67, 0.3);
    expect(third.voice).toBe(1);
    expect(third.stolen).toBe(true);
    expect(third.previousNote).toBe(64);
    expect(a.heldNotes()).toEqual([60, 67]);
  });

  it('steals the oldest held voice but spares the lowest (bass) note', () => {
    const a = new PolyVoiceAllocator(3);
    a.noteOn(48, 0); // bass, oldest
    a.noteOn(64, 0.1);
    a.noteOn(67, 0.2);
    const fourth = a.noteOn(72, 0.3);
    expect(fourth.stolen).toBe(true);
    expect(fourth.previousNote).toBe(64);
    expect(a.heldNotes()).toEqual([48, 67, 72]);
    // The stolen note's later noteOff must not release anything else.
    expect(a.noteOff(64, 0.4, 0.1)).toEqual([]);
    expect(a.heldNotes()).toEqual([48, 67, 72]);
  });

  it('never exceeds max polyphony', () => {
    const a = new PolyVoiceAllocator(4);
    for (let n = 0; n < 20; n++) a.noteOn(40 + n, n * 0.01);
    expect(a.poolSize).toBe(4);
    expect(a.heldNotes()).toHaveLength(4);
  });

  it('books scheduled one-shots against event time', () => {
    const a = new PolyVoiceAllocator(2);
    a.oneShot(60, 10, 1, 0.5); // busy until 11.5
    a.oneShot(62, 10, 1, 0.5);
    // At t=12 both are free again: no stealing.
    const later = a.oneShot(64, 12, 1, 0.5);
    expect(later.stolen).toBe(false);
    expect(a.activeVoiceCount(12.1)).toBe(1);
    expect(a.activeVoiceCount(14)).toBe(0);
  });

  it('silence() frees a voice after the fade', () => {
    const a = new PolyVoiceAllocator(2);
    a.noteOn(60, 0);
    a.silence(0, 1, 0.02);
    expect(a.activeVoiceCount(1.01)).toBe(1);
    expect(a.activeVoiceCount(1.03)).toBe(0);
  });
});

describe('MonoNoteStack', () => {
  it('first key attacks, release when all keys are up', () => {
    const s = new MonoNoteStack(true);
    expect(s.noteOn(60, 0.8)).toEqual({ type: 'attack', note: 60, velocity: 0.8, from: null });
    expect(s.noteOff(60)).toEqual({ type: 'release', note: 60 });
    expect(s.current()).toBeNull();
  });

  it('legato: overlapping key glides, releasing it falls back to the held key', () => {
    const s = new MonoNoteStack(true);
    s.noteOn(60, 0.8);
    expect(s.noteOn(64, 0.7)).toEqual({ type: 'legato', note: 64, velocity: 0.7, from: 60 });
    // Release the sounding top note -> glide back to 60 (still held).
    expect(s.noteOff(64)).toEqual({ type: 'legato', note: 60, velocity: 0.8, from: 64 });
    expect(s.noteOff(60)).toEqual({ type: 'release', note: 60 });
  });

  it('releasing a key that is not sounding changes nothing', () => {
    const s = new MonoNoteStack(true);
    s.noteOn(60, 0.8);
    s.noteOn(64, 0.8);
    expect(s.noteOff(60)).toEqual({ type: 'none' });
    expect(s.current()).toBe(64);
    // ...and the voice is released when the sounding key goes up.
    expect(s.noteOff(64)).toEqual({ type: 'release', note: 64 });
  });

  it('non-legato mode re-attacks each new key', () => {
    const s = new MonoNoteStack(false);
    s.noteOn(60, 0.8);
    expect(s.noteOn(64, 0.8)).toEqual({ type: 'attack', note: 64, velocity: 0.8, from: 60 });
  });

  it('re-pressing a held key moves it to the top without duplicates', () => {
    const s = new MonoNoteStack(true);
    s.noteOn(60, 0.8);
    s.noteOn(64, 0.8);
    s.noteOn(60, 0.9);
    expect(s.heldNotes()).toEqual([64, 60]);
    expect(s.noteOff(60)).toMatchObject({ type: 'legato', note: 64 });
    expect(s.noteOff(64)).toMatchObject({ type: 'release' });
  });

  it('releaseAll clears the stack; stray noteOffs are ignored', () => {
    const s = new MonoNoteStack(true);
    s.noteOn(60, 0.8);
    s.noteOn(62, 0.8);
    s.releaseAll();
    expect(s.heldNotes()).toEqual([]);
    expect(s.noteOff(62)).toEqual({ type: 'none' });
  });
});
