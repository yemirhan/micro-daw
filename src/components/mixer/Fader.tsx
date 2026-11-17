import { useRef } from 'react';
import { cn } from '@/lib/utils';

interface FaderProps {
  value: number;
  min: number;
  max: number;
  /** Double-click resets here. */
  defaultValue?: number;
  height: number;
  onChange: (value: number) => void;
  label: string;
  className?: string;
}

/** Vertical mixer fader with a cap: drag, arrow keys (Shift = coarse), double-click to reset. */
export function Fader({ value, min, max, defaultValue = 0, height, onChange, label, className }: FaderProps) {
  const drag = useRef<{ y: number; start: number } | null>(null);
  const CAP = 22;
  const travel = height - CAP;
  const norm = (Math.min(max, Math.max(min, value)) - min) / (max - min);
  const capTop = (1 - norm) * travel;
  const clamp = (v: number) => Math.round(Math.min(max, Math.max(min, v)));
  const zeroTop = ((max - 0) / (max - min)) * travel + CAP / 2;

  return (
    <div
      role="slider"
      tabIndex={0}
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={value}
      aria-valuetext={`${value} dB`}
      className={cn('group/fader relative w-7 cursor-ns-resize touch-none rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring/70', className)}
      style={{ height }}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.currentTarget.focus();
        const rect = e.currentTarget.getBoundingClientRect();
        const localY = e.clientY - rect.top;
        // Clicking off the cap jumps to that position; grabbing the cap keeps its offset.
        let start = value;
        if (localY < capTop || localY > capTop + CAP) {
          const n = 1 - Math.min(1, Math.max(0, (localY - CAP / 2) / travel));
          start = clamp(min + n * (max - min));
          onChange(start);
        }
        drag.current = { y: e.clientY, start };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const dy = d.y - e.clientY;
        const perPx = (max - min) / travel / (e.shiftKey ? 4 : 1);
        onChange(clamp(d.start + dy * perPx));
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onDoubleClick={() => onChange(defaultValue)}
      onKeyDown={(e) => {
        const step = e.shiftKey ? 6 : 1;
        if (e.key === 'ArrowUp' || e.key === 'ArrowRight') { e.preventDefault(); onChange(clamp(value + step)); }
        if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') { e.preventDefault(); onChange(clamp(value - step)); }
      }}
    >
      {/* Slot */}
      <div className="absolute inset-y-[11px] left-1/2 w-[3px] -translate-x-1/2 rounded-full bg-surface-0 shadow-[inset_0_1px_1px_oklch(0_0_0/0.6)]" />
      {/* 0 dB tick */}
      {min < 0 && max >= 0 && (
        <div className="absolute left-0.5 right-0.5 h-px bg-muted-foreground/40" style={{ top: zeroTop }} />
      )}
      {/* Cap */}
      <div
        className="absolute left-0 h-[22px] w-7 rounded-[3px] border border-black/50 bg-[linear-gradient(to_bottom,oklch(0.46_0.006_65),oklch(0.34_0.006_65))] shadow-[0_2px_4px_oklch(0_0_0/0.5),inset_0_1px_0_oklch(1_0_0/0.15)] transition-[filter] group-hover/fader:brightness-110"
        style={{ top: capTop }}
      >
        <div className="absolute inset-x-1 top-1/2 h-[2px] -translate-y-1/2 rounded-full bg-foreground/85" />
      </div>
    </div>
  );
}
