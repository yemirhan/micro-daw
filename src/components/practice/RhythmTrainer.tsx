import { Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { PianoRoll } from '@/components/PianoRoll';
import { Panel, ProgressBar, Readout } from '@/components/learn/kit';
import { InstrumentWell, StepDescription } from '@/components/learn/StepStage';
import { VisualMetronome } from './VisualMetronome';
import { AccuracyMeter } from './AccuracyMeter';
import { TimingFeedback } from './TimingFeedback';
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
  /** Exercise instructions (replaces the default text). */
  instructions?: string;
}

export function RhythmTrainer({
  activeNotes,
  onNoteOn,
  onNoteOff,
  bpm = 120,
  totalBeats,
  subdivision = '4n',
  onComplete,
  instructions,
}: RhythmTrainerProps) {
  // Hits come from the shared note event bus: MIDI keyboard and on-screen keys alike.
  const trainer = useRhythmTrainer({ bpm, totalBeats, subdivision, onComplete, input: 'keys' });
  const ticksPerBar = subdivision === '8n' ? 8 : 4;

  return (
    <div className="mx-auto flex w-full max-w-[1080px] flex-col gap-5 px-10 py-8">
      <StepDescription>
        {instructions ??
          'Play any key on every click. Watch the beat lights and aim for the center of the timing lane.'}
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
          <Readout label="Note">{subdivision === '8n' ? '1/8' : '1/4'}</Readout>
          {totalBeats !== undefined && (
            <Readout label="Beat">
              {Math.min(trainer.beatsPlayed, totalBeats)}
              <span className="text-muted-foreground">/{totalBeats}</span>
            </Readout>
          )}
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

        <div className="flex flex-wrap items-center gap-8">
          <VisualMetronome currentBeat={trainer.currentBeat} isRunning={trainer.isRunning} ticksPerBar={ticksPerBar} size="lg" />
          <div className="min-w-[360px] flex-1">
            <TimingFeedback history={trainer.feedbackHistory} isRunning={trainer.isRunning} />
          </div>
        </div>
      </Panel>

      <InstrumentWell>
        <PianoRoll activeNotes={activeNotes} onNoteOn={onNoteOn} onNoteOff={onNoteOff} compact />
      </InstrumentWell>
    </div>
  );
}
