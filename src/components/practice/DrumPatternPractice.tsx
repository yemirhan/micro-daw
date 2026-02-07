import { useCallback } from 'react';
import { Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { DrumPads } from '@/components/DrumPads';
import { VisualMetronome } from './VisualMetronome';
import { AccuracyMeter } from './AccuracyMeter';
import { useRhythmTrainer } from '@/hooks/useRhythmTrainer';
import type { DrumPadId } from '@/types/drums';
import type { RhythmFeedback } from '@/types/appMode';

interface DrumPatternPracticeProps {
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  pattern?: number[][];
  bpm?: number;
  totalBeats?: number;
  patternName?: string;
  onComplete?: (accuracy: number, feedbackHistory: RhythmFeedback[]) => void;
}

const PAD_NAMES = ['KCK', 'SNR', 'CHH', 'OHH', 'CLP', 'TOM', 'CRS', 'RDE'];

// Default rock pattern
const DEFAULT_PATTERN: number[][] = [
  [0, 2], // Beat 1: kick + hi-hat
  [1, 2], // Beat 2: snare + hi-hat
  [0, 2], // Beat 3: kick + hi-hat
  [1, 2], // Beat 4: snare + hi-hat
];

export function DrumPatternPractice({
  activePads,
  onDrumHit,
  pattern = DEFAULT_PATTERN,
  bpm = 100,
  totalBeats,
  patternName = 'Rock Beat',
  onComplete,
}: DrumPatternPracticeProps) {
  const trainer = useRhythmTrainer({ bpm, totalBeats, onComplete });

  // Update highlighted pads based on current beat
  const beatInBar = trainer.currentBeat % pattern.length;
  const currentHighlight = new Set((pattern[beatInBar] ?? []) as DrumPadId[]);

  const handleDrumHit = useCallback(
    (padId: DrumPadId, velocity: number) => {
      onDrumHit(padId, velocity);
      if (trainer.isRunning) {
        trainer.recordHit();
      }
    },
    [onDrumHit, trainer],
  );

  return (
    <div className="flex flex-1 flex-col gap-6 p-6">
      <div>
        <h2 className="text-sm font-semibold text-muted-foreground">Drum Pattern Practice</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          Follow the highlighted drum pads. Hit them in time with the metronome!
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

      {/* Pattern reference */}
      <div className="rounded-lg p-3" style={{ backgroundColor: 'oklch(0.14 0.01 270)' }}>
        <p className="mb-2 text-xs font-medium text-muted-foreground">{patternName} Pattern:</p>
        <div className="grid gap-2 text-center text-xs" style={{ gridTemplateColumns: `repeat(${pattern.length}, 1fr)` }}>
          {pattern.map((pads, i) => (
            <div
              key={i}
              className="rounded-md py-1 transition-colors"
              style={{
                backgroundColor: trainer.isRunning && beatInBar === i
                  ? 'oklch(0.25 0.10 270)'
                  : 'oklch(0.18 0.01 270)',
              }}
            >
              <div className="font-medium">Beat {i + 1}</div>
              <div className="text-muted-foreground">
                {pads.map((p) => PAD_NAMES[p]).join(' + ')}
              </div>
            </div>
          ))}
        </div>
      </div>

      <div className="w-[320px]">
        <DrumPads
          activePads={activePads}
          onHit={handleDrumHit}
          highlightedPads={trainer.isRunning ? currentHighlight : undefined}
        />
      </div>
    </div>
  );
}
