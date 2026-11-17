import type { PracticeExercise } from '@/types/appMode';

export const CHORDS: PracticeExercise[] = [
  {
    id: 'chord-c-major',
    title: 'C Major Chord',
    description: 'Three white keys, skipping one between each',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'C', quality: 'maj', display: 'Cmaj' },
        { root: 'C', quality: 'maj', display: 'Cmaj' },
      ],
      instructions: 'Press C, E and G together, then lift every finger before the next one. Do it three times. Thumb, middle finger and pinky fit this shape nicely.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-g-major',
    title: 'G Major Chord',
    description: 'The C major shape, moved up to G',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'G', quality: 'maj', display: 'Gmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
        { root: 'G', quality: 'maj', display: 'Gmaj' },
      ],
      instructions: 'Hold G, B and D together, three times. It\'s the same skip-a-key shape as C major, just slid up. Let go completely between chords so each one counts.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-f-major',
    title: 'F Major Chord',
    description: 'F, A and C together',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'F', quality: 'maj', display: 'Fmaj' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
        { root: 'F', quality: 'maj', display: 'Fmaj' },
      ],
      instructions: 'Play F, A and C at once, three times. Any inversion counts, so if reaching up to C feels like a stretch, play the C below F instead.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-a-minor',
    title: 'A Minor Chord',
    description: 'The minor chord built on A',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'A', quality: 'm', display: 'Am' },
        { root: 'A', quality: 'm', display: 'Am' },
      ],
      instructions: 'Play A, C and E together, three times. It shares C and E with C major, yet the mood is completely different. Listen for that shift.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-d-minor',
    title: 'D Minor Chord',
    description: 'D, F and A for a softer, moodier sound',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'D', quality: 'm', display: 'Dm' },
        { root: 'D', quality: 'm', display: 'Dm' },
        { root: 'D', quality: 'm', display: 'Dm' },
      ],
      instructions: 'Stack D, F and A, three times. Keep your hand relaxed and lift it fully between chords; tense fingers make the next chord harder to place.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-e-minor',
    title: 'E Minor Chord',
    description: 'E, G and B, all on white keys',
    category: 'chords',
    difficulty: 'beginner',
    activity: 'chord-detection',
    config: {
      targetChords: [
        { root: 'E', quality: 'm', display: 'Em' },
        { root: 'E', quality: 'm', display: 'Em' },
        { root: 'E', quality: 'm', display: 'Em' },
      ],
      instructions: 'Play E, G and B together, three times. Raise the G to G# and it would become E major, so make sure you\'re on the white key.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-prog-cgamf',
    title: 'I-V-vi-IV Progression',
    description: 'C, G, Am, F: the chords behind countless pop songs',
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
      instructions: 'Play C, then G, then Am, then F, lifting your hand between each. Any inversion counts, so try C-E-G, B-D-G, A-C-E, A-C-F. Your hand barely has to move.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
  {
    id: 'chord-prog-amfcg',
    title: 'vi-IV-I-V Progression',
    description: 'The same four chords, starting on A minor',
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
      instructions: 'Play Am, F, C, G. These are the chords from the I-V-vi-IV progression, but starting on the minor one makes the loop sound more wistful. Lift between chords so each one registers.',
    },
    scoring: { type: 'chord-sequence', threeStars: 0.9, twoStars: 0.7 },
  },
];
