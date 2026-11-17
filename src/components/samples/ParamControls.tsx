import type { ReactNode } from 'react';
import { Slider } from '@/components/ui/slider';
import { Switch } from '@/components/ui/switch';
import { cn } from '@/lib/utils';

/** Labelled slider row with a mono value readout (DAW-style). */
export function ParamSlider({
  label,
  value,
  min,
  max,
  step,
  displayValue,
  disabled,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  displayValue: string;
  disabled?: boolean;
  suffix?: ReactNode;
  onChange: (v: number) => void;
}) {
  return (
    <div className={cn('flex h-8 items-center gap-3', disabled && 'opacity-45')}>
      <span className="w-20 shrink-0 text-[10.5px] font-bold uppercase tracking-[0.08em] text-muted-foreground">{label}</span>
      <Slider
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={([v]) => onChange(v)}
        disabled={disabled}
        className="flex-1"
        aria-label={label}
      />
      <span className="daw-lcd flex h-6 w-16 shrink-0 items-center justify-end px-2 text-[11px] text-foreground">
        {displayValue}
      </span>
      {suffix}
    </div>
  );
}

/** A processing stage: switch + title, parameters revealed when enabled. */
export function EffectSection({
  title,
  description,
  enabled,
  onEnabledChange,
  children,
}: {
  title: string;
  description: string;
  enabled: boolean;
  onEnabledChange: (v: boolean) => void;
  children?: ReactNode;
}) {
  const id = `fx-${title.toLowerCase().replace(/\s+/g, '-')}`;
  return (
    <section
      className={cn(
        'rounded-lg border transition-colors duration-150',
        enabled ? 'border-hairline-strong bg-surface-2' : 'border-border bg-surface-1',
      )}
    >
      <div className="flex items-center gap-3 px-3.5 py-2.5">
        <Switch id={id} checked={enabled} onCheckedChange={onEnabledChange} />
        <label htmlFor={id} className="flex min-w-0 flex-1 cursor-default items-baseline gap-2">
          <span className={cn('text-[13px] font-semibold', enabled ? 'text-foreground' : 'text-foreground/80')}>{title}</span>
          <span className="truncate text-xs text-muted-foreground">{description}</span>
        </label>
      </div>
      {enabled && children && <div className="flex flex-col gap-1 border-t border-border px-3.5 py-2.5">{children}</div>}
    </section>
  );
}
