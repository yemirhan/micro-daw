import { useRef, useCallback, useEffect, useState } from 'react';
import { cn } from '@/lib/utils';

type KnobSize = 'sm' | 'md' | 'lg';

const SIZE_MAP: Record<KnobSize, number> = { sm: 32, md: 40, lg: 48 };

interface KnobProps {
  value: number;
  min: number;
  max: number;
  step?: number;
  defaultValue?: number;
  label?: string;
  formatValue?: (v: number) => string;
  size?: KnobSize;
  color?: string;
  onChange: (value: number) => void;
  onChangeEnd?: (value: number) => void;
  disabled?: boolean;
  className?: string;
}

const ARC_START = -135; // degrees from 12 o'clock
const ARC_END = 135;
const ARC_RANGE = ARC_END - ARC_START; // 270 degrees
const DRAG_PX = 200; // 200px vertical drag = full range

export function Knob({
  value,
  min,
  max,
  step = 1,
  defaultValue,
  label,
  formatValue,
  size = 'md',
  color = 'oklch(0.65 0.15 265)',
  onChange,
  onChangeEnd,
  disabled = false,
  className,
}: KnobProps) {
  const px = SIZE_MAP[size];
  const radius = px / 2 - 4;
  const strokeWidth = size === 'sm' ? 2.5 : 3;
  const center = px / 2;

  const dragRef = useRef<{ startY: number; startValue: number } | null>(null);
  const rafRef = useRef(0);
  const pendingValue = useRef(value);
  const knobRef = useRef<SVGSVGElement>(null);

  // Local display value during drag to avoid re-render lag
  const [displayValue, setDisplayValue] = useState(value);
  useEffect(() => {
    if (!dragRef.current) setDisplayValue(value);
  }, [value]);

  const clamp = useCallback(
    (v: number) => {
      const stepped = Math.round(v / step) * step;
      return Math.min(max, Math.max(min, stepped));
    },
    [min, max, step],
  );

  const normalize = useCallback((v: number) => (v - min) / (max - min), [min, max]);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (disabled || e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      dragRef.current = { startY: e.clientY, startValue: value };
      pendingValue.current = value;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
    },
    [disabled, value],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!dragRef.current) return;
      e.preventDefault();
      const dy = dragRef.current.startY - e.clientY;
      const range = max - min;
      const newVal = clamp(dragRef.current.startValue + (dy / DRAG_PX) * range);

      if (newVal !== pendingValue.current) {
        pendingValue.current = newVal;
        cancelAnimationFrame(rafRef.current);
        rafRef.current = requestAnimationFrame(() => {
          setDisplayValue(pendingValue.current);
          onChange(pendingValue.current);
        });
      }
    },
    [min, max, clamp, onChange],
  );

  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      if (!dragRef.current) return;
      dragRef.current = null;
      cancelAnimationFrame(rafRef.current);
      onChangeEnd?.(pendingValue.current);
    },
    [onChangeEnd],
  );

  const handleWheel = useCallback(
    (e: React.WheelEvent) => {
      if (disabled) return;
      e.stopPropagation();
      const range = max - min;
      const tickSize = Math.max(step, range * 0.015);
      const delta = e.deltaY < 0 ? tickSize : -tickSize;
      const newVal = clamp(value + delta);
      if (newVal !== value) onChange(newVal);
    },
    [disabled, min, max, step, value, clamp, onChange],
  );

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent) => {
      if (disabled || defaultValue === undefined) return;
      e.preventDefault();
      e.stopPropagation();
      onChange(defaultValue);
      onChangeEnd?.(defaultValue);
    },
    [disabled, defaultValue, onChange, onChangeEnd],
  );

  // Cleanup RAF on unmount
  useEffect(() => () => cancelAnimationFrame(rafRef.current), []);

  const norm = normalize(displayValue);
  const angleDeg = ARC_START + norm * ARC_RANGE;

  // Arc path helpers
  const toRad = (deg: number) => ((deg - 90) * Math.PI) / 180;
  const arcPath = (startDeg: number, endDeg: number, r: number) => {
    const start = { x: center + r * Math.cos(toRad(startDeg)), y: center + r * Math.sin(toRad(startDeg)) };
    const end = { x: center + r * Math.cos(toRad(endDeg)), y: center + r * Math.sin(toRad(endDeg)) };
    const largeArc = Math.abs(endDeg - startDeg) > 180 ? 1 : 0;
    return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 1 ${end.x} ${end.y}`;
  };

  // Indicator dot position
  const indicatorR = radius - (size === 'sm' ? 5 : 6);
  const ix = center + indicatorR * Math.cos(toRad(angleDeg));
  const iy = center + indicatorR * Math.sin(toRad(angleDeg));

  const display = formatValue ? formatValue(displayValue) : String(Math.round(displayValue * 100) / 100);

  return (
    <div
      className={cn(
        'flex flex-col items-center gap-0.5 select-none',
        disabled && 'opacity-40 pointer-events-none',
        className,
      )}
    >
      <svg
        ref={knobRef}
        width={px}
        height={px}
        className="cursor-grab active:cursor-grabbing"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onWheel={handleWheel}
        onDoubleClick={handleDoubleClick}
      >
        {/* Glow filter */}
        <defs>
          <filter id={`knob-glow-${label ?? ''}`} x="-50%" y="-50%" width="200%" height="200%">
            <feGaussianBlur in="SourceGraphic" stdDeviation="2" result="blur" />
            <feMerge>
              <feMergeNode in="blur" />
              <feMergeNode in="SourceGraphic" />
            </feMerge>
          </filter>
        </defs>

        {/* Background arc */}
        <path
          d={arcPath(ARC_START, ARC_END, radius)}
          fill="none"
          stroke="oklch(0.25 0.01 265)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
        />

        {/* Value arc */}
        {norm > 0.005 && (
          <path
            d={arcPath(ARC_START, angleDeg, radius)}
            fill="none"
            stroke={color}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
            filter={norm > 0.02 ? `url(#knob-glow-${label ?? ''})` : undefined}
          />
        )}

        {/* Indicator dot */}
        <circle cx={ix} cy={iy} r={size === 'sm' ? 2 : 2.5} fill={color} />
      </svg>

      {/* Value display */}
      <span className="text-[9px] font-mono text-muted-foreground leading-none whitespace-nowrap">
        {display}
      </span>

      {/* Label */}
      {label && (
        <span className="text-[9px] font-semibold uppercase tracking-wider text-muted-foreground/70 leading-none">
          {label}
        </span>
      )}
    </div>
  );
}
