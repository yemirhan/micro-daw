import { beatToPx } from '@/utils/arrangementHelpers';

interface PlayheadProps {
  position: number;
  pxPerBeat: number;
  height: number;
}

export function Playhead({ position, pxPerBeat, height }: PlayheadProps) {
  const x = beatToPx(position, pxPerBeat);

  return (
    <div
      className="pointer-events-none absolute top-0 z-30 w-px bg-red-500"
      style={{
        left: x,
        height,
        boxShadow: '0 0 6px 2px oklch(0.58 0.22 25 / 0.30)',
      }}
    >
      <div className="absolute -left-1 -top-0.5 h-2 w-2 rotate-45 bg-red-500" />
    </div>
  );
}
