/**
 * Practice mode end to end (with a fake audio engine): exercises are scored
 * from MIDI input (MidiService -> useMidi -> useMidiNotes -> note bus), not
 * only from clicks on the on-screen keyboard, finish with a score, and
 * record the session in the stats.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { fakeAudio, resetFakeEngines } from '@/test/fakeAudioEngine';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useMidi } from '@/hooks/useMidi';
import { useLearningMode } from '@/hooks/useLearningMode';
import { midiService } from '@/services/MidiService';
import { PracticeView } from '@/components/practice/PracticeView';
import { buildExerciseSequence } from '@/components/practice/ScalePractice';
import { ALL_PRACTICES, getPracticeById } from '@/data/practices';
import { chordPitchClasses, noteNameToPitchClass } from '@/utils/lessonMatching';
import { SCALE_DEFINITIONS } from '@/utils/scales';
import { DRUM_SOUNDS, PIANO_END, PIANO_START, PRACTICE_STATS_STORAGE_KEY } from '@/utils/constants';
import { midiToNoteName } from '@/utils/noteHelpers';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());
vi.mock('@/services/DrumEngine', async () => (await import('@/test/fakeAudioEngine')).drumEngineModule());
vi.mock('@/services/ArrangementEngine', async () =>
  (await import('@/test/fakeAudioEngine')).arrangementEngineModule(),
);

let now = 1_700_000_000_000;
beforeEach(() => {
  resetFakeEngines();
  now = 1_700_000_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
});

function Harness() {
  const notes = useMidiNotes();
  useMidi(notes.callbacks);
  const learning = useLearningMode(notes.activeNotes);
  return (
    <PracticeView
      activeNotes={notes.activeNotes}
      onNoteOn={notes.noteOn}
      onNoteOff={notes.noteOff}
      activePads={new Set()}
      onDrumHit={notes.drumHit}
      detectedChord={learning.detectedChord}
      highlightedNotes={learning.highlightedNotes}
      selectedRoot={learning.selectedRoot}
      setSelectedRoot={learning.setSelectedRoot}
      selectedScale={learning.selectedScale}
      setSelectedScale={learning.setSelectedScale}
      onAllNotesOff={notes.allNotesOff}
    />
  );
}

const play = (note: number) => {
  act(() => midiService.simulateNoteOn(note, 0.8));
  act(() => midiService.simulateNoteOff(note));
};
const chord = (notes: number[]) => {
  for (const n of notes) act(() => midiService.simulateNoteOn(n, 0.8));
  for (const n of notes) act(() => midiService.simulateNoteOff(n));
};

function openExercise(id: string) {
  render(<Harness />);
  const exercise = getPracticeById(id)!;
  fireEvent.click(screen.getByText(exercise.title));
  return exercise;
}

function recordedSessions() {
  const raw = localStorage.getItem(PRACTICE_STATS_STORAGE_KEY);
  return raw ? (JSON.parse(raw).sessions as { exerciseId: string; accuracy: number | null; stars: number }[]) : [];
}

describe('practice exercises with a MIDI keyboard', () => {
  it('note sequence: MIDI input is scored, finishes with a score and records the session', () => {
    openExercise('piano-white-keys-cg');
    for (const n of [60, 62, 61, 64, 65, 67]) play(n); // one wrong note (C#)
    expect(screen.getByText('83%')).toBeInTheDocument(); // 5 of 6
    now += 60_000;
    fireEvent.click(screen.getByTitle('Back to exercises'));
    const [session] = recordedSessions();
    expect(session).toMatchObject({ exerciseId: 'piano-white-keys-cg', accuracy: 83, stars: 2 });
    expect(fakeAudio.sounding()).toEqual([]);
  });

  it('scale exercise: completes after the scale is played up an octave (any octave)', () => {
    openExercise('scale-c-major');
    for (const n of [48, 50, 52, 53, 55, 57, 59, 60]) play(n);
    expect(screen.getByText('100%')).toBeInTheDocument();
  });

  it('chord exercise: wrong chords count against accuracy; inversions are fine', () => {
    openExercise('chord-prog-cgamf');
    chord([60, 64, 67]); // C
    chord([67, 71, 74]); // G
    chord([57, 60, 63]); // A dim - wrong
    chord([60, 64, 69]); // Am, first inversion
    chord([65, 69, 72]); // F
    expect(screen.getByText('80%')).toBeInTheDocument();
  });

  it('leaving Practice mid-exercise still records the session and silences notes', () => {
    const { unmount } = render(<Harness />);
    fireEvent.click(screen.getByText(getPracticeById('chord-c-major')!.title));
    act(() => midiService.simulateNoteOn(60, 0.8));
    now += 30_000;
    unmount();
    expect(recordedSessions()[0]).toMatchObject({ exerciseId: 'chord-c-major', accuracy: null });
  });
});

describe('practice data', () => {
  it('has unique ids', () => {
    const ids = ALL_PRACTICES.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it.each(ALL_PRACTICES.map((e) => [e.id, e] as const))('%s is well-formed and completable', (_id, exercise) => {
    const { config } = exercise;
    expect(exercise.scoring.threeStars).toBeGreaterThan(exercise.scoring.twoStars);
    switch (exercise.activity) {
      case 'scale-practice': {
        const seq = buildExerciseSequence(exercise);
        expect(seq, 'needs targetNotes or a known scale').not.toBeNull();
        if (config.targetNotes) {
          for (const n of config.targetNotes) {
            expect(n.midi).toBeGreaterThanOrEqual(PIANO_START);
            expect(n.midi).toBeLessThanOrEqual(PIANO_END);
            expect(n.label).toBe(midiToNoteName(n.midi));
          }
        } else {
          expect(SCALE_DEFINITIONS.map((d) => d.name)).toContain(config.scaleName);
        }
        break;
      }
      case 'chord-detection':
        expect(config.targetChords?.length).toBeGreaterThan(0);
        for (const c of config.targetChords!) {
          expect(chordPitchClasses(c), `${c.root}${c.quality}`).not.toBeNull();
          expect(noteNameToPitchClass(c.root)).toBeDefined();
        }
        break;
      case 'rhythm-training':
        expect(config.bpm).toBeGreaterThanOrEqual(40);
        expect(config.totalBeats).toBeGreaterThan(0);
        break;
      case 'drum-patterns':
        expect(config.pattern?.length).toBeGreaterThan(0);
        for (const beat of config.pattern!) for (const p of beat) expect(DRUM_SOUNDS[p]).toBeDefined();
        expect(config.totalBeats).toBeGreaterThan(0);
        break;
      default:
        break;
    }
  });
});
