import { useCallback, useRef, useState } from 'react';
import type { PracticeActivity, PracticeConfig, PracticeExercise } from '@/types/appMode';

export interface SessionResult {
  exerciseId: string;
  category: string;
  durationSeconds: number;
}

export function usePracticeSession() {
  const [config, setConfig] = useState<PracticeConfig | null>(null);
  const [activeExercise, setActiveExercise] = useState<PracticeExercise | null>(null);
  const sessionStartRef = useRef<number>(0);
  const sessionIdRef = useRef<string>('');
  const sessionCategoryRef = useRef<string>('');

  const startActivity = useCallback((activity: PracticeActivity) => {
    setActiveExercise(null);
    setConfig({ activity });
    sessionStartRef.current = Date.now();
    sessionIdRef.current = `free-${activity}`;
    sessionCategoryRef.current = 'free-play';
  }, []);

  const startExercise = useCallback((exercise: PracticeExercise) => {
    setConfig(null);
    setActiveExercise(exercise);
    sessionStartRef.current = Date.now();
    sessionIdRef.current = exercise.id;
    sessionCategoryRef.current = exercise.category;
  }, []);

  const exitActivity = useCallback((): SessionResult | null => {
    const duration = Math.round((Date.now() - sessionStartRef.current) / 1000);
    const result: SessionResult | null = sessionStartRef.current > 0
      ? {
        exerciseId: sessionIdRef.current,
        category: sessionCategoryRef.current,
        durationSeconds: duration,
      }
      : null;

    setConfig(null);
    setActiveExercise(null);
    sessionStartRef.current = 0;
    return result;
  }, []);

  return { config, activeExercise, startActivity, startExercise, exitActivity };
}
