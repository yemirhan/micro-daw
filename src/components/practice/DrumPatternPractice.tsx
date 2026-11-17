import { Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { DrumPads } from '@/components/DrumPads';
import { Panel, ProgressBar, Readout } from '@/components/learn/kit';
import { InstrumentWell, StepDescription } from '@/components/learn/StepStage';
import { cn } from '@/lib/utils';
import { DRUM_SOUNDS } from '@/utils/constants';
import { VisualMetronome } from './VisualMetronome';
import { AccuracyMeter } from './AccuracyMeter';
import { TimingFeedback } from './TimingFeedback';
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
  instructions?: string;
}

// Default rock pattern
const DEFAULT_PATTERN: number[][] = [
  [0, 2], // Beat 1: kick + hi-hat
  [1, 2], // Beat 2: snare + hi-hat
  [0, 2], // Beat 3: kick + hi-hat
  [1, 2], // Beat 4: snare + hi-hat
];

/** The pattern as a step grid: one row per pad used, one column per beat. */
function PatternGrid({ pattern, currentStep, name }: { pattern: number[][]; currentStep: number; name: string }) {
  const pads = [...new Set(pattern.flat())].sort((a, b) => b - a) as DrumPadId[];
  return (
    <div>
      <div className="mb-2 flex items-baseline justify-between">
        <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Pattern · {name}</span>
        <span className="font-mono text-[11px] text-muted-foreground">{pattern.length} steps</span>
      </div>
      <div
        className="grid items-center gap-1"
        style={{ gridTemplateColumns: `88px repeat(${pattern.length}, minmax(0, 1fr))` }}
        role="table"
        aria-label={`${name} pattern`}
      >
        <span />
        {pattern.map((_, i) => (
          <span
            key={`h${i}`}
            className={cn(
              'text-center font-mono text-[10px] tabular-nums',
              i === currentStep ? 'font-semibold text-primary' : 'text-muted-foreground',
            )}
          >
            {i + 1}
          </span>
        ))}
        {pads.map((pad) => (
          <div key={pad} className="contents" role="row">
            <span className="truncate pr-2 text-xs font-medium text-foreground/85" role="rowheader">
              {DRUM_SOUNDS[pad]?.name ?? `Pad ${pad + 1}`}
            </span>
            {pattern.map((hits, i) => {
              const on = hits.includes(pad);
              const now = i === currentStep;
              return (
                <span
                  key={i}
                  role="cell"
                  aria-label={on ? `Beat ${i + 1}: hit` : `Beat ${i + 1}: rest`}
                  className={cn(
                    'h-7 rounded border transition-colors duration-75',
                    on
                      ? now
                        ? 'border-primary bg-primary'
                        : 'border-foreground/20 bg-foreground/70'
                      : now
                        ? 'border-primary/40 bg-primary/10'
                        : 'border-border bg-surface-2',
                    i % 4 === 0 && !on && !now && 'bg-surface-3',
                  )}
                />
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

export function DrumPatternPractice({
  activePads,
  onDrumHit,
  pattern = DEFAULT_PATTERN,
  bpm = 100,
  totalBeats,
  patternName = 'Rock Beat',
  onComplete,
  instructions,
}: DrumPatternPracticeProps) {
  // Hits (MIDI pads and on-screen pads) come from the shared note event bus
  // and are judged per pad against the pattern.
  const trainer = useRhythmTrainer({
    bpm,
    totalBeats,
    onComplete,
    pattern: pattern as DrumPadId[][],
    input: 'drums',
  });

  // Highlight the pads expected on the current beat
  const beatInBar = trainer.currentBeat >= 0 ? trainer.currentBeat % pattern.length : -1;
  const currentHighlight = new Set((pattern[beatInBar] ?? []) as DrumPadId[]);

  return (
    <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-5 px-10 py-8">
      <StepDescription>
        {instructions ?? 'Hit the lit pads in time with the click. The grid shows the whole groove; the orange column is now.'}
      </StepDescription>

      <Panel className="flex flex-col gap-6 p-5">
        <div className="flex flex-wrap items-center gap-3">
          <Button
            size="lg"
            variant={trainer.isRunning ? 'secondary' : 'default'}
            onClick={trainer.isRunning ? trainer.stop : trainer.start}
            className="h-11 min-w-28 font-semibold"
          >
            {trainer.isRunning ? (
              <>
                <Square className="size-4 fill-current" /> Stop
              </>
            ) : (
              <>
                <Play className="size-4 fill-current" /> Start
              </>
            )}
          </Button>
          <Readout label="Tempo">
            {bpm}
            <span className="ml-1 text-[10px] text-muted-foreground">BPM</span>
          </Readout>
          {totalBeats !== undefined && (
            <Readout label="Beat">
              {Math.min(trainer.beatsPlayed, totalBeats)}
              <span className="text-muted-foreground">/{totalBeats}</span>
            </Readout>
          )}
          <VisualMetronome currentBeat={trainer.currentBeat} isRunning={trainer.isRunning} />
          <div className="ml-auto">
            {trainer.feedbackHistory.length > 0 ? (
              <AccuracyMeter accuracy={trainer.accuracy} />
            ) : (
              <span className="text-xs text-muted-foreground">Accuracy appears after your first beat</span>
            )}
          </div>
        </div>

        {totalBeats !== undefined && (
          <ProgressBar value={totalBeats ? Math.min(trainer.beatsPlayed, totalBeats) / totalBeats : 0} label="Exercise progress" />
        )}

        <PatternGrid pattern={pattern} currentStep={trainer.isRunning ? beatInBar : -1} name={patternName} />
        <TimingFeedback history={trainer.feedbackHistory} isRunning={trainer.isRunning} />
      </Panel>

      <InstrumentWell label={<span>Pads · MIDI drum pads work too</span>}>
        <div className="w-[520px] max-w-full">
          <DrumPads
            activePads={activePads}
            onHit={onDrumHit}
            highlightedPads={trainer.isRunning ? currentHighlight : undefined}
          />
        </div>
      </InstrumentWell>
    </div>
  );
}
