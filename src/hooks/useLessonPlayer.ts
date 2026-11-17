import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useProgress } from './useProgress';
import { useNoteEvents } from './useNoteEvents';
import { audioPromptPlayer } from '@/services/AudioPromptPlayer';
import type { NoteEvent } from '@/services/NoteEventBus';
import type { Lesson, LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';
import { DRUM_NOTE_TO_PAD } from '@/utils/constants';
import {
  chordMatches,
  drumPadsSatisfied,
  expectedMidi,
  notesSatisfied,
  playNotesMode,
  samePitchClass,
  scaleProgress,
  type DrumHit,
} from '@/utils/lessonMatching';

/**
 * Notes struck within this window count as "together" for together-mode
 * play-notes and chord steps, so the on-screen keyboard (one pointer) can
 * complete them by clicking the notes in quick succession.
 */
export const TOGETHER_WINDOW_MS = 1500;
/** Pads hit within this window count as hit "together". */
export const DRUM_TOGETHER_WINDOW_MS = 600;
const MAX_HISTORY = 64;

interface UseLessonPlayerOptions {
  /**
   * Hard stop for live notes (useMidiNotes().allNotesOff). Called whenever the
   * player leaves a step: next/prev, exit, finishing the lesson, unmount.
   */
  onPanic?: () => void;
}

interface Machine {
  lesson: Lesson | null;
  index: number;
  completed: boolean;
  /** Condition met; completes as soon as every key is released. */
  pending: boolean;
  held: Set<number>;
  recent: { note: number; time: number }[];
  played: number[];
  /** Melody (sequence) steps: index of the next expected note. */
  cursor: number;
  drumHits: DrumHit[];
}

function stepAt(lesson: Lesson | null, index: number): LessonStep | null {
  return lesson?.steps[index] ?? null;
}

/** First step that still needs doing when resuming a lesson. */
export function resumeIndex(lesson: Lesson, completedSteps: number[], lessonCompleted: boolean): number {
  if (lessonCompleted) return 0;
  const done = new Set(completedSteps);
  const first = lesson.steps.findIndex((_, i) => !done.has(i));
  return first === -1 ? Math.max(0, lesson.steps.length - 1) : first;
}

export function useLessonPlayer(options: UseLessonPlayerOptions = {}) {
  const [activeLesson, setActiveLesson] = useState<Lesson | null>(null);
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [stepCompleted, setStepCompleted] = useState(false);
  const [pendingRelease, setPendingRelease] = useState(false);
  /** Notes/degrees matched so far for sequence + scale steps. */
  const [sequenceIndex, setSequenceIndex] = useState(0);
  const [lastCompletedLessonId, setLastCompletedLessonId] = useState<string | null>(null);
  const { markStepCompleted, markLessonCompleted, isLessonCompleted, getLessonProgress } = useProgress();

  const { onPanic } = options;
  const onPanicRef = useRef(onPanic);
  useLayoutEffect(() => {
    onPanicRef.current = onPanic;
  }, [onPanic]);

  /** Steps completed in this run of the lesson (gates the Next button). */
  const sessionDoneRef = useRef<Set<number>>(new Set());
  const machineRef = useRef<Machine>({
    lesson: null,
    index: 0,
    completed: false,
    pending: false,
    held: new Set(),
    recent: [],
    played: [],
    cursor: 0,
    drumHits: [],
  });

  const currentStep: LessonStep | null = stepAt(activeLesson, currentStepIndex);

  const completeStep = useCallback(() => {
    const m = machineRef.current;
    if (!m.lesson || m.completed) return;
    m.completed = true;
    m.pending = false;
    sessionDoneRef.current.add(m.index);
    setStepCompleted(true);
    setPendingRelease(false);
    markStepCompleted(m.lesson.id, m.index);
  }, [markStepCompleted]);

  /** Stop anything this lesson made audible (prompt + live notes). */
  const silence = useCallback(() => {
    audioPromptPlayer.stop();
    onPanicRef.current?.();
  }, []);

  /** Point the machine at a step (no audio side effects). */
  const enterStep = useCallback((lesson: Lesson | null, index: number) => {
    const m = machineRef.current;
    m.lesson = lesson;
    m.index = index;
    m.pending = false;
    m.recent = [];
    m.played = [];
    m.cursor = 0;
    m.drumHits = [];
    const step = stepAt(lesson, index);
    m.completed = !!lesson && sessionDoneRef.current.has(index);
    setCurrentStepIndex(index);
    setPendingRelease(false);
    setSequenceIndex(0);
    setStepCompleted(m.completed);
    if (lesson && step?.type === 'info' && !m.completed) completeStep();
  }, [completeStep]);

  const startLesson = useCallback((lesson: Lesson) => {
    silence();
    const progress = getLessonProgress(lesson.id);
    const completed = progress?.completed ?? false;
    // Redoing a finished lesson starts a fresh run; resuming keeps what's done.
    sessionDoneRef.current = new Set(completed ? [] : (progress?.completedSteps ?? []));
    setLastCompletedLessonId(null);
    setActiveLesson(lesson);
    enterStep(lesson, resumeIndex(lesson, progress?.completedSteps ?? [], completed));
  }, [getLessonProgress, enterStep, silence]);

  const exitLesson = useCallback(() => {
    silence();
    setActiveLesson(null);
    enterStep(null, 0);
  }, [enterStep, silence]);

  const goToStep = useCallback((index: number) => {
    const lesson = machineRef.current.lesson;
    if (!lesson || index < 0 || index >= lesson.steps.length) return;
    silence();
    enterStep(lesson, index);
  }, [enterStep, silence]);

  const nextStep = useCallback(() => {
    const m = machineRef.current;
    const lesson = m.lesson;
    if (!lesson || !m.completed) return;
    if (m.index < lesson.steps.length - 1) {
      goToStep(m.index + 1);
      return;
    }
    // Finish: record completion, then make sure nothing keeps sounding.
    markLessonCompleted(lesson.id);
    setLastCompletedLessonId(lesson.id);
    silence();
    setActiveLesson(null);
    enterStep(null, 0);
  }, [goToStep, markLessonCompleted, silence, enterStep]);

  const prevStep = useCallback(() => {
    goToStep(machineRef.current.index - 1);
  }, [goToStep]);

  const completeQuiz = useCallback((correct: boolean) => {
    const step = stepAt(machineRef.current.lesson, machineRef.current.index);
    if (!correct || !step || (step.type !== 'quiz' && step.type !== 'listen-and-identify')) return;
    completeStep();
  }, [completeStep]);

  // --- Input evaluation (event driven, so fast notes are never missed) -----

  const evaluate = useCallback((event: NoteEvent) => {
    const m = machineRef.current;
    const step = stepAt(m.lesson, m.index);

    // Track held keys regardless of step type (release gating).
    if (event.type === 'noteon') {
      m.held.add(event.note);
      m.recent = [...m.recent.filter((r) => event.time - r.time <= TOGETHER_WINDOW_MS), { note: event.note, time: event.time }];
      m.played = [...m.played, event.note].slice(-MAX_HISTORY);
    } else if (event.type === 'noteoff') {
      m.held.delete(event.note);
    } else if (event.type === 'allnotesoff') {
      m.held.clear();
    }

    if (!step || m.completed) return;

    if (m.pending) {
      if (m.held.size === 0) completeStep();
      return;
    }

    let satisfied = false;
    switch (step.type) {
      case 'play-notes': {
        if (event.type !== 'noteon') break;
        const expected = expectedMidi(step);
        if (playNotesMode(step) === 'sequence') {
          // Melody: advance on the next expected note; wrong notes are simply
          // ignored (a slip doesn't restart a whole song).
          const target = expected[m.cursor];
          const matches = step.anyOctave ? samePitchClass : (a: number, b: number) => a === b;
          if (target !== undefined && matches(target, event.note)) {
            m.cursor += 1;
            setSequenceIndex(m.cursor);
          }
          satisfied = m.cursor >= expected.length;
        } else {
          const candidates = new Set([...m.held, ...m.recent.map((r) => r.note)]);
          satisfied = notesSatisfied(expected, candidates, step.anyOctave);
        }
        break;
      }
      case 'play-chord': {
        if (event.type !== 'noteon' || !step.expectedChord) break;
        satisfied =
          chordMatches(step.expectedChord, m.held) ||
          chordMatches(step.expectedChord, [...m.held, ...m.recent.map((r) => r.note)]);
        break;
      }
      case 'play-scale': {
        if (event.type !== 'noteon' || !step.expectedScale) break;
        const progress = scaleProgress(step.expectedScale.notes, m.played);
        setSequenceIndex(progress);
        satisfied = progress === step.expectedScale.notes.length;
        break;
      }
      case 'play-drums': {
        const pads = step.expectedDrumPattern?.pads ?? [];
        let padId: DrumPadId | undefined;
        if (event.type === 'drum') padId = event.padId;
        else if (event.type === 'noteon') padId = DRUM_NOTE_TO_PAD.get(event.note); // pads routed as keys
        if (padId === undefined) break;
        m.drumHits = [...m.drumHits.filter((h) => event.time - h.time <= DRUM_TOGETHER_WINDOW_MS), { padId, time: event.time }];
        // Drums are one-shots: complete right away, no release gate.
        if (drumPadsSatisfied(pads, m.drumHits, DRUM_TOGETHER_WINDOW_MS)) completeStep();
        return;
      }
      default:
        // info / quiz / listen-and-identify are completed by their own UI.
        return;
    }

    if (satisfied) {
      if (m.held.size === 0) {
        completeStep();
      } else {
        m.pending = true;
        setPendingRelease(true);
      }
    }
  }, [completeStep]);

  useNoteEvents(evaluate);

  // Leaving Learn (or unmounting the player) must not leave anything ringing.
  useEffect(() => () => {
    if (machineRef.current.lesson) {
      audioPromptPlayer.stop();
      onPanicRef.current?.();
    }
  }, []);

  return {
    activeLesson,
    currentStep,
    currentStepIndex,
    stepCompleted,
    pendingRelease,
    sequenceIndex,
    totalSteps: activeLesson?.steps.length ?? 0,
    lastCompletedLessonId,
    dismissCompletion: () => setLastCompletedLessonId(null),
    startLesson,
    exitLesson,
    nextStep,
    prevStep,
    goToStep,
    completeQuiz,
    isLessonCompleted,
  };
}
