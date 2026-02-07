import { useCallback, useState } from 'react';
import type { PracticeScore } from '@/types/appMode';

const STORAGE_KEY = 'micro-daw-practice-progress';

type PracticeProgressMap = Record<string, PracticeScore>;

function loadScores(): PracticeProgressMap {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw) as PracticeProgressMap;
  } catch { /* ignore corrupt data */ }
  return {};
}

function persistScores(scores: PracticeProgressMap) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(scores));
}

export function usePracticeProgress() {
  const [scores, setScores] = useState<PracticeProgressMap>(loadScores);

  const getScore = useCallback(
    (id: string): PracticeScore | undefined => scores[id],
    [scores],
  );

  const saveScore = useCallback((id: string, stars: 0 | 1 | 2 | 3, accuracy: number) => {
    setScores((prev) => {
      const existing = prev[id];
      const next: PracticeProgressMap = {
        ...prev,
        [id]: {
          bestStars: existing ? Math.max(existing.bestStars, stars) as 0 | 1 | 2 | 3 : stars,
          bestAccuracy: existing ? Math.max(existing.bestAccuracy, accuracy) : accuracy,
          attempts: (existing?.attempts ?? 0) + 1,
        },
      };
      persistScores(next);
      return next;
    });
  }, []);

  return { scores, getScore, saveScore };
}
