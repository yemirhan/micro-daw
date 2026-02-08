import { useCallback, useState } from 'react';
import { PRACTICE_STATS_STORAGE_KEY } from '@/utils/constants';
import type { PracticeSessionRecord, PracticeStatsStore, StreakData } from '@/types/practiceStats';

const MAX_SESSIONS = 500;
const MIN_DURATION = 5; // ignore sessions < 5 seconds

function defaultStore(): PracticeStatsStore {
  return {
    sessions: [],
    streaks: { currentStreak: 0, longestStreak: 0, lastPracticeDate: null },
  };
}

function loadStore(): PracticeStatsStore {
  try {
    const raw = localStorage.getItem(PRACTICE_STATS_STORAGE_KEY);
    if (raw) return JSON.parse(raw) as PracticeStatsStore;
  } catch { /* ignore */ }
  return defaultStore();
}

function saveStore(store: PracticeStatsStore) {
  localStorage.setItem(PRACTICE_STATS_STORAGE_KEY, JSON.stringify(store));
}

function todayString(): string {
  return new Date().toISOString().slice(0, 10);
}

function updateStreaks(streaks: StreakData): StreakData {
  const today = todayString();
  const yesterday = new Date(Date.now() - 86400000).toISOString().slice(0, 10);

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

    setStore((prev) => {
      const session: PracticeSessionRecord = {
        ...record,
        timestamp: new Date().toISOString(),
      };
      const sessions = [session, ...prev.sessions].slice(0, MAX_SESSIONS);
      const streaks = updateStreaks(prev.streaks);
      const next: PracticeStatsStore = { sessions, streaks };
      saveStore(next);
      return next;
    });
  }, []);

  const getDailySummaries = useCallback((days: number): DailySummary[] => {
    const result: Map<string, DailySummary> = new Map();
    const cutoff = new Date();
    cutoff.setDate(cutoff.getDate() - days);

    for (const s of store.sessions) {
      const date = s.timestamp.slice(0, 10);
      if (date < cutoff.toISOString().slice(0, 10)) continue;
      const existing = result.get(date) ?? { date, totalMinutes: 0, sessions: 0 };
      existing.totalMinutes += s.durationSeconds / 60;
      existing.sessions += 1;
      result.set(date, existing);
    }

    // Fill gaps with zero days
    const summaries: DailySummary[] = [];
    for (let i = days - 1; i >= 0; i--) {
      const d = new Date();
      d.setDate(d.getDate() - i);
      const dateStr = d.toISOString().slice(0, 10);
      summaries.push(result.get(dateStr) ?? { date: dateStr, totalMinutes: 0, sessions: 0 });
    }
    return summaries;
  }, [store.sessions]);

  const getAccuracyTrend = useCallback((): AccuracyPoint[] => {
    const byDate = new Map<string, { sum: number; count: number }>();
    for (const s of store.sessions) {
      if (s.accuracy == null) continue;
      const date = s.timestamp.slice(0, 10);
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
    return store.streaks;
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
