import { useEffect, useState } from 'react';
import * as Tone from 'tone';
import { padRows } from '@/components/beat/beatHelpers';
import { cn } from '@/lib/utils';
import type { PadIndex } from '@/types/sampler';
import { kitPreview, padsAt } from './kitPreview';

const ROWS = padRows('4x4');
const NONE: ReadonlySet<PadIndex> = new Set();

/**
 * Pads lit in time with the kit preview whose key starts with `prefix`
 * (empty set when nothing of this item is playing).
 */
export function usePreviewFlash(prefix: string | null): ReadonlySet<PadIndex> {
  const [pads, setPads] = useState<ReadonlySet<PadIndex>>(NONE);
  useEffect(() => {
    if (!prefix) return;
    let raf = 0;
    let lastKey = '';
    const tick = () => {
      const session = kitPreview.session;
      const now = session && session.id.startsWith(prefix) ? padsAt(session, Tone.immediate()) : NONE;
      const key = [...now].sort((a, b) => a - b).join(',');
      if (key !== lastKey) {
        lastKey = key;
        setPads(now);
      }
      raf = requestAnimationFrame(tick);
    };
    const start = () => {
      cancelAnimationFrame(raf);
      if (kitPreview.playing?.startsWith(prefix)) raf = requestAnimationFrame(tick);
      else {
        lastKey = '';
        setPads(NONE);
      }
    };
    start();
    const unsub = kitPreview.subscribe(start);
    return () => {
      unsub();
      cancelAnimationFrame(raf);
    };
  }, [prefix]);
  return prefix ? pads : NONE;
}

interface FaceplateProps {
  color: string;
  /** Which of the 16 pads hold a sound. */
  lit: readonly boolean[];
  /** Pads flashing right now (kit preview). */
  hits?: ReadonlySet<PadIndex>;
  size?: 'sm' | 'lg';
  className?: string;
}

/**
 * A pack's "faceplate": the 16 pads in the MPC layout (pad 1 bottom-left),
 * tinted with the pack's colour. Pads with a sound glow softly; pads hit by
 * the kit preview flash.
 */
export function Faceplate({ color, lit, hits = NONE, size = 'sm', className }: FaceplateProps) {
  return (
    <div
      aria-hidden
      className={cn(
        'grid shrink-0 grid-cols-4 rounded-md border border-hairline-strong bg-surface-0 shadow-[inset_0_1px_3px_oklch(0_0_0/0.45)]',
        size === 'sm' ? 'size-[68px] gap-[3px] p-[5px]' : 'size-[132px] gap-[5px] p-2',
        className,
      )}
      style={{ ['--pack' as string]: color }}
    >
      {ROWS.flat().map((pad) => {
        const on = lit[pad];
        const hit = hits.has(pad);
        return (
          <span
            key={pad}
            className={cn(
              'rounded-[2px] transition-[background-color,box-shadow] duration-75',
              size === 'lg' && 'rounded-[3px]',
              !on && 'bg-surface-2',
            )}
            style={
              on
                ? {
                    backgroundColor: hit
                      ? 'color-mix(in oklch, var(--pack) 70%, white)'
                      : `color-mix(in oklch, var(--pack) ${pad < 8 ? 62 : 46}%, var(--surface-1))`,
                    boxShadow: hit
                      ? '0 0 10px color-mix(in oklch, var(--pack) 70%, transparent), inset 0 1px 0 oklch(1 0 0 / 0.25)'
                      : 'inset 0 1px 0 oklch(1 0 0 / 0.14), inset 0 -1px 0 oklch(0 0 0 / 0.25)',
                  }
                : undefined
            }
          />
        );
      })}
    </div>
  );
}
