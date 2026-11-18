import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { fakeAudio, fakeArrangementEngine, resetFakeEngines } from '@/test/fakeAudioEngine';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { controllerMapping, createDefaultProfile } from '@/services/ControllerMapping';
import { noteEventBus, type NoteEvent } from '@/services/NoteEventBus';
import { noteRepeat, repeatStepBeats } from '@/services/NoteRepeat';
import { toneMock } from '@/test/toneMock';

const drum = vi.hoisted(() => ({
  hit: vi.fn(),
  release: vi.fn(),
  start: vi.fn(async () => undefined),
}));

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
vi.mock('@/services/DrumEngine', () => ({ drumEngine: drum }));
vi.mock('@/services/ArrangementEngine', async () =>
  (await import('@/test/fakeAudioEngine')).arrangementEngineModule(),
);

beforeEach(() => {
  resetFakeEngines();
  drum.hit.mockClear();
  drum.release.mockClear();
  controllerMapping.cancelLearn();
  controllerMapping.setProfile(createDefaultProfile());
});

function setup(options: Parameters<typeof useMidiNotes>[0] = {}) {
  const events: NoteEvent[] = [];
  const off = noteEventBus.subscribe((e) => events.push(e));
  const hook = renderHook(() => useMidiNotes(options));
  return { ...hook, events, off, cb: () => hook.result.current.callbacks };
}

describe('useMidiNotes split routing', () => {
  // Electron bug fixed in both apps (user decision 2026-11-18): 'split' used to behave like 'auto'.
  it('notes below the split point trigger pads chromatically, the rest play the synth', () => {
    localStorage.removeItem('micro-daw:midi-split-point');
    const { cb, result } = setup();
    act(() => result.current.setRoutingMode('split'));
    expect(result.current.splitPoint).toBe(60);
    for (const [note, pad] of [[44, 0], [51, 7], [59, 15], [36, 8], [28, 0], [0, 4]]) {
      drum.hit.mockClear();
      act(() => cb().onNoteOn(note, 0.5, 1));
      expect(drum.hit).toHaveBeenCalledWith(pad, 0.5);
      act(() => cb().onNoteOff(note, 1));
    }
    act(() => cb().onNoteOn(60, 0.5, 10));
    act(() => cb().onNoteOn(84, 0.5, 1));
    expect(fakeAudio.sounding()).toEqual([60, 84]);
    // a held pad keeps its route when the split moves
    act(() => cb().onNoteOn(50, 0.5, 1));
    act(() => result.current.setSplitPoint(48));
    drum.hit.mockClear();
    act(() => cb().onNoteOn(47, 0.5, 1));
    expect(drum.hit).toHaveBeenCalledWith(15, 0.5);
    act(() => cb().onNoteOn(52, 0.5, 1));
    expect(fakeAudio.sounding()).toEqual([52, 60, 84]);
    act(() => cb().onNoteOff(50, 1));
    expect(drum.release).toHaveBeenLastCalledWith(6);
    // persisted, clamped
    expect(localStorage.getItem('micro-daw:midi-split-point')).toBe('48');
    act(() => result.current.setSplitPoint(500));
    expect(result.current.splitPoint).toBe(127);
    localStorage.removeItem('micro-daw:midi-split-point');
  });

  it('reads the stored split point at mount', () => {
    localStorage.setItem('micro-daw:midi-split-point', '52.9');
    const { result } = setup();
    expect(result.current.splitPoint).toBe(52);
    localStorage.setItem('micro-daw:midi-split-point', '"x"');
    expect(setup().result.current.splitPoint).toBe(60);
    localStorage.removeItem('micro-daw:midi-split-point');
  });
});

