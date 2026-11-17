import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { fakeAudio, fakeArrangementEngine, resetFakeEngines } from '@/test/fakeAudioEngine';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { controllerMapping, createDefaultProfile } from '@/services/ControllerMapping';
import { noteEventBus, type NoteEvent } from '@/services/NoteEventBus';
import { noteRepeat } from '@/services/NoteRepeat';

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

function setup() {
  const events: NoteEvent[] = [];
  const off = noteEventBus.subscribe((e) => events.push(e));
  const hook = renderHook(() => useMidiNotes());
  return { ...hook, events, off, cb: () => hook.result.current.callbacks };
}

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
