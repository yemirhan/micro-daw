import { useLayoutEffect, useRef } from 'react';
import { DrumPads } from '@/components/DrumPads';
import { useNoteEvents } from '@/hooks/useNoteEvents';
import { DRUM_NOTE_TO_PAD, DRUM_SOUNDS } from '@/utils/constants';
import type { LessonStep } from '@/types/appMode';
import type { DrumPadId } from '@/types/drums';
import { NoteChip, useFlash } from './kit';
import { InstrumentWell, PlayStatus, StepDescription, StepHint } from './StepStage';

interface PlayDrumStepProps {
  step: LessonStep;
  activePads: Set<DrumPadId>;
  onDrumHit: (padId: DrumPadId, velocity: number) => void;
  completed: boolean;
}

const padName = (id: DrumPadId) => DRUM_SOUNDS[id]?.name ?? `Pad ${id + 1}`;

export function PlayDrumStep({ step, activePads, onDrumHit, completed }: PlayDrumStepProps) {
  const pads = (step.expectedDrumPattern?.pads ?? []) as DrumPadId[];
  const highlightedPads = new Set(pads);

  const { flash, trigger, clear } = useFlash();
  const latest = useRef({ pads, completed });
  useLayoutEffect(() => {
    latest.current = { pads, completed };
  });
  useNoteEvents((event) => {
    if (latest.current.completed) return;
    let padId: DrumPadId | undefined;
    if (event.type === 'drum') padId = event.padId;
    else if (event.type === 'noteon') padId = DRUM_NOTE_TO_PAD.get(event.note);
    if (padId === undefined) return;
    if (latest.current.pads.includes(padId)) clear();
    else trigger(padName(padId));
  });

  const prompt =
    pads.length > 1 ? (
      <>
        Hit{' '}
        {pads.map((p, i) => (
          <span key={p}>
            {i > 0 && ' + '}
            <span className="font-semibold text-note-target">{padName(p)}</span>
          </span>
        ))}{' '}
        at the same time
      </>
    ) : pads[0] !== undefined ? (
      <>
        Hit the <span className="font-semibold text-note-target">{padName(pads[0])}</span>
      </>
    ) : (
      'Hit the highlighted pads'
    );

  return (
    <div className="flex flex-col gap-5">
      <StepDescription>{step.description}</StepDescription>

      {pads.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
            {pads.length > 1 ? 'Hit together' : 'Target'}
          </span>
          <div className="flex flex-wrap gap-2">
            {pads.map((p) => (
              <NoteChip key={p} size="lg" state={completed ? 'done' : activePads.has(p) ? 'held' : 'current'} className="font-sans">
                {padName(p)}
              </NoteChip>
            ))}
          </div>
        </div>
      )}

      <PlayStatus completed={completed} pendingRelease={false} flash={flash} prompt={prompt} successText="Step complete. Great hit!" />

      {step.hint && !completed && <StepHint>{step.hint}</StepHint>}

      <InstrumentWell label={<span>Pads · MIDI drum pads work too</span>}>
        <div className="w-[480px] max-w-full">
          <DrumPads activePads={activePads} onHit={onDrumHit} highlightedPads={completed ? undefined : highlightedPads} />
        </div>
      </InstrumentWell>
    </div>
  );
}
