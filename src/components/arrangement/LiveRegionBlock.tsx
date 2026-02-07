import type { Region, RegionNote } from '@/types/arrangement';
import { beatToPx } from '@/utils/arrangementHelpers';
import { PIANO_START } from '@/utils/constants';

interface LiveRegionBlockProps {
  region: Region;
  trackColor: string;
  pxPerBeat: number;
}

function LiveNoteMinimap({ notes, lengthBeats, isDrum }: { notes: RegionNote[]; lengthBeats: number; isDrum: boolean }) {
  if (notes.length === 0) return null;

  const minNote = isDrum ? 36 : Math.min(...notes.map((n) => n.note), PIANO_START);
  const maxNote = isDrum ? 43 : Math.max(...notes.map((n) => n.note), PIANO_START + 12);
  const range = Math.max(maxNote - minNote + 1, 4);

  return (
    <div className="absolute inset-0.5 overflow-hidden">
      {notes.map((note, i) => {
        const left = `${(note.startBeat / lengthBeats) * 100}%`;
        const width = `${Math.max((note.durationBeats / lengthBeats) * 100, 1)}%`;
        const bottom = `${((note.note - minNote) / range) * 100}%`;
        const height = `${Math.max(100 / range, 4)}%`;

        return (
          <div
            key={i}
            className="absolute rounded-[1px] bg-white/70"
            style={{ left, width, bottom, height: Math.max(2, parseFloat(height)) }}
          />
        );
      })}
    </div>
  );
}

export function LiveRegionBlock({ region, trackColor, pxPerBeat }: LiveRegionBlockProps) {
  const left = beatToPx(region.startBeat, pxPerBeat);
  const width = beatToPx(region.lengthBeats, pxPerBeat);
  const color = region.color || trackColor;
  const isDrum = region.notes.some((n) => n.isDrum);

  return (
    <div
      className="absolute top-0.5 bottom-0.5 select-none rounded-sm border border-white/30"
      style={{
        left,
        width: Math.max(width, 4),
        backgroundColor: color,
        opacity: 0.75,
        boxShadow: `0 0 8px 1px ${color}80`,
      }}
    >
      <span className="absolute left-1 top-0 truncate text-[9px] font-medium text-white/90">
        REC
      </span>
      <LiveNoteMinimap notes={region.notes} lengthBeats={region.lengthBeats} isDrum={isDrum} />
    </div>
  );
}