describe('useMidiNotes pad routing', () => {
  it('a mapped pad note hits the pad, emits a drum event and releases on note-off', () => {
    const { cb, events, off } = setup();
    act(() => cb().onNoteOn(38, 0.9, 10));
    expect(drum.hit).toHaveBeenCalledWith(2, 0.9);
    expect(fakeAudio.sounding()).toEqual([]);
    expect(events.at(-1)).toMatchObject({ type: 'drum', padId: 2, note: 38, velocity: 0.9 });

    act(() => cb().onNoteOff(38, 10));
    expect(drum.release).toHaveBeenCalledWith(2);
    off();
  });

  it('uses the controller profile, not GM numbers', () => {
    const p = createDefaultProfile();
    p.padMap.notes[0] = 60; // hardware pad 1 sends C4
    p.padMap.notes[15] = null;
    controllerMapping.setProfile(p);
    const { cb } = setup();
    act(() => cb().onNoteOn(60, 0.5, 10));
    expect(drum.hit).toHaveBeenCalledWith(0, 0.5);
    act(() => cb().onNoteOff(60, 10));
    expect(drum.release).toHaveBeenCalledWith(0);
  });

  it('unmapped channel-10 notes fall through to the synth instead of being dropped', () => {
    const { cb } = setup();
    act(() => cb().onNoteOn(70, 0.8, 10));
    expect(drum.hit).not.toHaveBeenCalled();
    expect(fakeAudio.sounding()).toEqual([70]);
    act(() => cb().onNoteOff(70, 10));
    expect(fakeAudio.sounding()).toEqual([]);
  });

  it('pad map channel filter sends other channels to the synth', () => {
    const p = createDefaultProfile();
    p.padMap.channel = 10;
    controllerMapping.setProfile(p);
    const { cb } = setup();
    act(() => cb().onNoteOn(48, 0.8, 1)); // keyboard key C3 on ch 1
    expect(drum.hit).not.toHaveBeenCalled();
    expect(fakeAudio.sounding()).toEqual([48]);
    act(() => cb().onNoteOff(48, 1));
    expect(fakeAudio.sounding()).toEqual([]);
  });

  it('"keys" routing sends mapped notes to the synth', () => {
    const { result, cb } = setup();
    act(() => result.current.setRoutingMode('keys'));
    act(() => cb().onNoteOn(36, 0.8, 10));
    expect(drum.hit).not.toHaveBeenCalled();
    act(() => cb().onNoteOff(36, 10));
    expect(fakeAudio.sounding()).toEqual([]);
  });

  it('a remap while a pad is held still releases the pad it hit', () => {
    const { cb } = setup();
    act(() => cb().onNoteOn(36, 0.8, 10));
    const p = createDefaultProfile();
    p.padMap.notes[0] = null;
    act(() => controllerMapping.setProfile(p));
    act(() => cb().onNoteOff(36, 10));
    expect(drum.release).toHaveBeenCalledWith(0);
    expect(fakeAudio.sounding()).toEqual([]);
  });

  it('records pad hits with padToNote while recording', () => {
    fakeAudio.setArrangementState('recording');
    const p = createDefaultProfile();
    p.padMap.notes[3] = 80;
    controllerMapping.setProfile(p);
    const { cb } = setup();
    act(() => cb().onNoteOn(80, 0.7, 10));
    act(() => cb().onNoteOff(80, 10));
    expect(fakeArrangementEngine.captureNoteOn).toHaveBeenCalledWith(39, 0.7, true);
    expect(fakeArrangementEngine.captureNoteOff).toHaveBeenCalledWith(39);
  });

  it('allNotesOff releases held pads', () => {
    const { result, cb } = setup();
    act(() => cb().onNoteOn(36, 0.8, 10));
    act(() => result.current.allNotesOff());
    expect(drum.release).toHaveBeenCalledWith(0);
    act(() => cb().onNoteOff(36, 10)); // late note-off is harmless
    expect(drum.release).toHaveBeenCalledTimes(1);
  });

  it('on-screen drumHit accepts all 16 pads', () => {
    const { result, events } = setup();
    act(() => result.current.drumHit(12, 0.6));
    expect(drum.hit).toHaveBeenCalledWith(12, 0.6);
    expect(events.at(-1)).toMatchObject({ type: 'drum', padId: 12, note: 48 });
    act(() => result.current.drumHit(16, 0.6));
    expect(drum.hit).toHaveBeenCalledTimes(1);
  });
});

