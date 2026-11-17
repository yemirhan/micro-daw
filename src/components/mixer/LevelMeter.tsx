interface LevelMeterProps {
  level: number; // dB value
  height?: number;
  width?: number;
}

const FLOOR_DB = -60;

/** Peak meter: green up to -12 dB, amber to -3 dB, red above. */
export function LevelMeter({ level, height = 120, width = 6 }: LevelMeterProps) {
  const normalized = Number.isFinite(level) ? Math.max(0, Math.min(1, (level - FLOOR_DB) / -FLOOR_DB)) : 0;
  const yellowAt = (-12 - FLOOR_DB) / -FLOOR_DB; // 0.8
  const redAt = (-3 - FLOOR_DB) / -FLOOR_DB; // 0.95

  return (
    <div
      className="relative overflow-hidden rounded-[2px] bg-surface-0 shadow-[inset_0_1px_2px_oklch(0_0_0/0.6)]"
      style={{ height, width }}
      aria-hidden
    >
      {/* Full-scale colour ramp, revealed from the bottom (transform only). */}
      <div
        className="absolute inset-0 origin-bottom transition-transform duration-75 ease-out"
        style={{
          transform: `scaleY(${normalized})`,
          background: `linear-gradient(to top, var(--signal-play) 0%, var(--signal-play) ${yellowAt * 100}%, var(--signal-warn) ${yellowAt * 100}%, var(--signal-warn) ${redAt * 100}%, var(--signal-rec) ${redAt * 100}%)`,
        }}
      />
      {/* LED segment gaps */}
      <div
        className="pointer-events-none absolute inset-0"
        style={{ backgroundImage: 'repeating-linear-gradient(to top, transparent 0 3px, var(--surface-0) 3px 4px)' }}
      />
    </div>
  );
}

/** dB scale labels next to a meter of the same height. */
export function MeterScale({ height, align = 'right' }: { height: number; align?: 'left' | 'right' }) {
  const marks = [0, -6, -12, -24, -48];
  return (
    <div className="relative w-5" style={{ height }} aria-hidden>
      {marks.map((db) => (
        <span
          key={db}
          className={`absolute ${align === 'left' ? 'left-0' : 'right-0'} -translate-y-1/2 font-mono text-[8.5px] leading-none text-muted-foreground/70`}
          style={{ top: ((db / FLOOR_DB) * height) }}
        >
          {db === 0 ? '0' : db}
        </span>
      ))}
    </div>
  );
}
