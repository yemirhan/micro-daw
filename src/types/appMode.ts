import type { DrumPadId } from './drums';

// --- App Mode ---
export type AppMode = 'daw' | 'learn' | 'practice' | 'settings';

// --- Lesson Types ---
export type LessonCategory = 'piano' | 'drums' | 'theory';
export type LessonDifficulty = 'beginner' | 'intermediate' | 'advanced';

export type LessonStepType =
  | 'info'
  | 'play-notes'
  | 'play-chord'
  | 'play-scale'
  | 'play-drums'
  | 'quiz';

export interface ExpectedNote {
  midi: number;
  label?: string;
}

export interface ExpectedChord {
  root: string;
  quality: string;
  display: string;
}

export interface ExpectedScale {
  root: number;       // 0-11 pitch class
  name: string;       // e.g. "Major"
  notes: number[];    // pitch classes
}

export interface ExpectedDrumPattern {
  pads: DrumPadId[];
}

export interface QuizOption {
  label: string;
  correct: boolean;
}

export interface LessonStep {
  type: LessonStepType;
  title: string;
  description: string;
  // Type-specific fields
  expectedNotes?: ExpectedNote[];
  expectedChord?: ExpectedChord;
  expectedScale?: ExpectedScale;
  expectedDrumPattern?: ExpectedDrumPattern;
  quizOptions?: QuizOption[];
  /** Optional hint shown below the prompt */
  hint?: string;
}

export interface LessonMeta {
  id: string;
  title: string;
  category: LessonCategory;
  difficulty: LessonDifficulty;
  estimatedMinutes: number;
  description: string;
}

export interface Lesson extends LessonMeta {
  steps: LessonStep[];
}

// --- Progress ---
export interface LessonProgress {
  lessonId: string;
  completedSteps: number[];
  completed: boolean;
  lastStepIndex: number;
}

export interface UserProgress {
  lessons: Record<string, LessonProgress>;
}

// --- Practice ---
export type PracticeActivity =
  | 'free-play'
  | 'chord-detection'
  | 'scale-practice'
  | 'rhythm-training'
  | 'drum-patterns';

export type PracticeCategory = 'piano-basics' | 'scales' | 'chords' | 'rhythm' | 'drum-patterns';

export interface PracticeExercise {
  id: string;
  title: string;
  description: string;
  category: PracticeCategory;
  difficulty: 'beginner' | 'intermediate';
  activity: PracticeActivity;
  config: PracticeExerciseConfig;
  scoring: ScoringConfig;
}

export interface PracticeExerciseConfig {
  scaleRoot?: number;
  scaleName?: string;
  targetNotes?: { midi: number; label: string }[];
  targetChords?: ExpectedChord[];
  requiredCount?: number;
  bpm?: number;
  totalBeats?: number;
  subdivision?: '4n' | '8n';
  pattern?: number[][];
  patternName?: string;
  instructions?: string;
}

export interface ScoringConfig {
  type: 'note-accuracy' | 'chord-sequence' | 'rhythm-accuracy' | 'pattern-accuracy' | 'note-sequence';
  threeStars: number;
  twoStars: number;
}

export interface PracticeScore {
  bestStars: 0 | 1 | 2 | 3;
  bestAccuracy: number;
  attempts: number;
}

export interface PracticeConfig {
  activity: PracticeActivity;
  bpm?: number;
  selectedScale?: { root: number; name: string };
}

export interface RhythmFeedback {
  beat: number;
  timing: 'perfect' | 'good' | 'early' | 'late' | 'miss';
  offsetMs: number;
}
