interface VisualMetronomeProps {
  currentBeat: number;
  isRunning: boolean;
}

export function VisualMetronome({ currentBeat, isRunning }: VisualMetronomeProps) {
  const beatInBar = currentBeat % 4;

  return (
    <div className="flex items-center gap-2">
      {[0, 1, 2, 3].map((i) => (
        <div
          key={i}
          className="h-4 w-4 rounded-full transition-all duration-100"
          style={{
            backgroundColor:
              isRunning && beatInBar === i
                ? i === 0
                  ? 'oklch(0.75 0.25 25)'
                  : 'oklch(0.70 0.20 270)'
                : 'oklch(0.20 0.02 270)',
            boxShadow:
              isRunning && beatInBar === i
                ? `0 0 8px ${i === 0 ? 'oklch(0.75 0.25 25)' : 'oklch(0.70 0.20 270)'}`
                : 'none',
            transform: isRunning && beatInBar === i ? 'scale(1.3)' : 'scale(1)',
          }}
        />
      ))}
    </div>
  );
}
