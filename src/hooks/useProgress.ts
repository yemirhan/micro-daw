import { useCallback, useState } from 'react';
import type { UserProgress, LessonProgress } from '@/types/appMode';

const STORAGE_KEY = 'micro-daw-lesson-progress';

function loadProgress(): UserProgress {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as UserProgress;
  } catch { /* ignore corrupt data */ }
  return { lessons: {} };
}

function saveProgress(progress: UserProgress) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(progress));
}

export function useProgress() {
  const [progress, setProgress] = useState<UserProgress>(loadProgress);

  const getLessonProgress = useCallback(
    (lessonId: string): LessonProgress | undefined => progress.lessons[lessonId],
    [progress],
  );

  const isLessonCompleted = useCallback(
    (lessonId: string): boolean => progress.lessons[lessonId]?.completed ?? false,
    [progress],
  );

  const markStepCompleted = useCallback((lessonId: string, stepIndex: number) => {
    setProgress((prev) => {
      const existing = prev.lessons[lessonId] ?? {
        lessonId,
        completedSteps: [],
        completed: false,
        lastStepIndex: 0,
      };
      const completedSteps = existing.completedSteps.includes(stepIndex)
        ? existing.completedSteps
        : [...existing.completedSteps, stepIndex];
      const next: UserProgress = {
        ...prev,
        lessons: {
          ...prev.lessons,
          [lessonId]: { ...existing, completedSteps, lastStepIndex: stepIndex },
        },
      };
      saveProgress(next);
      return next;
    });
  }, []);

  const markLessonCompleted = useCallback((lessonId: string) => {
    setProgress((prev) => {
      const existing = prev.lessons[lessonId] ?? {
        lessonId,
        completedSteps: [],
        completed: false,
        lastStepIndex: 0,
      };
      const next: UserProgress = {
        ...prev,
        lessons: {
          ...prev.lessons,
          [lessonId]: { ...existing, completed: true },
        },
      };
      saveProgress(next);
      return next;
    });
  }, []);

  return { progress, getLessonProgress, isLessonCompleted, markStepCompleted, markLessonCompleted };
}
