/**
 * Fake `audioEngine` / `drumEngine` / `arrangementEngine` singletons for tests
 * of the note-handling layer (useMidiNotes, MidiService, PianoRoll).
 *
 * The fake audio engine deliberately models the WORST case of a Tone.PolySynth:
 * every noteOn allocates a new voice (even for a pitch that is already
 * sounding) and every noteOff releases only one voice of that pitch. A layer
 * that double-attacks or forgets a noteOff therefore shows up as a leaked
 * voice in `fakeAudio.sounding()` -- i.e. exactly the "stuck note" users hear.
 * `releaseAll()` silences everything.
 *
 * Usage (in a test file):
 *   vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
 *   vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());
 *   vi.mock('@/services/ArrangementEngine', async () => (await import('@/test/fakeAudioEngine')).arrangementEngineModule());
 *   import { fakeAudio, resetFakeEngines } from '@/test/fakeAudioEngine';
 *   beforeEach(resetFakeEngines);
 */
import { vi } from 'vitest';
import { DEFAULT_EFFECT_PARAMS } from '@/types/effects';

interface FakeState {
  voices: number[];
  log: string[];
  drumHits: { padId: number; velocity: number }[];
  arrangementState: 'stopped' | 'playing' | 'recording';
}

// On globalThis so vi.resetModules() does not create a second copy.
const g = globalThis as typeof globalThis & { __fakeEngineState?: FakeState };
const state: FakeState = (g.__fakeEngineState ??= {
  voices: [],
  log: [],
  drumHits: [],
  arrangementState: 'stopped',
});

function createFakeAudioEngine() {
  return {
    start: vi.fn(async () => undefined),
    isStarted: vi.fn(() => true),
    noteOn: vi.fn((midi: number, velocity: number) => {
      state.voices.push(midi);
      state.log.push(`on:${midi}:${velocity}`);
    }),
    noteOff: vi.fn((midi: number) => {
      const i = state.voices.indexOf(midi);
      if (i >= 0) state.voices.splice(i, 1);
      state.log.push(`off:${midi}`);
    }),
    releaseAll: vi.fn(() => {
      state.voices.length = 0;
      state.log.push('releaseAll');
    }),
    mute: vi.fn(),
    unmute: vi.fn(),
    setVolume: vi.fn(),
    getVolume: vi.fn(() => -6),
    setPreset: vi.fn(),
    getPresetIndex: vi.fn(() => 0),
    getEffectParams: vi.fn(() => ({ ...DEFAULT_EFFECT_PARAMS })),
    setReverbWet: vi.fn(),
    setChorusDepth: vi.fn(),
    setFilterCutoff: vi.fn(),
    setFilterResonance: vi.fn(),
  };
}

function createFakeDrumEngine() {
  return {
    start: vi.fn(async () => undefined),
    hit: vi.fn((padId: number, velocity: number) => {
      state.drumHits.push({ padId, velocity });
    }),
    mute: vi.fn(),
    unmute: vi.fn(),
  };
}

function createFakeArrangementEngine() {
  return {
    getState: vi.fn(() => state.arrangementState),
    captureNoteOn: vi.fn(),
    captureNoteOff: vi.fn(),
    stop: vi.fn(() => {
      state.arrangementState = 'stopped';
    }),
  };
}

type FakeAudioEngine = ReturnType<typeof createFakeAudioEngine>;
type FakeDrumEngine = ReturnType<typeof createFakeDrumEngine>;
type FakeArrangementEngine = ReturnType<typeof createFakeArrangementEngine>;

const engines = g as typeof g & {
  __fakeAudioEngine?: FakeAudioEngine;
  __fakeDrumEngine?: FakeDrumEngine;
  __fakeArrangementEngine?: FakeArrangementEngine;
};
export const fakeAudioEngine: FakeAudioEngine = (engines.__fakeAudioEngine ??= createFakeAudioEngine());
export const fakeDrumEngine: FakeDrumEngine = (engines.__fakeDrumEngine ??= createFakeDrumEngine());
export const fakeArrangementEngine: FakeArrangementEngine = (engines.__fakeArrangementEngine ??=
  createFakeArrangementEngine());

export const fakeAudio = {
  /** MIDI numbers of voices still sounding (duplicates = leaked extra voices). */
  sounding: (): number[] => [...state.voices].sort((a, b) => a - b),
  log: (): string[] => [...state.log],
  drumHits: () => [...state.drumHits],
  setArrangementState: (s: FakeState['arrangementState']) => {
    state.arrangementState = s;
  },
};

export function resetFakeEngines(): void {
  state.voices.length = 0;
  state.log.length = 0;
  state.drumHits.length = 0;
  state.arrangementState = 'stopped';
  for (const engine of [fakeAudioEngine, fakeDrumEngine, fakeArrangementEngine]) {
    for (const fn of Object.values(engine)) {
      if (typeof fn === 'function' && 'mockClear' in fn) (fn as { mockClear: () => void }).mockClear();
    }
  }
}

export const audioEngineModule = () => ({ audioEngine: fakeAudioEngine });
export const drumEngineModule = () => ({ drumEngine: fakeDrumEngine });
export const arrangementEngineModule = () => ({ arrangementEngine: fakeArrangementEngine });
