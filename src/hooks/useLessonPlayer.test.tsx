import { beforeEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { useLessonPlayer, resumeIndex, TOGETHER_WINDOW_MS } from '@/hooks/useLessonPlayer';
import { noteEventBus } from '@/services/NoteEventBus';
import type { Lesson, LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';

vi.mock('@/services/AudioEngine', async () => (await import('@/test/fakeAudioEngine')).audioEngineModule());

let clock = 1000;
const at = (ms: number) => (clock += ms);
const on = (note: number, dt = 10) =>
  act(() => noteEventBus.emit({ type: 'noteon', note, velocity: 0.8, channel: 1, time: at(dt) }));
const off = (note: number, dt = 10) => act(() => noteEventBus.emit({ type: 'noteoff', note, channel: 1, time: at(dt) }));
const tap = (note: number, dt = 10) => {
  on(note, dt);
  off(note);
};
const pad = (padId: DrumPadId, dt = 10) =>
  act(() => noteEventBus.emit({ type: 'drum', padId, note: 36, velocity: 0.8, time: at(dt) }));

function lessonOf(...steps: LessonStep[]): Lesson {
  return {
    id: `test-${Math.random()}`,
    title: 'Test',
    category: 'piano',
    difficulty: 'beginner',
    estimatedMinutes: 1,
    description: '',
    steps,
  };
}
const info: LessonStep = { type: 'info', title: 'Info', description: 'hi' };

function setup(lesson: Lesson) {
  const onPanic = vi.fn();
  const hook = renderHook(() => useLessonPlayer({ onPanic }));
  act(() => hook.result.current.startLesson(lesson));
  return { ...hook, onPanic };
}

beforeEach(() => {
  clock = 1000;
});

describe('useLessonPlayer step logic', () => {
  it('info steps complete on arrival', () => {
    const { result } = setup(lessonOf(info, info));
    expect(result.current.stepCompleted).toBe(true);
    act(() => result.current.nextStep());
    expect(result.current.currentStepIndex).toBe(1);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-notes (together) completes only after the keys are released', () => {
    const { result } = setup(
      lessonOf({ type: 'play-notes', title: 'CE', description: '', expectedNotes: [{ midi: 60 }, { midi: 64 }] }),
    );
    on(60);
    on(64);
    expect(result.current.stepCompleted).toBe(false);
    expect(result.current.pendingRelease).toBe(true);
    off(60);
    expect(result.current.stepCompleted).toBe(false);
    off(64);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-notes (together) accepts quick successive clicks from the one-pointer on-screen keyboard', () => {
    const { result } = setup(
      lessonOf({ type: 'play-notes', title: 'CE', description: '', expectedNotes: [{ midi: 60 }, { midi: 64 }] }),
    );
    tap(60);
    tap(64, 300);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-notes (together) does not accept notes struck too far apart', () => {
    const { result } = setup(
      lessonOf({ type: 'play-notes', title: 'CE', description: '', expectedNotes: [{ midi: 60 }, { midi: 64 }] }),
    );
    tap(60);
    tap(64, TOGETHER_WINDOW_MS + 100);
    expect(result.current.stepCompleted).toBe(false);
  });

  it('exact octave by default, any octave when anyOctave is set', () => {
    const exact = setup(lessonOf({ type: 'play-notes', title: 'C4', description: '', expectedNotes: [{ midi: 60 }] }));
    tap(72);
    expect(exact.result.current.stepCompleted).toBe(false);
    tap(60);
    expect(exact.result.current.stepCompleted).toBe(true);
    exact.unmount();

    const any = setup(
      lessonOf({ type: 'play-notes', title: 'F#', description: '', expectedNotes: [{ midi: 66 }], anyOctave: true }),
    );
    tap(78);
    expect(any.result.current.stepCompleted).toBe(true);
  });

  it('play-notes (sequence) follows a melody with repeats, ignoring wrong notes', () => {
    const melody = [55, 55, 57, 55, 60, 59];
    const { result } = setup(
      lessonOf({
        type: 'play-notes',
        title: 'Happy',
        description: '',
        noteMode: 'sequence',
        expectedNotes: melody.map((midi) => ({ midi })),
      }),
    );
    tap(55);
    tap(56); // slip
    tap(55);
    expect(result.current.sequenceIndex).toBe(2);
    tap(57);
    tap(55);
    tap(60);
    expect(result.current.stepCompleted).toBe(false);
    on(59);
    expect(result.current.sequenceIndex).toBe(6);
    expect(result.current.stepCompleted).toBe(false); // waits for release
    off(59);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-notes (sequence) does not complete when notes come out of order', () => {
    const { result } = setup(
      lessonOf({
        type: 'play-notes',
        title: 'EGA',
        description: '',
        noteMode: 'sequence',
        expectedNotes: [{ midi: 64 }, { midi: 67 }, { midi: 69 }],
      }),
    );
    tap(69);
    tap(67);
    tap(64);
    expect(result.current.stepCompleted).toBe(false);
    expect(result.current.sequenceIndex).toBe(1);
  });

  it('play-chord accepts any inversion/octave but not extra notes', () => {
    const { result } = setup(
      lessonOf({ type: 'play-chord', title: 'C', description: '', expectedChord: { root: 'C', quality: 'maj', display: 'C' } }),
    );
    on(60);
    on(64);
    on(70); // C E Bb ...
    on(67); // ... + G = C7, not C
    for (const n of [60, 64, 70, 67]) off(n);
    expect(result.current.stepCompleted).toBe(false);
    on(52, TOGETHER_WINDOW_MS + 10); // E3
    on(55); // G3
    on(60); // C4 -> first inversion
    off(52);
    off(55);
    off(60);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-scale requires the degrees in order (any octave); a double tap is forgiven', () => {
    const { result } = setup(
      lessonOf({
        type: 'play-scale',
        title: 'C major',
        description: '',
        expectedScale: { root: 0, name: 'Major', notes: [0, 2, 4, 5, 7, 9, 11, 0] },
      }),
    );
    for (const n of [60, 62, 64, 64, 65]) tap(n); // E double-tapped
    expect(result.current.sequenceIndex).toBe(4);
    tap(69); // skipped G -> restart
    expect(result.current.sequenceIndex).toBe(0);
    for (const n of [48, 50, 52, 53, 55, 57, 59]) tap(n); // C3 octave
    expect(result.current.stepCompleted).toBe(false);
    tap(60);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('play-drums: single pad, pads "together" within the window, MIDI pad notes routed as keys', () => {
    const { result } = setup(
      lessonOf(
        { type: 'play-drums', title: 'Kick', description: '', expectedDrumPattern: { pads: [0] } },
        { type: 'play-drums', title: 'Kick+HH', description: '', expectedDrumPattern: { pads: [0, 2] } },
        { type: 'play-drums', title: 'Snare (keys routing)', description: '', expectedDrumPattern: { pads: [1] } },
      ),
    );
    pad(1);
    expect(result.current.stepCompleted).toBe(false);
    pad(0);
    expect(result.current.stepCompleted).toBe(true);
    act(() => result.current.nextStep());
    pad(0);
    pad(2, 2000); // too late
    expect(result.current.stepCompleted).toBe(false);
    pad(0, 200);
    expect(result.current.stepCompleted).toBe(true);
    act(() => result.current.nextStep());
    tap(38); // snare note arriving as a key (routing mode "keys")
    expect(result.current.stepCompleted).toBe(true);
  });

  it('quiz and listen steps complete only on a correct answer', () => {
    const { result } = setup(
      lessonOf({
        type: 'quiz',
        title: 'Q',
        description: '',
        quizOptions: [{ label: 'a', correct: false }, { label: 'b', correct: true }],
      }),
    );
    act(() => result.current.completeQuiz(false));
    expect(result.current.stepCompleted).toBe(false);
    act(() => result.current.completeQuiz(true));
    expect(result.current.stepCompleted).toBe(true);
  });

  it('Next is a no-op until the step is completed', () => {
    const { result } = setup(
      lessonOf({ type: 'play-notes', title: 'C', description: '', expectedNotes: [{ midi: 60 }] }, info),
    );
    act(() => result.current.nextStep());
    expect(result.current.currentStepIndex).toBe(0);
  });

  it('every navigation / finish / exit / unmount silences live notes', () => {
    const lesson = lessonOf(info, info);
    const { result, onPanic, unmount } = setup(lesson);
    const calls = () => onPanic.mock.calls.length;
    const start = calls();
    act(() => result.current.nextStep());
    expect(calls()).toBe(start + 1);
    act(() => result.current.prevStep());
    expect(calls()).toBe(start + 2);
    act(() => result.current.exitLesson());
    expect(calls()).toBe(start + 3);
    act(() => result.current.startLesson(lesson));
    expect(result.current.currentStepIndex).toBe(1); // resumes at the last unfinished step
    const beforeFinish = calls();
    act(() => result.current.nextStep()); // finish
    expect(calls()).toBe(beforeFinish + 1);
    expect(result.current.activeLesson).toBeNull();
    expect(result.current.lastCompletedLessonId).toBe(lesson.id);
    expect(result.current.isLessonCompleted(lesson.id)).toBe(true);
    act(() => result.current.startLesson(lesson));
    const beforeUnmount = calls();
    unmount();
    expect(calls()).toBe(beforeUnmount + 1);
  });

  it('a step stays completed when revisited with Back in the same run', () => {
    const { result } = setup(
      lessonOf({ type: 'play-notes', title: 'C', description: '', expectedNotes: [{ midi: 60 }] }, info),
    );
    tap(60);
    act(() => result.current.nextStep());
    act(() => result.current.prevStep());
    expect(result.current.currentStepIndex).toBe(0);
    expect(result.current.stepCompleted).toBe(true);
  });

  it('progress persists: resume at the first unfinished step; a finished lesson restarts fresh', () => {
    const lesson = lessonOf(
      info,
      { type: 'play-notes', title: 'C', description: '', expectedNotes: [{ midi: 60 }] },
      { type: 'play-notes', title: 'D', description: '', expectedNotes: [{ midi: 62 }] },
    );
    const first = setup(lesson);
    act(() => first.result.current.nextStep());
    tap(60);
    first.unmount(); // quit mid-lesson

    const second = setup(lesson);
    expect(second.result.current.currentStepIndex).toBe(2);
    expect(second.result.current.stepCompleted).toBe(false);
    tap(62);
    act(() => second.result.current.nextStep()); // finish
    second.unmount();

    const third = setup(lesson);
    expect(third.result.current.isLessonCompleted(lesson.id)).toBe(true);
    expect(third.result.current.currentStepIndex).toBe(0);
    act(() => third.result.current.nextStep());
    expect(third.result.current.stepCompleted).toBe(false); // redo for real
  });
});

describe('resumeIndex', () => {
  const lesson = lessonOf(info, info, info);
  it('starts at the first incomplete step', () => {
    expect(resumeIndex(lesson, [], false)).toBe(0);
    expect(resumeIndex(lesson, [0, 1], false)).toBe(2);
    expect(resumeIndex(lesson, [0, 2], false)).toBe(1);
    expect(resumeIndex(lesson, [0, 1, 2], false)).toBe(2);
    expect(resumeIndex(lesson, [0, 1, 2], true)).toBe(0);
  });
});
