import { useLayoutEffect, useRef, type ReactNode } from 'react';
import { Lightbulb } from 'lucide-react';
import { cn } from '@/lib/utils';
import { useNoteEvents } from '@/hooks/useNoteEvents';
import { midiToNoteName } from '@/utils/noteHelpers';
import { StatusLine, useFlash, type StatusTone } from './kit';

/** Lesson / exercise instruction text: large and readable. */
export function StepDescription({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <p className={cn('max-w-[68ch] whitespace-pre-line text-base leading-relaxed text-foreground/85', className)}>
      {children}
    </p>
  );
}

export function StepHint({ children }: { children: ReactNode }) {
  return (
    <p className="flex max-w-[68ch] items-start gap-2 text-[13px] leading-relaxed text-muted-foreground">
      <Lightbulb className="mt-0.5 size-3.5 shrink-0 text-signal-warn/80" aria-hidden />
      <span>{children}</span>
    </p>
  );
}

/** The instrument area: a well that holds the keyboard / pads. */
export function InstrumentWell({ children, className, label }: { children: ReactNode; className?: string; label?: ReactNode }) {
  return (
    <div className={cn('rounded-xl border border-border bg-surface-1 p-4 shadow-[inset_0_1px_0_var(--hairline)]', className)}>
      {label && (
        <div className="mb-3 flex items-center justify-between text-[11px] font-bold uppercase tracking-[0.08em] text-muted-foreground">
          {label}
        </div>
      )}
      <div className="flex justify-center overflow-x-auto">{children}</div>
    </div>
  );
}

/** Note name, without the octave when the step accepts any octave. */
export function noteLabel(midi: number, anyOctave = false): string {
  const name = midiToNoteName(midi);
  return anyOctave ? name.replace(/-?\d+$/, '') : name;
}

/**
 * Watch live input (MIDI or on-screen) and flash a "not quite" message when a
 * played note doesn't belong to the step. `isWrong` may change every render.
 */
export function useWrongNoteFlash(isWrong: (note: number) => boolean, enabled: boolean, describe?: (note: number) => string) {
  const { flash, trigger, clear } = useFlash();
  const latest = useRef({ isWrong, describe, enabled });
  useLayoutEffect(() => {
    latest.current = { isWrong, describe, enabled };
  });

  useNoteEvents((event) => {
    if (event.type !== 'noteon' || !latest.current.enabled) return;
    if (latest.current.isWrong(event.note)) {
      trigger((latest.current.describe ?? midiToNoteName)(event.note));
    } else {
      clear();
    }
  });

  return flash;
}

/** Status line for a play step: prompt, wrong-note flash, release, done. */
export function PlayStatus({
  completed,
  pendingRelease,
  flash,
  prompt,
  successText = 'Step complete. Nice playing!',
}: {
  completed: boolean;
  pendingRelease: boolean;
  flash: { label: string; key: number } | null;
  prompt: ReactNode;
  successText?: ReactNode;
}) {
  let tone: StatusTone = 'prompt';
  let content: ReactNode = prompt;
  let testId: string | undefined;
  if (completed) {
    tone = 'success';
    content = successText;
  } else if (pendingRelease) {
    tone = 'success';
    content = 'Got it! Release the keys to continue.';
    testId = 'release-keys-hint';
  } else if (flash) {
    tone = 'error';
    content = (
      <>
        Not quite: that was <span className="font-mono font-semibold">{flash.label}</span>. {prompt}
      </>
    );
  }
  return (
    <StatusLine tone={tone} testId={testId} flashKey={tone === 'error' ? flash?.key : tone}>
      {content}
    </StatusLine>
  );
}
