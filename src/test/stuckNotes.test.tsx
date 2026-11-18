/**
 * Stuck-note regression suite ("the note gets stuck, I can't stop hearing it").
 *
 * Every test drives the real note-handling layer (useMidiNotes, MidiService
 * dispatch, PianoRoll) against a fake audio engine with worst-case
 * Tone.PolySynth voice semantics (see fakeAudioEngine.ts: each noteOn
 * allocates a voice, each noteOff frees only one) and asserts that no voice
 * is left sounding.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, renderHook, screen } from '@testing-library/react';
import { useEffect } from 'react';
import { fakeAudio, fakeAudioEngine, fakeDrumEngine, resetFakeEngines } from '@/test/fakeAudioEngine';
import { useMidiNotes, CC_SUSTAIN, CC_ALL_NOTES_OFF } from '@/hooks/useMidiNotes';
import { useMidi } from '@/hooks/useMidi';
import { midiService } from '@/services/MidiService';
import { PianoRoll } from '@/components/PianoRoll';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());
vi.mock('@/services/ArrangementEngine', async () =>
  (await import('@/test/fakeAudioEngine')).arrangementEngineModule(),
);

beforeEach(() => {
  resetFakeEngines();
  vi.useRealTimers();
});

const silent = () => expect(fakeAudio.sounding()).toEqual([]);

describe('useMidiNotes: one voice per pitch, every voice released', () => {
  it('duplicate MIDI note-on for a held key does not leak a voice', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => {
      result.current.callbacks.onNoteOn(60, 0.8, 1);
      result.current.callbacks.onNoteOn(60, 0.8, 1); // e.g. listener attached twice
    });
    expect(fakeAudio.sounding()).toEqual([60]);
    act(() => result.current.callbacks.onNoteOff(60, 1));
    silent();
  });

  it('MIDI key + on-screen key on the same pitch share one voice', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => result.current.callbacks.onNoteOn(64, 0.8, 1));
    act(() => result.current.noteOn(64, 0.7)); // mouse on the same key
    act(() => result.current.noteOff(64));
    act(() => result.current.callbacks.onNoteOff(64, 1));
    silent();
  });

  it('note-on with velocity 0 is a note-off', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => result.current.callbacks.onNoteOn(60, 0.8, 1));
    act(() => result.current.callbacks.onNoteOn(60, 0, 1));
    silent();
    expect(result.current.activeNotes.size).toBe(0);
  });

  it('changing routing mode while a key is held still releases it', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => result.current.setRoutingMode('keys'));
    act(() => result.current.callbacks.onNoteOn(36, 0.8, 1)); // C2 -> synth in keys mode
    act(() => result.current.setRoutingMode('auto')); // C2 is a drum note in auto mode
    act(() => result.current.callbacks.onNoteOff(36, 1));
    silent();
  });

  it('toggling mute (keyboard sound source) while a key is held still releases it', () => {
    const { result, rerender } = renderHook(({ muted }) => useMidiNotes({ muted }), {
      initialProps: { muted: false },
    });
    act(() => result.current.callbacks.onNoteOn(60, 0.8, 1));
    rerender({ muted: true });
    act(() => result.current.callbacks.onNoteOff(60, 1));
    silent();
  });

  it('on-screen drum pad in "keys" routing hits the drum engine, never the synth', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => result.current.setRoutingMode('keys'));
    // App.handleDrumHit used to call noteOn(DRUM_SOUNDS[pad].midiNote) with no note-off ever.
    act(() => result.current.drumHit(0, 0.8));
    silent();
    expect(fakeDrumEngine.hit).toHaveBeenCalledWith(0, 0.8);
  });

  it('on-screen piano keys in the drum range play the synth (not drums) and release', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => result.current.noteOn(36, 0.7));
    expect(fakeAudio.sounding()).toEqual([36]);
    expect(fakeDrumEngine.hit).not.toHaveBeenCalled();
    act(() => result.current.noteOff(36));
    silent();
  });

  it('sustain pedal holds released notes until pedal up, and re-strikes do not stack', () => {
    const { result } = renderHook(() => useMidiNotes());
    const cb = () => result.current.callbacks;
    act(() => cb().onControlChange(CC_SUSTAIN, 127));
    act(() => cb().onNoteOn(60, 0.8, 1));
    act(() => cb().onNoteOff(60, 1));
    expect(fakeAudio.sounding()).toEqual([60]); // ringing on the pedal
    expect(result.current.activeNotes.size).toBe(0); // but the key is up
    act(() => cb().onNoteOn(60, 0.8, 1)); // re-strike while sustained
    act(() => cb().onNoteOff(60, 1));
    act(() => cb().onNoteOn(64, 0.8, 1)); // still held at pedal-up
    expect(fakeAudio.sounding()).toEqual([60, 64]);
    act(() => cb().onControlChange(CC_SUSTAIN, 0));
    expect(fakeAudio.sounding()).toEqual([64]); // pedal up releases only pedal-held notes
    act(() => cb().onNoteOff(64, 1));
    silent();
  });

  it('MIDI "All Notes Off" (CC123) and allNotesOff() silence everything, later note-offs are harmless', () => {
    const { result } = renderHook(() => useMidiNotes());
    act(() => {
      result.current.callbacks.onNoteOn(60, 0.8, 1);
      result.current.callbacks.onNoteOn(64, 0.8, 1);
    });
    act(() => result.current.callbacks.onControlChange(CC_ALL_NOTES_OFF, 0));
    silent();
    expect(fakeAudioEngine.releaseAll).toHaveBeenCalled();
    expect(result.current.activeNotes.size).toBe(0);
    act(() => result.current.callbacks.onNoteOff(60, 1));
    act(() => result.current.callbacks.onNoteOn(67, 0.8, 1));
    act(() => result.current.allNotesOff());
    silent();
    expect(result.current.getSoundingNotes()).toEqual([]);
  });

  it('unmounting the note owner releases held notes', () => {
    const { result, unmount } = renderHook(() => useMidiNotes());
    act(() => result.current.callbacks.onNoteOn(60, 0.8, 1));
    unmount();
    silent();
  });
});

describe('MidiService dispatch', () => {
  it('simulated note-on velocity 0 is dispatched as note-off', () => {
    const { result } = renderHook(() => {
      const notes = useMidiNotes();
      useMidi(notes.callbacks);
      return notes;
    });
    act(() => midiService.simulateNoteOn(62, 0.8));
    act(() => midiService.simulateNoteOn(62, 0));
    silent();
    expect(result.current.activeNotes.size).toBe(0);
  });

  // Electron bug fixed in both apps (user decision 2026-11-18): losing an input released its
  // held keys but left its pedal down, so the notes it sustained rang on.
  it('losing the input lifts its sustain pedal and releases the sustained notes', () => {
    const { result } = renderHook(() => {
      const notes = useMidiNotes();
      useMidi(notes.callbacks);
      return notes;
    });
    act(() => midiService.simulateControlChange(CC_SUSTAIN, 127));
    act(() => midiService.simulateNoteOn(60, 0.8));
    act(() => midiService.simulateNoteOff(60));
    act(() => midiService.simulateNoteOn(64, 0.8));
    expect(fakeAudio.sounding()).toEqual([60, 64]);
    expect(result.current.sustainOn).toBe(true);
    act(() => midiService.detachForReload()); // the input is lost (switch / unplug / dispose)
    silent();
    expect(result.current.sustainOn).toBe(false);
    // the next note is not sustained
    act(() => midiService.simulateNoteOn(67, 0.8));
    act(() => midiService.simulateNoteOff(67));
    silent();
  });
});

let harnessNotes: ReturnType<typeof useMidiNotes> | null = null;
/** Expose the harness' note owner to the test body (after render, not during). */
function useExpose(notes: ReturnType<typeof useMidiNotes>) {
  useEffect(() => {
    harnessNotes = notes;
  });
}

