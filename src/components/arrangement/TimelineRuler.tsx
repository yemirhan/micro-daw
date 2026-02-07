import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';

interface TimelineRulerProps {
  lengthBeats: number;
  pxPerBeat: number;
  snapValue: number;
  onSeek: (beat: number) => void;
}

export function TimelineRuler({ lengthBeats, pxPerBeat, snapValue, onSeek }: TimelineRulerProps) {
  const totalWidth = beatToPx(lengthBeats, pxPerBeat);

  const markers: Array<{ beat: number; isBar: boolean; label: string }> = [];
  for (let beat = 0; beat <= lengthBeats; beat++) {
    if (beat % 4 === 0) {
      markers.push({ beat, isBar: true, label: `${beat / 4 + 1}` });
    } else if (pxPerBeat >= 20) {
      markers.push({ beat, isBar: false, label: '' });
    }
  }

  const handleClick = (e: React.MouseEvent) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
    onSeek(Math.max(0, beat));
  };

  return (
    <div
      className="relative h-6 cursor-pointer border-b border-border bg-card/80"
      style={{ width: totalWidth }}
      onClick={handleClick}
    >
      {markers.map((m) => (
        <div
          key={m.beat}
          className="absolute top-0"
          style={{ left: beatToPx(m.beat, pxPerBeat) }}
        >
          <div
            className={`w-px ${m.isBar ? 'h-6 bg-border' : 'h-3 bg-border/50'}`}
          />
          {m.isBar && (
            <span className="absolute left-1 top-0 select-none font-mono text-[10px] text-muted-foreground">
              {m.label}
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
