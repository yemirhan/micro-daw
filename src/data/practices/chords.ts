import type { PracticeExercise } from '@/types/appMode';

export const CHORDS: PracticeExercise[] = [
  {
    id: 'chord-c-major',
    title: 'C Major Chord',
    description: 'Play the C Major chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'C', quality: 'maj', display: 'Cmaj' },
      ],
      instructions: 'Play the C Major chord (C-E-G) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-g-major',
    title: 'G Major Chord',
    description: 'Play the G Major chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'G', quality: 'maj', display: 'Gmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
      ],
      instructions: 'Play the G Major chord (G-B-D) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-f-major',
    title: 'F Major Chord',
    description: 'Play the F Major chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'F', quality: 'maj', display: 'Fmaj' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
      ],
      instructions: 'Play the F Major chord (F-A-C) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-a-minor',
    title: 'A Minor Chord',
    description: 'Play the A Minor chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'A', quality: 'm', display: 'Am' },
      ],
      instructions: 'Play the A Minor chord (A-C-E) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-d-minor',
    title: 'D Minor Chord',
    description: 'Play the D Minor chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'D', quality: 'm', display: 'Dm' },
        { root: 'D', quality: 'm', display: 'Dm' },
        { root: 'D', quality: 'm', display: 'Dm' },
      ],
      instructions: 'Play the D Minor chord (D-F-A) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-e-minor',
    title: 'E Minor Chord',
    description: 'Play the E Minor chord 3 times',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'E', quality: 'm', display: 'Em' },
        { root: 'E', quality: 'm', display: 'Em' },
        { root: 'E', quality: 'm', display: 'Em' },
      ],
      instructions: 'Play the E Minor chord (E-G-B) three times.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-prog-cgamf',
    title: 'I-V-vi-IV Progression',
    description: 'Play C - G - Am - F chord progression',
    category: 'chords',
    difficulty: 'intermediate',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
      ],
      instructions: 'Play the famous I-V-vi-IV progression: C → G → Am → F',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-prog-amfcg',
    title: 'vi-IV-I-V Progression',
    description: 'Play Am - F - C - G chord progression',
    category: 'chords',
    difficulty: 'intermediate',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
      ],
      instructions: 'Play the vi-IV-I-V progression: Am → F → C → G',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
];
