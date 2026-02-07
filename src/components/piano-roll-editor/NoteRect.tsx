import { memo } from 'react';

interface NoteRectProps {
  x: number;
  y: number;
  width: number;
  height: number;
  color: string;
  selected: boolean;
  velocity: number;
}

export const NoteRect = memo(function NoteRect({
  x,
  y,
  width,
  height,
  color,
  selected,
  velocity,
}: NoteRectProps) {
  const opacity = 0.5 + velocity * 0.5;

  return (
    <div
      className="absolute rounded-[2px] border pointer-events-none"
      style={{
        left: x,
        top: y + 1,
        width: Math.max(width, 3),
        height: height - 2,
        backgroundColor: color,
        opacity,
        borderColor: selected ? 'white' : 'rgba(255,255,255,0.25)',
        borderWidth: selected ? 2 : 1,
        boxShadow: selected ? '0 0 6px rgba(255,255,255,0.3)' : undefined,
      }}
    >
      {/* Resize handle on right edge */}
      <div
        className="absolute top-0 right-0 bottom-0 pointer-events-auto cursor-col-resize"
        style={{ width: 6 }}
      />
    </div>
  );
});
