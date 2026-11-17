import { BookOpen, Drum, Ear, Headphones, ListChecks, Music, Music2, Piano, Waves, Layers, type LucideIcon } from 'lucide-react';
import { ALL_LESSONS } from '@/data/lessons';
import type { Lesson, LessonCategory, LessonStepType, UserProgress } from '@/types/appMode';

export const CATEGORY_META: Record<LessonCategory, { label: string; icon: LucideIcon; blurb: string }> = {
  piano: { label: 'Piano', icon: Piano, blurb: 'Notes, scales and chords at the keyboard' },
  drums: { label: 'Drums', icon: Drum, blurb: 'Kit pieces, grooves and fills' },
  theory: { label: 'Theory', icon: BookOpen, blurb: 'How music fits together' },
  'ear-training': { label: 'Ear Training', icon: Headphones, blurb: 'Hear intervals and chord qualities' },
  songs: { label: 'Songs', icon: Music, blurb: 'Play real melodies' },
};

export const CATEGORY_ORDER: LessonCategory[] = ['piano', 'drums', 'theory', 'ear-training', 'songs'];

export const STEP_KIND: Record<LessonStepType, { label: string; icon: LucideIcon }> = {
  info: { label: 'Read', icon: BookOpen },
  'play-notes': { label: 'Play notes', icon: Piano },
  'play-chord': { label: 'Play a chord', icon: Layers },
  'play-scale': { label: 'Play a scale', icon: Waves },
  'play-drums': { label: 'Hit the pads', icon: Drum },
  quiz: { label: 'Quiz', icon: ListChecks },
  'listen-and-identify': { label: 'Ear training', icon: Ear },
};

export const MELODY_ICON = Music2;

const PROGRESS_KEY = 'micro-daw-lesson-progress';

/**
 * Read-only snapshot of lesson progress (written by useProgress inside
 * useLessonPlayer). The browser only needs to display it.
 */
export function readLessonProgress(): UserProgress {
  try {
    const raw = localStorage.getItem(PROGRESS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as UserProgress;
      if (parsed && typeof parsed === 'object' && parsed.lessons) return parsed;
    }
  } catch {
    /* ignore corrupt data */
  }
  return { lessons: {} };
}

/** Steps done (0..1) for a lesson, 1 when completed. */
export function lessonFraction(lesson: Lesson, progress: UserProgress): number {
  const p = progress.lessons[lesson.id];
  if (!p) return 0;
  if (p.completed) return 1;
  const done = new Set(p.completedSteps.filter((i) => i >= 0 && i < lesson.steps.length));
  return lesson.steps.length > 0 ? done.size / lesson.steps.length : 0;
}

/**
 * Lesson to suggest after finishing `lessonId`: the next unfinished lesson in
 * the same category, else the next unfinished lesson anywhere.
 */
export function nextLessonAfter(lessonId: string, isCompleted: (id: string) => boolean): Lesson | undefined {
  const index = ALL_LESSONS.findIndex((l) => l.id === lessonId);
  if (index === -1) return undefined;
  const current = ALL_LESSONS[index];
  const after = [...ALL_LESSONS.slice(index + 1), ...ALL_LESSONS.slice(0, index)];
  return (
    after.find((l) => l.category === current.category && !isCompleted(l.id)) ??
    after.find((l) => !isCompleted(l.id))
  );
}
