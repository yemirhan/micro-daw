import { Drum, Layers, Music2, Piano, Timer, Waves, type LucideIcon } from 'lucide-react';
import { getPracticeById } from '@/data/practices';
import type { PracticeActivity, PracticeCategory } from '@/types/appMode';

export const PRACTICE_CATEGORY_META: Record<PracticeCategory, { label: string; icon: LucideIcon; blurb: string }> = {
  'piano-basics': { label: 'Piano Basics', icon: Piano, blurb: 'Find notes and play short patterns' },
  scales: { label: 'Scales', icon: Waves, blurb: 'Run scales up an octave' },
  chords: { label: 'Chords', icon: Layers, blurb: 'Shapes and progressions' },
  rhythm: { label: 'Rhythm', icon: Timer, blurb: 'Lock in with the click' },
  'drum-patterns': { label: 'Drum Patterns', icon: Drum, blurb: 'Grooves on the pads' },
};

export const PRACTICE_CATEGORY_ORDER: PracticeCategory[] = ['piano-basics', 'scales', 'chords', 'rhythm', 'drum-patterns'];

export const ACTIVITIES: {
  id: PracticeActivity;
  icon: LucideIcon;
  title: string;
  description: string;
}[] = [
  { id: 'free-play', icon: Music2, title: 'Free Play', description: 'Keys and pads with live chord names' },
  { id: 'chord-detection', icon: Layers, title: 'Chord Detection', description: 'See what chord you are holding' },
  { id: 'scale-practice', icon: Waves, title: 'Scale Practice', description: 'Light up any scale on the keys' },
  { id: 'rhythm-training', icon: Timer, title: 'Rhythm Training', description: 'Play along with the click' },
  { id: 'drum-patterns', icon: Drum, title: 'Drum Patterns', description: 'Follow a rock groove' },
];

export const ACTIVITY_LABEL: Record<PracticeActivity, string> = {
  'free-play': 'Free play',
  'chord-detection': 'Chords',
  'scale-practice': 'Notes',
  'rhythm-training': 'Timing',
  'drum-patterns': 'Pads',
};

/** Human name for a recorded session id (exercise id or `free-<activity>`). */
export function sessionTitle(exerciseId: string): string {
  const exercise = getPracticeById(exerciseId);
  if (exercise) return exercise.title;
  const activity = ACTIVITIES.find((a) => `free-${a.id}` === exerciseId || a.id === exerciseId);
  if (activity) return activity.title;
  return exerciseId;
}
