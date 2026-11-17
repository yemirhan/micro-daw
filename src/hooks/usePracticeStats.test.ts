import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { localDay, usePracticeStats } from '@/hooks/usePracticeStats';
import { PRACTICE_STATS_STORAGE_KEY } from '@/utils/constants';

afterEach(() => {
  vi.useRealTimers();
});

const record = { exerciseId: 'x', durationSeconds: 120, accuracy: 90, stars: 3 as const, category: 'scales' };

describe('usePracticeStats', () => {
  it('uses local calendar days', () => {
    expect(localDay(new Date(2026, 0, 5, 23, 59))).toBe('2026-01-05');
    expect(localDay(new Date(2026, 11, 31, 0, 1))).toBe('2026-12-31');
  });

  it('persists sessions synchronously and ignores very short ones', () => {
    const { result } = renderHook(() => usePracticeStats());
    act(() => result.current.recordSession({ ...record, durationSeconds: 2 }));
    expect(result.current.totalSessions).toBe(0);
    act(() => result.current.recordSession(record));
    expect(result.current.totalSessions).toBe(1);
    expect(JSON.parse(localStorage.getItem(PRACTICE_STATS_STORAGE_KEY)!).sessions).toHaveLength(1);
  });

  it('builds a streak over consecutive days and reports a broken streak as 0', () => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(new Date(2026, 4, 1, 20, 0));
    const { result, rerender } = renderHook(() => usePracticeStats());
    act(() => result.current.recordSession(record));
    vi.setSystemTime(new Date(2026, 4, 2, 9, 0));
    act(() => result.current.recordSession(record));
    rerender();
    expect(result.current.getStreak().currentStreak).toBe(2);
    vi.setSystemTime(new Date(2026, 4, 5, 9, 0));
    rerender();
    expect(result.current.getStreak().currentStreak).toBe(0);
    expect(result.current.getStreak().longestStreak).toBe(2);
  });

  it('migrates legacy 0..1 accuracies to percentages', () => {
    localStorage.setItem(
      PRACTICE_STATS_STORAGE_KEY,
      JSON.stringify({
        sessions: [{ ...record, accuracy: 0.85, timestamp: new Date().toISOString() }],
        streaks: { currentStreak: 1, longestStreak: 1, lastPracticeDate: localDay() },
      }),
    );
    const { result } = renderHook(() => usePracticeStats());
    expect(result.current.getRecentSessions()[0].accuracy).toBe(85);
    expect(result.current.getAccuracyTrend()[0].accuracy).toBe(85);
  });

  it('migrates every unversioned record once, including exact 0 and 1 (100%)', () => {
    const ts = new Date().toISOString();
    localStorage.setItem(
      PRACTICE_STATS_STORAGE_KEY,
      JSON.stringify({
        sessions: [
          { ...record, accuracy: 1, timestamp: ts },
          { ...record, accuracy: 0, timestamp: ts },
          { ...record, accuracy: null, timestamp: ts },
        ],
        streaks: { currentStreak: 1, longestStreak: 1, lastPracticeDate: localDay() },
      }),
    );
    const { result } = renderHook(() => usePracticeStats());
    expect(result.current.getRecentSessions().map((s) => s.accuracy)).toEqual([100, 0, null]);
    const stored = JSON.parse(localStorage.getItem(PRACTICE_STATS_STORAGE_KEY)!);
    expect(stored.version).toBe(2);
    expect(stored.sessions[0].accuracy).toBe(100);
    // Already-migrated stores are left alone (1% stays 1%).
    act(() => result.current.recordSession({ ...record, accuracy: 1 }));
    const { result: again } = renderHook(() => usePracticeStats());
    expect(again.current.getRecentSessions()[0].accuracy).toBe(1);
    expect(again.current.getRecentSessions()[1].accuracy).toBe(100);
  });
});
