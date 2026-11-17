import type { Region } from '@/types/arrangement';
import { beatToPx } from '@/utils/arrangementHelpers';
import { NoteMinimap } from './RegionBlock';

interface LiveRegionBlockProps {
  region: Region;
  trackColor: string;
  pxPerBeat: number;
}

/** The region being recorded right now. */
export function LiveRegionBlock({ region, trackColor, pxPerBeat }: LiveRegionBlockProps) {
  const left = beatToPx(region.startBeat, pxPerBeat);
  const width = beatToPx(region.lengthBeats, pxPerBeat);
  const color = region.color || trackColor;
  const isDrum = region.notes.some((n) => n.isDrum);

  return (
    <div
      className="pointer-events-none absolute top-[3px] bottom-[3px] z-[3] flex select-none flex-col overflow-hidden rounded-[5px]"
      style={{
        left,
        width: Math.max(width, 4),
        backgroundColor: `color-mix(in oklab, var(--signal-rec) 16%, var(--surface-1))`,
        boxShadow: 'inset 0 0 0 1px color-mix(in oklab, var(--signal-rec) 70%, transparent)',
      }}
    >
      <div className="flex h-[15px] shrink-0 items-center gap-1 bg-signal-rec px-1.5">
        <span className="size-1.5 animate-rec-pulse rounded-full bg-white" />
        <span className="text-[10px] font-bold leading-none tracking-[0.06em] text-white">REC</span>
      </div>
      <div className="relative min-h-0 flex-1">
        <NoteMinimap notes={region.notes} lengthBeats={region.lengthBeats} isDrum={isDrum} color={color} />
      </div>
    </div>
  );
}
