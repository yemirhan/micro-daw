import { useCallback, useState } from 'react';
import type { PracticeActivity, PracticeConfig, PracticeExercise } from '@/types/appMode';

export function usePracticeSession() {
  const [config, setConfig] = useState<PracticeConfig | null>(null);
  const [activeExercise, setActiveExercise] = useState<PracticeExercise | null>(null);

  const startActivity = useCallback((activity: PracticeActivity) => {
    setActiveExercise(null);
    setConfig({ activity });
  }, []);

  const startExercise = useCallback((exercise: PracticeExercise) => {
    setConfig(null);
    setActiveExercise(exercise);
  }, []);

  const exitActivity = useCallback(() => {
    setConfig(null);
    setActiveExercise(null);
  }, []);

  return { config, activeExercise, startActivity, startExercise, exitActivity };
}
