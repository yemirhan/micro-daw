import { useState } from 'react';
import { GraduationCap } from 'lucide-react';
import { CategoryFilter } from './CategoryFilter';
import { LessonCard } from './LessonCard';
import { LessonPlayer } from './LessonPlayer';
import { useLessonPlayer } from '@/hooks/useLessonPlayer';
import { getLessonsByCategory } from '@/data/lessons';
import type { LessonCategory } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface LearnViewProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
}

export function LearnView({
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
}: LearnViewProps) {
  const [categoryFilter, setCategoryFilter] = useState<LessonCategory | null>(null);
  const lessonPlayer = useLessonPlayer(activeNotes, activePads);

  const filteredLessons = getLessonsByCategory(categoryFilter ?? undefined);

  // If a lesson is active, show the player
  if (lessonPlayer.activeLesson && lessonPlayer.currentStep) {
    return (
      <LessonPlayer
        lesson={lessonPlayer.activeLesson}
        currentStep={lessonPlayer.currentStep}
        currentStepIndex={lessonPlayer.currentStepIndex}
        totalSteps={lessonPlayer.totalSteps}
        stepCompleted={lessonPlayer.stepCompleted}
        activeNotes={activeNotes}
        onNoteOn={onNoteOn}
        onNoteOff={onNoteOff}
        activePads={activePads}
        onDrumHit={onDrumHit}
        detectedChord={detectedChord}
        onNext={lessonPlayer.nextStep}
        onPrev={lessonPlayer.prevStep}
        onExit={lessonPlayer.exitLesson}
        onQuizAnswer={lessonPlayer.completeQuiz}
      />
    );
  }

  // Lesson browser
  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      <div className="border-b border-border/50 px-6 py-4">
        <div className="flex items-center gap-3">
          <GraduationCap className="h-5 w-5 text-muted-foreground" />
          <h1 className="text-lg font-bold">Learn</h1>
        </div>
        <p className="mt-1 text-xs text-muted-foreground">
          Structured lessons to learn piano, drums, and music theory
        </p>
        <div className="mt-3">
          <CategoryFilter selected={categoryFilter} onChange={setCategoryFilter} />
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-6 py-4">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {filteredLessons.map((lesson) => (
            <LessonCard
              key={lesson.id}
              lesson={lesson}
              isCompleted={lessonPlayer.isLessonCompleted(lesson.id)}
              onClick={() => lessonPlayer.startLesson(lesson)}
            />
          ))}
        </div>

        {filteredLessons.length === 0 && (
          <p className="mt-8 text-center text-sm text-muted-foreground">
            No lessons in this category yet.
          </p>
        )}
      </div>
    </div>
  );
}
