import { useCallback } from 'react';
import { Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { PianoRoll } from '@/components/PianoRoll';
import { VisualMetronome } from './VisualMetronome';
import { AccuracyMeter } from './AccuracyMeter';
import { useRhythmTrainer } from '@/hooks/useRhythmTrainer';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { RhythmFeedback } from '@/types/appMode';

interface RhythmTrainerProps {
  activeNotes: Map<number, ActiveNote>;
  onNoteOn: (note: number, velocity: number) => void;
  onNoteOff: (note: number) => void;
  bpm?: number;
  totalBeats?: number;
  subdivision?: '4n' | '8n';
  onComplete?: (accuracy: number, feedbackHistory: RhythmFeedback[]) => void;
}

export function RhythmTrainer({
  activeNotes,
  onNoteOn,
  onNoteOff,
  bpm = 120,
  totalBeats,
  subdivision = '4n',
  onComplete,
}: RhythmTrainerProps) {
  const trainer = useRhythmTrainer({ bpm, totalBeats, subdivision, onComplete });

  const handleNoteOn = useCallback(
    (note: number, velocity: number) => {
      onNoteOn(note, velocity);
      if (trainer.isRunning) {
        trainer.recordHit();
      }
    },
    [onNoteOn, trainer],
  );

  const lastFeedback = trainer.feedbackHistory[trainer.feedbackHistory.length - 1];

  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <div>
        <h2 className="text-sm font-semibold text-muted-foreground">Rhythm Training</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Play notes on the beat. The metronome shows you the tempo — try to match it!
          {totalBeats && ` (${totalBeats} beats at ${bpm} BPM)`}
        </p>
      </div>

      <div className="flex items-center gap-6">
        <Button
          size="sm"
          variant={trainer.isRunning ? 'destructive' : 'default'}
          onClick={trainer.isRunning ? trainer.stop : trainer.start}
        >
          {trainer.isRunning ? (
            <><Square className="mr-1 h-3.5 w-3.5" /> Stop</>
          ) : (
            <><Play className="mr-1 h-3.5 w-3.5" /> Start</>
          )}
        </Button>

        <VisualMetronome currentBeat={trainer.currentBeat} isRunning={trainer.isRunning} />

        {totalBeats && (
          <span className="text-xs text-muted-foreground">
            {Math.min(trainer.currentBeat, totalBeats)} / {totalBeats}
          </span>
        )}

        {trainer.feedbackHistory.length > 0 && (
          <AccuracyMeter accuracy={trainer.accuracy} />
        )}
      </div>

      {lastFeedback && (
        <div className="flex items-center gap-2">
          <span
            className="rounded-md px-2 py-1 text-xs font-medium"
            style={{
              backgroundColor:
                lastFeedback.timing === 'perfect' ? 'oklch(0.30 0.15 150)' :
                lastFeedback.timing === 'good' ? 'oklch(0.30 0.12 90)' :
                lastFeedback.timing === 'early' ? 'oklch(0.30 0.12 270)' :
                lastFeedback.timing === 'late' ? 'oklch(0.30 0.12 40)' :
                'oklch(0.30 0.12 25)',
            }}
          >
            {lastFeedback.timing.toUpperCase()}
          </span>
          <span className="text-xs text-muted-foreground">
            {Math.abs(Math.round(lastFeedback.offsetMs))}ms {lastFeedback.offsetMs < 0 ? 'early' : 'late'}
          </span>
        </div>
      )}

      <div className="overflow-x-auto">
        <PianoRoll
          activeNotes={activeNotes}
          onNoteOn={handleNoteOn}
          onNoteOff={onNoteOff}
          compact
        />
      </div>
    </div>
  );
}