describe('useMidiNotes learn consumption', () => {
  it('while pad-learning the hit is consumed (no sound) and its note-off swallowed', () => {
    const { cb } = setup();
    controllerMapping.startPadLearn(4);
    act(() => cb().onNoteOn(64, 0.8, 1));
    expect(drum.hit).not.toHaveBeenCalled();
    expect(fakeAudio.sounding()).toEqual([]);
    act(() => cb().onNoteOff(64, 1));
    expect(drum.release).not.toHaveBeenCalled();
    expect(fakeAudio.log()).toEqual([]);
    // Learned: the same note now hits pad 4.
    act(() => cb().onNoteOn(64, 0.8, 1));
    expect(drum.hit).toHaveBeenCalledWith(4, 0.8);
  });
});

describe('useMidiNotes CC dispatch', () => {
  it('mapped CCs go to controllerMapping knobs, unmapped ones keep the legacy effect path', async () => {
    const { audioEngine } = await import('@/services/AudioEngine');
    const knob = vi.fn();
    const off = controllerMapping.onKnob(knob);
    const { cb } = setup();

    act(() => cb().onControlChange(76, 127, 1)); // default: trackVolume
    expect(knob).toHaveBeenCalledWith({ kind: 'trackVolume' }, 1);
    expect(audioEngine.setVolume).not.toHaveBeenCalled();

    act(() => cb().onControlChange(5, 127, 1)); // legacy volume CC, unmapped in the profile
    expect(audioEngine.setVolume).toHaveBeenCalledWith(0);
    expect(knob).toHaveBeenCalledTimes(1);
    off();
  });

  it('knob learn consumes the next CC', () => {
    const knob = vi.fn();
    const off = controllerMapping.onKnob(knob);
    const { cb } = setup();
    controllerMapping.startKnobLearn({ kind: 'swing' });
    act(() => cb().onControlChange(21, 64, 1));
    expect(knob).not.toHaveBeenCalled();
    act(() => cb().onControlChange(21, 127, 1));
    expect(knob).toHaveBeenCalledWith({ kind: 'swing' }, 1);
    off();
  });

  it('sustain is never routed to knobs', () => {
    const knob = vi.fn();
    const off = controllerMapping.onKnob(knob);
    const { result, cb } = setup();
    controllerMapping.startKnobLearn({ kind: 'swing' });
    act(() => cb().onControlChange(64, 127, 1));
    expect(result.current.sustainOn).toBe(true);
    expect(controllerMapping.isLearning()).toBe(true);
    off();
  });
});

