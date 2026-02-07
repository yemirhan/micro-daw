interface AccuracyMeterProps {
  accuracy: number; // 0-1
}

export function AccuracyMeter({ accuracy }: AccuracyMeterProps) {
  const pct = Math.round(accuracy * 100);
  const hue = accuracy * 120; // 0=red, 120=green

  return (
    <div className="flex items-center gap-3">
      <span className="text-xs text-muted-foreground">Accuracy</span>
      <div className="h-2 w-24 rounded-full bg-secondary">
        <div
          className="h-full rounded-full transition-all duration-300"
          style={{
            width: `${pct}%`,
            backgroundColor: `oklch(0.65 0.20 ${hue})`,
          }}
        />
      </div>
      <span className="text-xs font-medium" style={{ color: `oklch(0.75 0.18 ${hue})` }}>
        {pct}%
      </span>
    </div>
  );
}
