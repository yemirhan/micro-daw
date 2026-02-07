import { useCallback, useState } from 'react';
import type { AppMode } from '@/types/appMode';

export function useAppMode() {
  const [mode, setMode] = useState<AppMode>('daw');

  const switchMode = useCallback((next: AppMode) => {
    setMode(next);
  }, []);

  return { mode, switchMode };
}