// ---------------------------------------------------------------------------
// On-screen keyboard pointer handling
// ---------------------------------------------------------------------------

describe('PianoRoll pointer handling', () => {
  function KeyboardHarness() {
    const notes = useMidiNotes();
    useExpose(notes);
    return <PianoRoll activeNotes={notes.activeNotes} onNoteOn={notes.noteOn} onNoteOff={notes.noteOff} compact />;
  }
  const keyEl = (midi: number) => document.querySelector(`[data-midi="${midi}"]`) as HTMLElement;
  const keyboard = () => screen.getByRole('group', { name: /playable piano keyboard/i });

  it('pointer up anywhere releases the key', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, buttons: 1, pointerType: 'mouse' });
    expect(fakeAudio.sounding()).toEqual([60]);
    fireEvent.pointerUp(window, { pointerId: 1 });
    silent();
  });

  it('sliding across keys keeps exactly one voice and releases on leaving the keyboard', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, buttons: 1, pointerType: 'mouse' });
    fireEvent.pointerEnter(keyEl(62), { pointerId: 1, buttons: 1, pointerType: 'mouse' });
    fireEvent.pointerEnter(keyEl(64), { pointerId: 1, buttons: 1, pointerType: 'mouse' });
    expect(fakeAudio.sounding()).toEqual([64]);
    fireEvent.pointerLeave(keyboard(), { pointerId: 1 });
    silent();
  });

  it('pointercancel, window blur and hidden tab release held keys', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, pointerType: 'touch' });
    fireEvent.pointerCancel(window, { pointerId: 1 });
    silent();
    fireEvent.pointerDown(keyEl(62), { pointerId: 2, button: 0, pointerType: 'mouse' });
    fireEvent.blur(window);
    silent();
    fireEvent.pointerDown(keyEl(64), { pointerId: 3, button: 0, pointerType: 'mouse' });
    Object.defineProperty(document, 'visibilityState', { value: 'hidden', configurable: true });
    fireEvent(document, new Event('visibilitychange'));
    Object.defineProperty(document, 'visibilityState', { value: 'visible', configurable: true });
    silent();
  });

  it('entering a key with the mouse button already up releases instead of playing', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, buttons: 1, pointerType: 'mouse' });
    // pointerup happened outside the window and never reached us
    fireEvent.pointerEnter(keyEl(62), { pointerId: 1, buttons: 0, pointerType: 'mouse' });
    silent();
  });

  it('right-click does not play a note', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 2, buttons: 2, pointerType: 'mouse' });
    silent();
  });

  it('unmounting mid-press releases the key', () => {
    const { unmount } = render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, pointerType: 'mouse' });
    // Keep the note owner alive: only the keyboard goes away.
    const owner = harnessNotes!;
    unmount();
    silent();
    expect(owner.getSoundingNotes()).toEqual([]);
  });

  it('two fingers on the same key: lifting one keeps it, lifting both releases', () => {
    render(<KeyboardHarness />);
    fireEvent.pointerDown(keyEl(60), { pointerId: 1, button: 0, pointerType: 'touch' });
    fireEvent.pointerDown(keyEl(60), { pointerId: 2, button: 0, pointerType: 'touch' });
    fireEvent.pointerUp(window, { pointerId: 1 });
    expect(fakeAudio.sounding()).toEqual([60]);
    fireEvent.pointerUp(window, { pointerId: 2 });
    silent();
  });
});
