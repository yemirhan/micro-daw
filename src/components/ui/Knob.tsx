import { useRef, useCallback, useEffect, useState } from 'react';
import { cn } from '@/lib/utils';

type KnobSize = 'sm' | 'md' | 'lg';

const SIZE_MAP: Record<KnobSize, number> = { sm: 34, md: 42, lg: 52 };

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
  color = 'var(--primary)',
  onChange,
  onChangeEnd,
  disabled = false,
  className,
}: KnobProps) {
  const px = SIZE_MAP[size];
  const strokeWidth = size === 'sm' ? 2.5 : 3;
  const radius = px / 2 - strokeWidth / 2 - 1;
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
      const newVal = clamp(dragRef.current.startValue + (dy / (e.shiftKey ? DRAG_PX * 4 : DRAG_PX)) * range);

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
    () => {
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

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (disabled) return;
      const range = max - min;
      const tick = Math.max(step, range * (e.shiftKey ? 0.1 : 0.01));
      let next: number | null = null;
      if (e.key === 'ArrowUp' || e.key === 'ArrowRight') next = clamp(value + tick);
      else if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') next = clamp(value - tick);
      else if (e.key === 'Home') next = min;
      else if (e.key === 'End') next = max;
      else if ((e.key === 'Delete' || e.key === 'Backspace') && defaultValue !== undefined) next = defaultValue;
      if (next === null) return;
      e.preventDefault();
      e.stopPropagation();
      if (next !== value) {
        onChange(next);
        onChangeEnd?.(next);
      }
    },
    [disabled, min, max, step, value, clamp, defaultValue, onChange, onChangeEnd],
  );

  const norm = Math.min(1, Math.max(0, normalize(displayValue)));
  const angleDeg = ARC_START + norm * ARC_RANGE;
  // Bipolar ranges (e.g. EQ -12..+12, pan) draw the value arc from zero.
  const bipolar = min < 0 && max > 0;
  const originDeg = bipolar ? ARC_START + normalize(0) * ARC_RANGE : ARC_START;

  // Arc path helpers
  const toRad = (deg: number) => ((deg - 90) * Math.PI) / 180;
  const arcPath = (startDeg: number, endDeg: number, r: number) => {
    const [a, b] = startDeg <= endDeg ? [startDeg, endDeg] : [endDeg, startDeg];
    const start = { x: center + r * Math.cos(toRad(a)), y: center + r * Math.sin(toRad(a)) };
    const end = { x: center + r * Math.cos(toRad(b)), y: center + r * Math.sin(toRad(b)) };
    const largeArc = Math.abs(b - a) > 180 ? 1 : 0;
    return `M ${start.x} ${start.y} A ${r} ${r} 0 ${largeArc} 1 ${end.x} ${end.y}`;
  };

  // Knob body + pointer line
  const bodyR = radius - strokeWidth - 1.5;
  const pInner = bodyR * 0.3;
  const pOuter = bodyR - 1.5;
  const px1 = center + pInner * Math.cos(toRad(angleDeg));
  const py1 = center + pInner * Math.sin(toRad(angleDeg));
  const px2 = center + pOuter * Math.cos(toRad(angleDeg));
  const py2 = center + pOuter * Math.sin(toRad(angleDeg));

  const display = formatValue ? formatValue(displayValue) : String(Math.round(displayValue * 100) / 100);
  const showArc = Math.abs(angleDeg - originDeg) > 0.5;

  return (
    <div
      className={cn(
        'group/knob flex flex-col items-center gap-1 select-none',
        disabled && 'opacity-40 pointer-events-none',
        className,
      )}
    >
      <svg
        ref={knobRef}
        width={px}
        height={px}
        role="slider"
        tabIndex={disabled ? -1 : 0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={displayValue}
        aria-valuetext={display}
        className="cursor-ns-resize rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring/70"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onWheel={handleWheel}
        onDoubleClick={handleDoubleClick}
        onKeyDown={handleKeyDown}
      >
        {/* Track */}
        <path
          d={arcPath(ARC_START, ARC_END, radius)}
          fill="none"
          stroke="var(--surface-4)"
          strokeWidth={strokeWidth}
          strokeLinecap="round"
        />

        {/* Value arc */}
        {showArc && (
          <path
            d={arcPath(originDeg, angleDeg, radius)}
            fill="none"
            stroke={color}
            strokeWidth={strokeWidth}
            strokeLinecap="round"
          />
        )}

        {/* Body */}
        <circle cx={center} cy={center} r={bodyR} fill="var(--surface-3)" stroke="oklch(0 0 0 / 0.45)" strokeWidth={1} />
        <circle
          cx={center}
          cy={center}
          r={bodyR - 1}
          fill="none"
          stroke="oklch(1 0 0 / 0.07)"
          strokeWidth={1}
          className="transition-[stroke] duration-150 group-hover/knob:stroke-[oklch(1_0_0/0.16)]"
        />

        {/* Pointer */}
        <line x1={px1} y1={py1} x2={px2} y2={py2} stroke="var(--foreground)" strokeWidth={size === 'sm' ? 1.75 : 2} strokeLinecap="round" />
      </svg>

      {/* Value display */}
      <span className="font-mono text-[10px] leading-none whitespace-nowrap text-foreground/85 tabular-nums">
        {display}
      </span>

      {/* Label */}
      {label && (
        <span className="text-[9.5px] font-bold uppercase leading-none tracking-[0.08em] text-muted-foreground">
          {label}
        </span>
      )}
    </div>
  );
}
