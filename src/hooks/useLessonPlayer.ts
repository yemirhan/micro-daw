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
) {
  const [activeLesson, setActiveLesson] = useState<Lesson | null>(null);
  const [currentStepIndex, setCurrentStepIndex] = useState(0);
  const [stepCompleted, setStepCompleted] = useState(false);
  const noteSequenceRef = useRef<number[]>([]);
  const prevNotesRef = useRef<Set<number>>(new Set());
  const { markStepCompleted, markLessonCompleted, isLessonCompleted, getLessonProgress } = useProgress();

  const currentStep: LessonStep | null = activeLesson?.steps[currentStepIndex] ?? null;

  const startLesson = useCallback((lesson: Lesson) => {
    setActiveLesson(lesson);
    const progress = getLessonProgress(lesson.id);
    setCurrentStepIndex(progress?.lastStepIndex ?? 0);
    setStepCompleted(progress?.completedSteps.includes(progress?.lastStepIndex ?? 0) ?? false);
    noteSequenceRef.current = [];
  }, [getLessonProgress]);

  const exitLesson = useCallback(() => {
    setActiveLesson(null);
    setCurrentStepIndex(0);
    setStepCompleted(false);
    noteSequenceRef.current = [];
  }, []);

  const nextStep = useCallback(() => {
    if (!activeLesson) return;
    if (currentStepIndex < activeLesson.steps.length - 1) {
      setCurrentStepIndex((i) => i + 1);
      setStepCompleted(false);
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
      noteSequenceRef.current = [];
    }
  }, [currentStepIndex]);

  // Validate current step based on active notes
  useEffect(() => {
    if (!activeLesson || !currentStep || stepCompleted) return;

    const currentNotes = new Set(activeNotes.keys());

    // Track note sequence for scale steps: detect new notes
    for (const note of currentNotes) {
      if (!prevNotesRef.current.has(note)) {
        noteSequenceRef.current.push(note % 12);
      }
    }
    prevNotesRef.current = currentNotes;

    let completed = false;

    switch (currentStep.type) {
      case 'play-notes': {
        if (!currentStep.expectedNotes) break;
        completed = currentStep.expectedNotes.every((en) => activeNotes.has(en.midi));
        break;
      }
      case 'play-chord': {
        if (!currentStep.expectedChord) break;
        const chord = detectChord(Array.from(activeNotes.keys()));
        if (chord) {
          completed =
            chord.root === currentStep.expectedChord.root &&
            chord.quality === currentStep.expectedChord.quality;
        }
        break;
      }
      case 'play-scale': {
        if (!currentStep.expectedScale) break;
        const expected = currentStep.expectedScale.notes;
        const seq = noteSequenceRef.current;
        // Check if the last N notes match the expected scale ascending
        if (seq.length >= expected.length) {
          const tail = seq.slice(-expected.length);
          completed = expected.every((pc, i) => tail[i] === pc);
        }
        break;
      }
      case 'play-drums': {
        if (!currentStep.expectedDrumPattern) break;
        const expectedPads = currentStep.expectedDrumPattern.pads;
        // Check via activePads OR via active MIDI notes mapped to drum pads
        const currentPads = new Set<DrumPadId>();
        for (const note of activeNotes.keys()) {
          const padId = DRUM_NOTE_TO_PAD.get(note);
          if (padId !== undefined) currentPads.add(padId);
        }
        for (const pad of activePads) currentPads.add(pad);
        completed = expectedPads.every((p) => currentPads.has(p as DrumPadId));
        break;
      }
      // info and quiz are handled by their own UI
    }

    if (completed) {
      setStepCompleted(true);
      markStepCompleted(activeLesson.id, currentStepIndex);
    }
  }, [activeNotes, activePads, activeLesson, currentStep, currentStepIndex, stepCompleted, markStepCompleted]);

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
