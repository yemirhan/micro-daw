import { DRUM_ROWS } from '@/utils/pianoRollHelpers';

interface DrumKeyColumnProps {
  rowHeight: number;
}

export function DrumKeyColumn({ rowHeight }: DrumKeyColumnProps) {
  return (
    <div className="relative" style={{ height: DRUM_ROWS.length * rowHeight }}>
      {DRUM_ROWS.map((drum, i) => (
        <div
          key={drum.midiNote}
          className="absolute right-0 left-0 flex items-center justify-end border-b px-1.5 select-none"
          style={{
            top: i * rowHeight,
            height: rowHeight,
            backgroundColor: i % 2 === 0 ? 'oklch(0.20 0.01 270)' : 'oklch(0.18 0.01 270)',
            borderColor: 'oklch(0.30 0.01 270)',
          }}
        >
          <span
            className="font-mono text-[9px] font-semibold"
            style={{ color: drum.color }}
          >
            {drum.name}
          </span>
        </div>
      ))}
    </div>
  );
}
