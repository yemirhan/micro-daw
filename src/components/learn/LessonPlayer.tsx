import { ArrowLeft, ArrowRight, X, CheckCircle2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { LessonProgress as ProgressBar } from './LessonProgress';
import { LessonStepRenderer } from './LessonStepRenderer';
import type { Lesson, LessonStep } from '@/types/appMode';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { ChordInfo } from '@/types/music';
import type { DrumPadId } from '@/types/drums';

interface LessonPlayerProps {
  lesson: Lesson;
  currentStep: LessonStep;
  currentStepIndex: number;
  totalSteps: number;
  stepCompleted: boolean;
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  detectedChord: ChordInfo | null;
  onNext: () => void;
  onPrev: () => void;
  onExit: () => void;
  onQuizAnswer: (correct: boolean) => void;
}

export function LessonPlayer({
  lesson,
  currentStep,
  currentStepIndex,
  totalSteps,
  stepCompleted,
  activeNotes,
  onNoteOn,
  onNoteOff,
  activePads,
  onDrumHit,
  detectedChord,
  onNext,
  onPrev,
  onExit,
  onQuizAnswer,
}: LessonPlayerProps) {
  const isLast = currentStepIndex === totalSteps - 1;

  return (
    <div className="flex flex-1 flex-col overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border/50 px-6 py-3">
        <div className="flex items-center gap-3">
          <Button variant="ghost" size="sm" className="h-7 w-7 p-0" onClick={onExit} title="Exit lesson">
            <X className="h-4 w-4" />
          </Button>
          <h2 className="text-sm font-semibold">{lesson.title}</h2>
        </div>
        <div className="w-48">
          <ProgressBar current={currentStepIndex} total={totalSteps} />
        </div>
      </div>

      {/* Step content */}
      <div className="flex-1 overflow-y-auto px-6 py-6">
        <div className="mx-auto max-w-2xl">
          <h3 className="mb-4 text-lg font-bold">{currentStep.title}</h3>

          <LessonStepRenderer
            step={currentStep}
            completed={stepCompleted}
            activeNotes={activeNotes}
            onNoteOn={onNoteOn}
            onNoteOff={onNoteOff}
            activePads={activePads}
            onDrumHit={onDrumHit}
            detectedChord={detectedChord}
            onQuizAnswer={onQuizAnswer}
          />

          {/* Completion indicator */}
          {stepCompleted && currentStep.type !== 'info' && (
            <div className="mt-4 flex items-center gap-2 text-sm" style={{ color: 'oklch(0.75 0.20 150)' }}>
              <CheckCircle2 className="h-4 w-4" />
              <span>Step completed!</span>
            </div>
          )}
        </div>
      </div>

      {/* Navigation footer */}
      <div className="flex items-center justify-between border-t border-border/50 px-6 py-3">
        <Button
          variant="ghost"
          size="sm"
          onClick={onPrev}
          disabled={currentStepIndex === 0}
        >
          <ArrowLeft className="mr-1 h-3.5 w-3.5" />
          Back
        </Button>

        <Button
          size="sm"
          onClick={onNext}
          disabled={!stepCompleted}
        >
          {isLast ? 'Finish' : 'Next'}
          {!isLast && <ArrowRight className="ml-1 h-3.5 w-3.5" />}
        </Button>
      </div>
    </div>
  );
}
