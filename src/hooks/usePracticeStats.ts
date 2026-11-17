import { useCallback, useState } from 'react';
import { PRACTICE_STATS_STORAGE_KEY } from '@/utils/constants';
import type { PracticeSessionRecord, PracticeStatsStore, StreakData } from '@/types/practiceStats';

const MAX_SESSIONS = 500;
const MIN_DURATION = 5; // ignore sessions < 5 seconds
/** v2: accuracy is a percentage (0..100). Unversioned stores used 0..1. */
const STORE_VERSION = 2;

function defaultStore(): PracticeStatsStore {
  return {
    version: STORE_VERSION,
    sessions: [],
    streaks: { currentStreak: 0, longestStreak: 0, lastPracticeDate: null },
  };
}

function loadStore(): PracticeStatsStore {
  try {
    const raw = localStorage.getItem(PRACTICE_STATS_STORAGE_KEY);
    if (raw) {
      const store = JSON.parse(raw) as PracticeStatsStore;
      store.sessions ??= [];
      store.streaks ??= defaultStore().streaks;
      if ((store.version ?? 1) < STORE_VERSION) {
        // Older builds stored accuracy as 0..1 (including exact 0 and 1 for
        // 0% / 100%); convert every record once and persist the result.
        store.sessions = store.sessions.map((s) =>
          s.accuracy != null && s.accuracy <= 1 ? { ...s, accuracy: Math.round(s.accuracy * 100) } : s,
        );
        store.version = STORE_VERSION;
        saveStore(store);
      }
      return store;
    }
  } catch { /* ignore */ }
  return defaultStore();
}

function saveStore(store: PracticeStatsStore) {
  localStorage.setItem(PRACTICE_STATS_STORAGE_KEY, JSON.stringify(store));
}

/** YYYY-MM-DD in the user's local time zone (a practice "day" is a local day). */
export function localDay(date: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

function daysAgo(days: number): Date {
  const d = new Date();
  d.setDate(d.getDate() - days);
  return d;
}

function updateStreaks(streaks: StreakData): StreakData {
  const today = localDay();
  const yesterday = localDay(daysAgo(1));

  if (streaks.lastPracticeDate === today) {
    // Already practiced today — no streak change
    return streaks;
  }

  let current: number;
  if (streaks.lastPracticeDate === yesterday) {
    current = streaks.currentStreak + 1;
  } else {
    current = 1; // streak broken or first session
  }

  return {
    currentStreak: current,
    longestStreak: Math.max(streaks.longestStreak, current),
    lastPracticeDate: today,
  };
}

export interface DailySummary {
  date: string;
  totalMinutes: number;
  sessions: number;
}

export interface AccuracyPoint {
  date: string;
  accuracy: number;
}

export interface CategoryTime {
  category: string;
  minutes: number;
}

export function usePracticeStats() {
  const [store, setStore] = useState<PracticeStatsStore>(loadStore);

  const recordSession = useCallback((record: Omit<PracticeSessionRecord, 'timestamp'>) => {
    if (record.durationSeconds < MIN_DURATION) return;
    // Read-modify-write storage synchronously so a session recorded while the
    // view unmounts (mode switch) is still persisted.
    const prev = loadStore();
    const session: PracticeSessionRecord = {
      ...record,
      timestamp: new Date().toISOString(),
    };
    const sessions = [session, ...prev.sessions].slice(0, MAX_SESSIONS);
    const streaks = updateStreaks(prev.streaks);
    const next: PracticeStatsStore = { version: STORE_VERSION, sessions, streaks };
    saveStore(next);
    setStore(next);
  }, []);

  const getDailySummaries = useCallback((days: number): DailySummary[] => {
    const result: Map<string, DailySummary> = new Map();
    const cutoff = localDay(daysAgo(days));

    for (const s of store.sessions) {
      const date = localDay(new Date(s.timestamp));
      if (date < cutoff) continue;
      const existing = result.get(date) ?? { date, totalMinutes: 0, sessions: 0 };
      existing.totalMinutes += s.durationSeconds / 60;
      existing.sessions += 1;
      result.set(date, existing);
    }

    // Fill gaps with zero days
    const summaries: DailySummary[] = [];
    for (let i = days - 1; i >= 0; i--) {
      const dateStr = localDay(daysAgo(i));
      summaries.push(result.get(dateStr) ?? { date: dateStr, totalMinutes: 0, sessions: 0 });
    }
    return summaries;
  }, [store.sessions]);

  const getAccuracyTrend = useCallback((): AccuracyPoint[] => {
    const byDate = new Map<string, { sum: number; count: number }>();
    for (const s of store.sessions) {
      if (s.accuracy == null) continue;
      const date = localDay(new Date(s.timestamp));
      const existing = byDate.get(date) ?? { sum: 0, count: 0 };
      existing.sum += s.accuracy;
      existing.count += 1;
      byDate.set(date, existing);
    }
    return Array.from(byDate.entries())
      .map(([date, { sum, count }]) => ({
        date,
        accuracy: Math.round(sum / count),
      }))
      .sort((a, b) => a.date.localeCompare(b.date))
      .slice(-30);
  }, [store.sessions]);

  const getTotalPracticeTime = useCallback((): number => {
    return store.sessions.reduce((sum, s) => sum + s.durationSeconds, 0);
  }, [store.sessions]);

  const getStreak = useCallback((): StreakData => {
    // A streak whose last practice day is older than yesterday is broken.
    const { lastPracticeDate } = store.streaks;
    const alive = lastPracticeDate === localDay() || lastPracticeDate === localDay(daysAgo(1));
    return alive ? store.streaks : { ...store.streaks, currentStreak: 0 };
  }, [store.streaks]);

  const getCategoryBreakdown = useCallback((): CategoryTime[] => {
    const map = new Map<string, number>();
    for (const s of store.sessions) {
      const existing = map.get(s.category) ?? 0;
      map.set(s.category, existing + s.durationSeconds / 60);
    }
    return Array.from(map.entries())
      .map(([category, minutes]) => ({ category, minutes: Math.round(minutes * 10) / 10 }))
      .sort((a, b) => b.minutes - a.minutes);
  }, [store.sessions]);

  const getRecentSessions = useCallback((limit = 20): PracticeSessionRecord[] => {
    return store.sessions.slice(0, limit);
  }, [store.sessions]);

  const resetStats = useCallback(() => {
    const next = defaultStore();
    saveStore(next);
    setStore(next);
  }, []);

  return {
    recordSession,
    getDailySummaries,
    getAccuracyTrend,
    getTotalPracticeTime,
    getStreak,
    getCategoryBreakdown,
    getRecentSessions,
    resetStats,
    totalSessions: store.sessions.length,
  };
}
