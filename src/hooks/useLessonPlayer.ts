import { useCallback, useEffect, useRef, useState } from 'react';
import { detectChord } from '@/utils/chordDetection';
import { useProgress } from './useProgress';
import type { ActiveNote } from './useMidiNotes';
import type { Lesson, LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';
import { DRUM_NOTE_TO_PAD } from '@/utils/constants';

export function useLessonPlayer(
  activeNotes: Map<number, ActiveNote>,
  activePads: Set<DrumPadId>,
  releaseAllNotes?: () => void,
) {
  const [activeLesson, setActiveLesson] = useState<Lesson | null>(null);
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [stepCompleted, setStepCompleted] = useState(false);
  // Two-phase completion: condition detected → wait for key release → complete
  const [pendingCompletion, setPendingCompletion] = useState(false);
  const noteSequenceRef = useRef<number[]>([]);
  const prevNotesRef = useRef<Set<number>>(new Set());
  const { markStepCompleted, markLessonCompleted, isLessonCompleted, getLessonProgress } = useProgress();

  const currentStep: LessonStep | null = activeLesson?.steps[currentStepIndex] ?? null;

  const startLesson = useCallback((lesson: Lesson) => {
    setActiveLesson(lesson);
    const progress = getLessonProgress(lesson.id);
    setCurrentStepIndex(progress?.lastStepIndex ?? 0);
    setStepCompleted(progress?.completedSteps.includes(progress?.lastStepIndex ?? 0) ?? false);
    setPendingCompletion(false);
    noteSequenceRef.current = [];
  }, [getLessonProgress]);

  const exitLesson = useCallback(() => {
    setActiveLesson(null);
    setCurrentStepIndex(0);
    setStepCompleted(false);
    setPendingCompletion(false);
    noteSequenceRef.current = [];
  }, []);

  const nextStep = useCallback(() => {
    if (!activeLesson) return;
    if (currentStepIndex < activeLesson.steps.length - 1) {
      setCurrentStepIndex((i) => i + 1);
      setStepCompleted(false);
      setPendingCompletion(false);
      noteSequenceRef.current = [];
    } else {
      markLessonCompleted(activeLesson.id);
      exitLesson();
    }
  }, [activeLesson, currentStepIndex, markLessonCompleted, exitLesson]);

  const prevStep = useCallback(() => {
    if (currentStepIndex > 0) {
      setCurrentStepIndex((i) => i - 1);
      setStepCompleted(false);
      setPendingCompletion(false);
      noteSequenceRef.current = [];
    }
  }, [currentStepIndex]);

  // Effect 1: Detect when the play condition is satisfied (while keys are still held)
  useEffect(() => {
    if (!activeLesson || !currentStep || stepCompleted || pendingCompletion) return;

    const currentNotes = new Set(activeNotes.keys());

    // Track note sequence for scale steps: detect new notes
    for (const note of currentNotes) {
      if (!prevNotesRef.current.has(note)) {
        noteSequenceRef.current.push(note % 12);
      }
    }
    prevNotesRef.current = currentNotes;

    let matchedThisFrame = false;

    switch (currentStep.type) {
      case 'play-notes': {
        if (!currentStep.expectedNotes) break;
        matchedThisFrame = currentStep.expectedNotes.every((en) => activeNotes.has(en.midi));
        break;
      }
      case 'play-chord': {
        if (!currentStep.expectedChord) break;
        const chord = detectChord(Array.from(activeNotes.keys()));
        if (chord) {
          matchedThisFrame =
            chord.root === currentStep.expectedChord.root &&
            chord.quality === currentStep.expectedChord.quality;
        }
        break;
      }
      case 'play-scale': {
        if (!currentStep.expectedScale) break;
        const expected = currentStep.expectedScale.notes;
        const seq = noteSequenceRef.current;
        if (seq.length >= expected.length) {
          const tail = seq.slice(-expected.length);
          matchedThisFrame = expected.every((pc, i) => tail[i] === pc);
        }
        break;
      }
      case 'play-drums': {
        // Drums are percussive one-shots — complete immediately, no release gate
        if (!currentStep.expectedDrumPattern) break;
        const expectedPads = currentStep.expectedDrumPattern.pads;
        const currentPads = new Set<DrumPadId>();
        for (const note of activeNotes.keys()) {
          const padId = DRUM_NOTE_TO_PAD.get(note);
          if (padId !== undefined) currentPads.add(padId);
        }
        for (const pad of activePads) currentPads.add(pad);
        if (expectedPads.every((p) => currentPads.has(p as DrumPadId))) {
          setStepCompleted(true);
          markStepCompleted(activeLesson.id, currentStepIndex);
        }
        return;
      }
      // info and quiz are handled by their own UI
    }

    if (matchedThisFrame) {
      setPendingCompletion(true);
    }
  }, [activeNotes, activePads, activeLesson, currentStep, currentStepIndex, stepCompleted, pendingCompletion, markStepCompleted]);

  // Effect 2: Complete only after all keys are released
  useEffect(() => {
    if (!pendingCompletion || stepCompleted || !activeLesson) return;
    if (activeNotes.size === 0) {
      setStepCompleted(true);
      markStepCompleted(activeLesson.id, currentStepIndex);
      setPendingCompletion(false);
      // Safety net: force-release any lingering audio
      releaseAllNotes?.();
    }
  }, [pendingCompletion, activeNotes, stepCompleted, activeLesson, currentStepIndex, markStepCompleted, releaseAllNotes]);

  const completeQuiz = useCallback(
    (correct: boolean) => {
      if (correct && activeLesson) {
        setStepCompleted(true);
        markStepCompleted(activeLesson.id, currentStepIndex);
      }
    },
    [activeLesson, currentStepIndex, markStepCompleted],
  );

  // For info steps, mark as completed immediately
  useEffect(() => {
    if (currentStep?.type === 'info' && !stepCompleted) {
      setStepCompleted(true);
      if (activeLesson) {
        markStepCompleted(activeLesson.id, currentStepIndex);
      }
    }
  }, [currentStep, stepCompleted, activeLesson, currentStepIndex, markStepCompleted]);

  return {
    activeLesson,
    currentStep,
    currentStepIndex,
    stepCompleted,
    totalSteps: activeLesson?.steps.length ?? 0,
    startLesson,
    exitLesson,
    nextStep,
    prevStep,
    completeQuiz,
    isLessonCompleted,
  };
}
