import type { TourStep } from '@/types/onboarding';

export const TOUR_STEPS: TourStep[] = [
  {
    target: 'sidebar',
    title: 'Welcome to Micro DAW!',
    description: 'This is your navigation sidebar. Switch between DAW, Learn, Practice, and more.',
    placement: 'right',
  },
  {
    target: 'transport',
    title: 'Transport Controls',
    description: 'Record, play, stop, and control tempo here. Arm a track first, then hit record to capture your performance.',
    placement: 'bottom',
    requiredMode: 'daw',
  },
  {
    target: 'arrangement',
    title: 'Arrangement View',
    description: 'This is where your tracks and regions live. Add tracks, record MIDI, and arrange your song.',
    placement: 'top',
    requiredMode: 'daw',
  },
  {
    target: 'instrument-dock',
    title: 'Instrument Area',
    description: 'Play notes on the piano keyboard or drum pads. You can also connect a MIDI controller.',
    placement: 'top',
    requiredMode: 'daw',
  },
  {
    target: 'learn-view',
    title: 'Learn Mode',
    description: 'Structured lessons for piano, drums, ear training, and music theory. Start here if you\'re new!',
    placement: 'right',
    requiredMode: 'learn',
  },
  {
    target: 'practice-view',
    title: 'Practice Mode',
    description: 'Free-play activities and scored exercises. Track your progress with detailed statistics.',
    placement: 'right',
    requiredMode: 'practice',
  },
  {
    target: 'sidebar',
    title: 'You\'re all set!',
    description: 'Explore the DAW, try a lesson, or jump into practice. You can replay this tour anytime from Settings.',
    placement: 'right',
    requiredMode: 'daw',
  },
];
