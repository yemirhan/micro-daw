import { useCallback, useRef, useState } from 'react';
import type { ScoringConfig } from '@/types/appMode';

interface ScoringResult {
  accuracy: number;
  stars: 0 | 1 | 2 | 3;
}

export function usePracticeScoring(config: ScoringConfig) {
  const [finished, setFinished] = useState(false);
  const [result, setResult] = useState<ScoringResult | null>(null);
  const correctRef = useRef(0);
  const totalRef = useRef(0);

  const recordEvent = useCallback((correct: boolean) => {
    totalRef.current += 1;
    if (correct) correctRef.current += 1;
  }, []);

  const computeStars = useCallback((accuracy: number): 0 | 1 | 2 | 3 => {
    if (accuracy >= config.threeStars) return 3;
    if (accuracy >= config.twoStars) return 2;
    if (totalRef.current > 0) return 1;
    return 0;
  }, [config.threeStars, config.twoStars]);

  const finish = useCallback((externalAccuracy?: number): ScoringResult => {
    const accuracy = externalAccuracy ?? (totalRef.current > 0 ? correctRef.current / totalRef.current : 0);
    const stars = computeStars(accuracy);
    const res = { accuracy, stars };
    setResult(res);
    setFinished(true);
    return res;
  }, [computeStars]);

  const reset = useCallback(() => {
    correctRef.current = 0;
    totalRef.current = 0;
    setFinished(false);
    setResult(null);
  }, []);

  return { finished, result, recordEvent, finish, reset };
}
