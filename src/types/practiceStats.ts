export interface PracticeSessionRecord {
  exerciseId: string;
  timestamp: string;          // ISO date string
  durationSeconds: number;
  accuracy: number | null;    // null for free-play
  stars: 0 | 1 | 2 | 3;
  category: string;
}

export interface StreakData {
  currentStreak: number;
  longestStreak: number;
  lastPracticeDate: string | null;  // ISO date (day only)
}

export interface PracticeStatsStore {
  /** Schema version. Missing = written by <= v1.4 (accuracy stored as 0..1). */
  version?: number;
  sessions: PracticeSessionRecord[];  // max 500
  streaks: StreakData;
}
