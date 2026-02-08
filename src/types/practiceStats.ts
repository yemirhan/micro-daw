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
  sessions: PracticeSessionRecord[];  // max 500
  streaks: StreakData;
}
