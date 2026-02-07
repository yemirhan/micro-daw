import { useState } from 'react';
import { ArrowLeft, Piano, Music2, Waves, Timer, Drum } from 'lucide-react';
import { Target } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { PracticeActivityCard } from './PracticeActivityCard';
import { PracticeCategoryFilter } from './PracticeCategoryFilter';
import { PracticeExerciseCard } from './PracticeExerciseCard';
import { ExerciseRunner } from './ExerciseRunner';
import { FreePlay } from './FreePlay';
import { ChordDetector } from './ChordDetector';
import { ScalePractice } from './ScalePractice';
import { RhythmTrainer } from './RhythmTrainer';
import { DrumPatternPractice } from './DrumPatternPractice';
import { usePracticeSession } from '@/hooks/usePracticeSession';
import { usePracticeProgress } from '@/hooks/usePracticeProgress';
import { getPracticesByCategory } from '@/data/practices';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { ScaleName } from '@/types/music';
import type { DrumPadId } from '@/types/drums';
import type { PracticeActivity, PracticeCategory } from '@/types/appMode';

interface PracticeViewProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  highlightedNotes: Set<number>;
  selectedRoot: number;
  setSelectedRoot: (root: number) => void;
  selectedScale: ScaleName | null;
  setSelectedScale: (scale: ScaleName | null) => void;
}

const ACTIVITIES: { id: PracticeActivity; icon: typeof Piano; title: string; description: string; color: string }[] = [
  { id: 'free-play', icon: Music2, title: 'Free Play', description: 'Play freely with chord detection', color: 'oklch(0.70 0.20 270)' },
  { id: 'chord-detection', icon: Piano, title: 'Chord Detection', description: 'Identify chords in real-time', color: 'oklch(0.70 0.20 150)' },
  { id: 'scale-practice', icon: Waves, title: 'Scale Practice', description: 'Explore scales freely', color: 'oklch(0.70 0.20 90)' },
  { id: 'rhythm-training', icon: Timer, title: 'Rhythm Training', description: 'Free rhythm practice', color: 'oklch(0.70 0.20 25)' },
  { id: 'drum-patterns', icon: Drum, title: 'Drum Patterns', description: 'Free drum practice', color: 'oklch(0.70 0.20 310)' },
];

export function PracticeView({
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  highlightedNotes,
  selectedRoot,
  setSelectedRoot,
  selectedScale,
  setSelectedScale,
}: PracticeViewProps) {
  const { config, activeExercise, startActivity, startExercise, exitActivity } = usePracticeSession();
  const { getScore, saveScore } = usePracticeProgress();
  const [selectedCategory, setSelectedCategory] = useState<PracticeCategory | null>(null);

  const exercises = getPracticesByCategory(selectedCategory);

  // Active exercise mode
  if (activeExercise) {
    return (
      <ExerciseRunner
        exercise={activeExercise}
        onExit={exitActivity}
        onSaveScore={saveScore}
        activeNotes={activeNotes}
        onNoteOn={onNoteOn}
        onNoteOff={onNoteOff}
        activePads={activePads}
        onDrumHit={onDrumHit}
        detectedChord={detectedChord}
        highlightedNotes={highlightedNotes}
        selectedRoot={selectedRoot}
        setSelectedRoot={setSelectedRoot}
        selectedScale={selectedScale}
        setSelectedScale={setSelectedScale}
      />
    );
  }

  // Active free activity mode
  if (config) {
    return (
      <div className="flex flex-1 flex-col overflow-hidden">
        <div className="flex items-center gap-3 border-b border-border/50 px-6 py-3">
          <Button variant="ghost" size="sm" className="h-7 w-7 p-0" onClick={exitActivity} title="Back to activities">
            <ArrowLeft className="h-4 w-4" />
          </Button>
          <h2 className="text-sm font-semibold">
            {ACTIVITIES.find((a) => a.id === config.activity)?.title}
          </h2>
        </div>

        <div className="flex-1 overflow-y-auto">
          {config.activity === 'free-play' && (
            <FreePlay
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              activePads={activePads}
              onDrumHit={onDrumHit}
              detectedChord={detectedChord}
            />
          )}
          {config.activity === 'chord-detection' && (
            <ChordDetector
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              detectedChord={detectedChord}
            />
          )}
          {config.activity === 'scale-practice' && (
            <ScalePractice
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
              highlightedNotes={highlightedNotes}
              selectedRoot={selectedRoot}
              setSelectedRoot={setSelectedRoot}
              selectedScale={selectedScale}
              setSelectedScale={setSelectedScale}
            />
          )}
          {config.activity === 'rhythm-training' && (
            <RhythmTrainer
              activeNotes={activeNotes}
              onNoteOn={onNoteOn}
              onNoteOff={onNoteOff}
            />
          )}
          {config.activity === 'drum-patterns' && (
            <DrumPatternPractice
              activePads={activePads}
              onDrumHit={onDrumHit}
            />
          )}
        </div>
      </div>
    );
  }

  // Browse mode — activity hub + exercise grid
  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="border-b border-border/50 px-6 py-4">
        <div className="flex items-center gap-3">
          <Target className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-lg font-bold">Practice</h1>
        </div>
        <p className="mt-1 text-xs text-muted-foreground">
          Free-play activities and scored exercises with star ratings
        </p>
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        {/* Free Activities row */}
        <div className="mb-6">
          <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Free Activities
          </h2>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-5">
            {ACTIVITIES.map((activity) => (
              <PracticeActivityCard
                key={activity.id}
                icon={activity.icon}
                title={activity.title}
                description={activity.description}
                color={activity.color}
                onClick={() => startActivity(activity.id)}
              />
            ))}
          </div>
        </div>

        {/* Exercises section */}
        <div>
          <h2 className="mb-3 text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Exercises
          </h2>
          <div className="mb-4">
            <PracticeCategoryFilter selected={selectedCategory} onChange={setSelectedCategory} />
          </div>
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {exercises.map((exercise) => (
              <PracticeExerciseCard
                key={exercise.id}
                exercise={exercise}
                score={getScore(exercise.id)}
                onClick={() => startExercise(exercise)}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
