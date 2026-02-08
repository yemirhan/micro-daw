interface LevelMeterProps {
  level: number; // dB value
  height?: number;
}

export function LevelMeter({ level, height = 120 }: LevelMeterProps) {
  // Map dB to 0-1 range: -60dB = 0, 0dB = 1
  const normalized = Math.max(0, Math.min(1, (level + 60) / 60));
  const fillHeight = normalized * height;

  // Color zones
  const getColor = (pos: number) => {
    const ratio = pos / height;
    if (ratio > 0.85) return 'oklch(0.55 0.22 25)'; // red
    if (ratio > 0.7) return 'oklch(0.75 0.17 90)'; // yellow
    return 'oklch(0.65 0.20 155)'; // green
  };

  return (
    <div
      className="relative w-3 rounded-sm overflow-hidden"
      style={{ height, backgroundColor: 'oklch(0.15 0.01 270)' }}
    >
      <div
        className="absolute bottom-0 w-full rounded-sm transition-[height] duration-75"
        style={{
          height: fillHeight,
          background: `linear-gradient(to top, ${getColor(0)}, ${getColor(fillHeight)})`,
        }}
      />
      {/* Peak markers */}
      <div className="absolute w-full" style={{ bottom: height * 0.7, height: 1, backgroundColor: 'oklch(0.75 0.17 90 / 0.3)' }} />
      <div className="absolute w-full" style={{ bottom: height * 0.85, height: 1, backgroundColor: 'oklch(0.55 0.22 25 / 0.3)' }} />
    </div>
  );
}