describe('useMidiNotes note repeat', () => {
  it('held pads (MIDI and on-screen) feed note repeat; one-shot hits do not', () => {
    const press = vi.spyOn(noteRepeat, 'press');
    const release = vi.spyOn(noteRepeat, 'release');
    const { cb, result } = setup();

    act(() => cb().onNoteOn(36, 100, 10));
    expect(press).toHaveBeenCalledWith(0, 100);
    act(() => cb().onNoteOff(36, 10));
    expect(release).toHaveBeenCalledWith(0);

    act(() => result.current.drumPress(5, 0.7));
    expect(press).toHaveBeenLastCalledWith(5, 0.7);
    act(() => result.current.drumRelease(5));
    expect(release).toHaveBeenLastCalledWith(5);
    expect(drum.release).toHaveBeenCalledWith(5);

    press.mockClear();
    act(() => result.current.drumHit(3, 0.5));
    expect(drum.hit).toHaveBeenLastCalledWith(3, 0.5);
    expect(press).not.toHaveBeenCalled();
    press.mockRestore();
    release.mockRestore();
  });

  it('scheduled retriggers play and record at their audio time; flash/event when heard', () => {
    vi.useFakeTimers();
    try {
      const { events, off } = setup();
      const trigger = (noteRepeat as unknown as { trigger: (pad: number, v: number, t: number) => void }).trigger;
      fakeAudio.setArrangementState('recording');
      toneMock.clock.now = 5;
      const before = events.length;
      act(() => trigger(6, 0.7, 5.08));
      expect(drum.hit).toHaveBeenCalledWith(6, 0.7, 5.08);
      expect(events.length).toBe(before); // not yet heard
      // Not recorded until heard (a release before then must be able to drop it).
      expect(fakeArrangementEngine.captureNoteOn).not.toHaveBeenCalled();
      act(() => {
        vi.advanceTimersByTime(81);
      });
      // Recorded at its audio time, with the repeat grid as a quantize hint.
      expect(fakeArrangementEngine.captureNoteOn).toHaveBeenCalledWith(42, 0.7, true, 5.08, {
        gridBeats: repeatStepBeats(noteRepeat.getRate()),
      });
      expect(events.at(-1)).toMatchObject({ type: 'drum', padId: 6, audioTime: 5.08 });
      off();
    } finally {
      vi.useRealTimers();
    }
  });

  it('release drops repeats not heard yet (no record, no flash) and settles the ones already due first', () => {
    vi.useFakeTimers();
    try {
      const flash = vi.fn();
      const { events, off, result } = setup({ onDrumFlash: flash });
      const trigger = (noteRepeat as unknown as { trigger: (pad: number, v: number, t: number) => void }).trigger;
      fakeAudio.setArrangementState('recording');
      toneMock.clock.now = 5;
      act(() => {
        trigger(2, 0.9, 5.03); // due before the release (its timer has not fired yet)
        trigger(2, 0.9, 5.09); // after the release: must never be recorded
        trigger(3, 0.5, 5.09); // another pad: unaffected
      });
      toneMock.clock.now = 5.05;
      act(() => result.current.drumRelease(2));
      const capture = fakeArrangementEngine.captureNoteOn as unknown as { mock: { calls: unknown[][] } };
      expect(capture.mock.calls.map((c) => [c[0], c[3]])).toEqual([[38, 5.03]]);
      // The heard repeat is captured before the note-off.
      const onOrder = (fakeArrangementEngine.captureNoteOn as unknown as { mock: { invocationCallOrder: number[] } }).mock.invocationCallOrder[0];
      const offOrder = (fakeArrangementEngine.captureNoteOff as unknown as { mock: { invocationCallOrder: number[] } }).mock.invocationCallOrder[0];
      expect(onOrder).toBeLessThan(offOrder);
      expect(drum.release).toHaveBeenCalledWith(2);
      act(() => {
        vi.advanceTimersByTime(200);
      });
      expect(capture.mock.calls.map((c) => [c[0], c[3]])).toEqual([[38, 5.03], [39, 5.09]]);
      expect(flash.mock.calls.map((c) => c[0])).toEqual([2, 3]);
      expect(events.filter((e) => e.type === 'drum' && e.padId === 2)).toHaveLength(1);
      off();
    } finally {
      vi.useRealTimers();
    }
  });

  it('keyboard press/release: release stops note repeat for the pad and releases its gate voices', () => {
    const { result } = setup();
    try {
      act(() => result.current.drumPress(7, 0.8));
      expect(noteRepeat.isHeld(7)).toBe(true);
      act(() => result.current.drumRelease(7));
      expect(noteRepeat.isHeld(7)).toBe(false);
      expect(noteRepeat.isRepeating(7)).toBe(false);
      expect(drum.release).toHaveBeenCalledWith(7);
      // A stray second release (keyup after blur) is harmless.
      act(() => result.current.drumRelease(7));
    } finally {
      noteRepeat.releaseAll();
    }
  });

  it('retriggers take the normal hit path (sound, event bus, loop recorder)', () => {
    const { events, off } = setup();
    const trigger = (noteRepeat as unknown as { trigger: (pad: number, v: number) => void }).trigger;
    fakeAudio.setArrangementState('recording');
    act(() => trigger(4, 0.6));
    expect(drum.hit).toHaveBeenCalledWith(4, 0.6);
    expect(fakeArrangementEngine.captureNoteOn).toHaveBeenCalledWith(40, 0.6, true);
    expect(events.at(-1)).toMatchObject({ type: 'drum', padId: 4, note: 40 });
    off();
  });
});
