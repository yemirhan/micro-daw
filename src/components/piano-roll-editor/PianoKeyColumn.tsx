import { isBlackKey, midiToNoteName } from '@/utils/noteHelpers';
import { PR_NOTE_MIN, PR_NOTE_MAX } from '@/utils/pianoRollHelpers';

interface PianoKeyColumnProps {
  rowHeight: number;
  scrollTop: number;
  visibleHeight: number;
}

export function PianoKeyColumn({ rowHeight, scrollTop, visibleHeight }: PianoKeyColumnProps) {
  const keys: JSX.Element[] = [];

  // Calculate visible range to only render what's needed
  const startNote = Math.max(PR_NOTE_MIN, PR_NOTE_MAX - Math.floor((scrollTop + visibleHeight) / rowHeight) - 2);
  const endNote = Math.min(PR_NOTE_MAX, PR_NOTE_MAX - Math.floor(scrollTop / rowHeight) + 2);

  for (let midi = endNote - 1; midi >= startNote; midi--) {
    const black = isBlackKey(midi);
    const name = midiToNoteName(midi);
    const y = (PR_NOTE_MAX - midi - 1) * rowHeight;
    const isC = midi % 12 === 0;

    keys.push(
      <div
        key={midi}
        className="absolute right-0 left-0 flex items-center justify-end border-b px-1.5 select-none"
        style={{
          top: y,
          height: rowHeight,
          backgroundColor: black ? 'oklch(0.18 0.01 270)' : 'oklch(0.22 0.005 270)',
          borderColor: 'oklch(0.30 0.01 270)',
        }}
      >
        <span
          className="font-mono text-[9px]"
          style={{
            color: isC ? 'oklch(0.75 0.08 265)' : 'oklch(0.55 0.02 270)',
            fontWeight: isC ? 600 : 400,
          }}
        >
          {name}
        </span>
      </div>
    );
  }

  return (
    <div className="relative" style={{ height: (PR_NOTE_MAX - PR_NOTE_MIN) * rowHeight }}>
      {keys}
    </div>
  );
}
