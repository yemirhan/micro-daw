import { useCallback, useState } from 'react';
import type { AppSettings } from '@/types/settings';
import { SETTINGS_STORAGE_KEY, DEFAULT_SETTINGS } from '@/utils/constants';

function loadSettings(): AppSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<AppSettings>;
      return {
        general: { ...DEFAULT_SETTINGS.general, ...parsed.general },
        audio: { ...DEFAULT_SETTINGS.audio, ...parsed.audio },
        midi: { ...DEFAULT_SETTINGS.midi, ...parsed.midi },
      };
    }
  } catch { /* ignore corrupt data */ }
  return { ...DEFAULT_SETTINGS };
}

function saveSettings(settings: AppSettings) {
  localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(settings));
}

export function useSettings() {
  const [settings, setSettings] = useState<AppSettings>(loadSettings);

  const updateSettings = useCallback((partial: Partial<AppSettings>) => {
    setSettings((prev) => {
      const next: AppSettings = {
        general: { ...prev.general, ...partial.general },
        audio: { ...prev.audio, ...partial.audio },
        midi: { ...prev.midi, ...partial.midi },
      };
      saveSettings(next);
      return next;
    });
  }, []);

  const resetLessonProgress = useCallback(() => {
    localStorage.removeItem('micro-daw-lesson-progress');
  }, []);

  const resetPracticeProgress = useCallback(() => {
    localStorage.removeItem('micro-daw-practice-progress');
  }, []);

  return { settings, updateSettings, resetLessonProgress, resetPracticeProgress };
}
