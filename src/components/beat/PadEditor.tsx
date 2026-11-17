import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Play, Scissors } from 'lucide-react';
import { Knob } from '@/components/ui/Knob';
import { Button } from '@/components/ui/button';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import type { PadIndex, PadSample, PadSettings } from '@/types/sampler';
import { formatHz, formatSeconds, fromLog01, padColor, padLabel, padName, toLog01 } from './beatHelpers';
import { chopRanges, type ChopMode } from './chop';
import { usePadBuffer } from './usePadBuffer';
import { drawPeaks, getPeaks, prepareCanvas } from './waveform';

const DECAY_MIN = 0.005;
const DECAY_MAX = 4;
const CUTOFF_MIN = 20;
const CUTOFF_MAX = 20000;
const MIN_SLICE = 0.005;
const CHOKE_GROUPS = [0, 1, 2, 3, 4, 5, 6, 7, 8];

export interface PadEditorProps {
  pad: PadIndex;
  settings: PadSettings;
  /** Continuous edit (knob/handle drag); the owner groups these into one undo step. */
  onChange: (patch: Partial<PadSettings>) => void;
  /** End of a continuous edit gesture. */
  onChangeEnd?: () => void;
  /** Audition the pad. */
  onPreview?: (pad: PadIndex) => void;
  /** Assign slices of `sample` to consecutive pads from `fromPad`. */
  onChop?: (fromPad: PadIndex, sample: PadSample, ranges: { startSec: number; endSec: number }[]) => void;
  /** Bump to open the chop panel (e.g. from the pad's context menu). */
  chopRequest?: number;
  className?: string;
}

const fmtDb = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}dB`;
const fmtPan = (v: number) => (Math.abs(v) < 0.005 ? 'C' : `${v < 0 ? 'L' : 'R'}${Math.round(Math.abs(v) * 100)}`);
const fmtPitch = (v: number) => `${v > 0 ? '+' : ''}${Math.round(v)}st`;

function Segmented<T extends string | number>({
  value,
  options,
  onChange,
  label,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div role="radiogroup" aria-label={label} className="flex h-7 items-center rounded-md border border-border bg-surface-0 p-0.5">
      {options.map((o) => (
        <button
          key={String(o.value)}
          type="button"
          role="radio"
          aria-checked={value === o.value}
          onClick={() => onChange(o.value)}
          className={cn(
            'h-full rounded-[4px] px-2 text-[11px] font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
            value === o.value ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/** Editor for the selected pad: waveform with trim handles, knobs, play mode, chop. */
export function PadEditor({ pad, settings, onChange, onChangeEnd, onPreview, onChop, chopRequest, className }: PadEditorProps) {
  const color = padColor(settings, pad);
  const name = padName(settings);
  const { buffer, status } = usePadBuffer(settings.sample?.source);
  const duration = buffer?.duration ?? 0;

  // --- Chop panel ------------------------------------------------------------
  const [chopOpen, setChopOpen] = useState(false);
  const [chopMode, setChopMode] = useState<ChopMode>({ kind: 'transients', sensitivity: 0.5 });
  const lastChopRequest = useRef(chopRequest);
  useEffect(() => {
    if (chopRequest !== undefined && chopRequest !== lastChopRequest.current && settings.sample) setChopOpen(true);
    lastChopRequest.current = chopRequest;
  }, [chopRequest, settings.sample]);

  const slices = useMemo(
    () => (chopOpen && buffer ? chopRanges(buffer, settings, chopMode, pad) : []),
    [chopOpen, buffer, settings, chopMode, pad],
  );

  const applyChop = () => {
    if (!settings.sample || slices.length === 0) return;
    onChop?.(pad, settings.sample, slices);
    setChopOpen(false);
  };

  return (
    <section aria-label={`Pad ${padLabel(pad)} editor`} className={cn('flex min-h-0 min-w-0 flex-col gap-2', className)}>
      {/* Header */}
      <header className="flex min-w-0 items-center gap-2">
        <span className="h-5 w-1 shrink-0 rounded-full" style={{ background: color }} aria-hidden />
        <span className="font-mono text-[11px] font-semibold text-muted-foreground">{padLabel(pad)}</span>
        <span className="min-w-0 truncate text-[13px] font-semibold text-foreground" title={settings.sample?.source.kind === 'file' ? settings.sample.source.path : undefined}>
          {name || 'Empty pad'}
        </span>
        {status === 'missing' && <span className="shrink-0 text-[11px] text-signal-warn">File not found</span>}
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {buffer && (
            <span className="mr-1.5 font-mono text-[11px] text-muted-foreground tabular-nums" title="Trim start – end / sample length">
              {formatSeconds(Math.min(settings.startSec, duration))} – {formatSeconds(Math.min(settings.endSec ?? duration, duration))}
              <span className="text-muted-foreground"> / {formatSeconds(duration)}</span>
            </span>
          )}
          {onPreview && (
            <Tip label="Audition pad">
              <Button variant="ghost" size="icon-sm" disabled={!settings.sample} onClick={() => onPreview(pad)} aria-label="Audition pad">
                <Play />
              </Button>
            </Tip>
          )}
          {onChop && (
            <Popover open={chopOpen} onOpenChange={setChopOpen}>
              <PopoverTrigger asChild>
                <Button variant="outline" size="sm" disabled={!settings.sample || !buffer} aria-label="Chop to pads">
                  <Scissors />
                  Chop
                </Button>
              </PopoverTrigger>
              <PopoverContent align="end" className="w-64">
                <ChopPanel mode={chopMode} onModeChange={setChopMode} sliceCount={slices.length} fromPad={pad} onApply={applyChop} />
              </PopoverContent>
            </Popover>
          )}
        </div>
      </header>

      {/* Waveform */}
      <TrimWaveform
        buffer={buffer}
        status={status}
        color={color}
        settings={settings}
        duration={duration}
        slices={slices}
        onChange={onChange}
        onChangeEnd={onChangeEnd}
      />

      {/* Controls */}
      <div className="flex flex-wrap items-end gap-x-3 gap-y-2">
        <Knob label="Gain" value={settings.gainDb} min={-60} max={12} step={0.5} defaultValue={0} formatValue={fmtDb} color={color}
          onChange={(v) => onChange({ gainDb: v })} onChangeEnd={onChangeEnd} />
        <Knob label="Pan" value={settings.pan} min={-1} max={1} step={0.01} defaultValue={0} formatValue={fmtPan} color={color}
          onChange={(v) => onChange({ pan: v })} onChangeEnd={onChangeEnd} />
        <Knob label="Pitch" value={settings.pitch} min={-24} max={24} step={1} defaultValue={0} formatValue={fmtPitch} color={color}
          onChange={(v) => onChange({ pitch: v })} onChangeEnd={onChangeEnd} />
        <Knob label="Decay" value={toLog01(settings.decay, DECAY_MIN, DECAY_MAX)} min={0} max={1} step={0.001}
          defaultValue={toLog01(0.05, DECAY_MIN, DECAY_MAX)} formatValue={(t) => formatSeconds(fromLog01(t, DECAY_MIN, DECAY_MAX))} color={color}
          onChange={(t) => onChange({ decay: fromLog01(t, DECAY_MIN, DECAY_MAX) })} onChangeEnd={onChangeEnd} />
        <Knob label="Filter" value={toLog01(settings.filterCutoff, CUTOFF_MIN, CUTOFF_MAX)} min={0} max={1} step={0.001} defaultValue={1}
          formatValue={(t) => (t >= 0.999 ? 'Open' : formatHz(fromLog01(t, CUTOFF_MIN, CUTOFF_MAX)))} color={color}
          onChange={(t) => onChange({ filterCutoff: t >= 0.999 ? CUTOFF_MAX : Math.round(fromLog01(t, CUTOFF_MIN, CUTOFF_MAX)) })} onChangeEnd={onChangeEnd} />

        <div className="ml-auto flex flex-wrap items-center gap-2">
          <Segmented
            label="Play mode"
            value={settings.mode}
            options={[{ value: 'oneshot', label: 'One-shot' }, { value: 'gate', label: 'Gate' }]}
            onChange={(mode) => { onChange({ mode }); onChangeEnd?.(); }}
          />
          <button
            type="button"
            aria-pressed={settings.reverse}
            onClick={() => { onChange({ reverse: !settings.reverse }); onChangeEnd?.(); }}
            className={cn(
              'h-7 rounded-md border px-2 text-[11px] font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
              settings.reverse ? 'border-transparent bg-foreground text-background' : 'border-border bg-surface-0 text-muted-foreground hover:text-foreground',
            )}
          >
            Reverse
          </button>
          <label className="flex h-7 items-center gap-1.5 rounded-md border border-border bg-surface-0 pr-1 pl-2 text-[11px] text-muted-foreground">
            Choke
            <select
              aria-label="Choke group"
              value={settings.chokeGroup}
              onChange={(e) => { onChange({ chokeGroup: Number(e.target.value) }); onChangeEnd?.(); }}
              className="h-5 rounded-sm bg-surface-2 px-1 font-mono text-[11px] text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring/70"
            >
              {CHOKE_GROUPS.map((g) => (
                <option key={g} value={g}>{g === 0 ? 'Off' : g}</option>
              ))}
            </select>
          </label>
        </div>
      </div>
    </section>
  );
}

function ChopPanel({
  mode,
  onModeChange,
  sliceCount,
  fromPad,
  onApply,
}: {
  mode: ChopMode;
  onModeChange: (m: ChopMode) => void;
  sliceCount: number;
  fromPad: PadIndex;
  onApply: () => void;
}) {
  const last = Math.min(15, fromPad + Math.max(0, sliceCount - 1));
  return (
    <div className="flex flex-col gap-3">
      <div className="text-[12px] font-semibold">Chop to pads</div>
      <Segmented<string>
        label="Chop mode"
        value={mode.kind === 'equal' ? String(mode.count) : 'transients'}
        options={[
          { value: 'transients', label: 'Hits' },
          { value: '4', label: '4' },
          { value: '8', label: '8' },
          { value: '16', label: '16' },
        ]}
        onChange={(v) =>
          onModeChange(v === 'transients' ? { kind: 'transients', sensitivity: 0.5 } : { kind: 'equal', count: Number(v) as 4 | 8 | 16 })
        }
      />
      {mode.kind === 'transients' && (
        <label className="flex items-center gap-2 text-[11px] text-muted-foreground">
          Sensitivity
          <input
            type="range"
            min={0}
            max={1}
            step={0.01}
            value={mode.sensitivity}
            onChange={(e) => onModeChange({ kind: 'transients', sensitivity: Number(e.target.value) })}
            className="flex-1 accent-[var(--primary)]"
          />
        </label>
      )}
      <p className="text-[11px] leading-snug text-muted-foreground">
        {sliceCount > 0
          ? `${sliceCount} slice${sliceCount === 1 ? '' : 's'} → pads ${padLabel(fromPad)}–${padLabel(last)}. Each pad keeps the same file with its own start and end.`
          : 'No slices found in the trimmed part.'}
      </p>
      <Button size="sm" disabled={sliceCount === 0} onClick={onApply}>
        Chop to {sliceCount} pad{sliceCount === 1 ? '' : 's'}
      </Button>
    </div>
  );
}

interface TrimWaveformProps {
  buffer: AudioBuffer | null;
  status: ReturnType<typeof usePadBuffer>['status'];
  color: string;
  settings: PadSettings;
  duration: number;
  slices: { startSec: number; endSec: number }[];
  onChange: (patch: Partial<PadSettings>) => void;
  onChangeEnd?: () => void;
}

/** Canvas waveform + draggable start/end handles + slice markers. */
function TrimWaveform({ buffer, status, color, settings, duration, slices, onChange, onChangeEnd }: TrimWaveformProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const drag = useRef<'start' | 'end' | null>(null);

  const start = duration > 0 ? Math.min(settings.startSec, duration) : 0;
  const end = duration > 0 ? Math.min(settings.endSec ?? duration, duration) : 0;
  const pct = (sec: number) => (duration > 0 ? `${(sec / duration) * 100}%` : '0%');

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const draw = () => {
      const w = canvas.clientWidth;
      const h = canvas.clientHeight;
      if (!w || !h) return;
      const ctx = prepareCanvas(canvas, w, h);
      if (!ctx) return;
      // Centre line
      ctx.fillStyle = 'oklch(1 0 0 / 0.08)';
      ctx.fillRect(0, Math.round(h / 2), w, 1);
      if (!buffer) return;
      const peaks = getPeaks(buffer, Math.max(32, Math.round(w)));
      drawPeaks(ctx, peaks, { x: 0, y: 4, w, h: h - 8 }, 'oklch(1 0 0 / 0.22)');
      const a = (start / buffer.duration) * w;
      const b = (end / buffer.duration) * w;
      ctx.save();
      ctx.beginPath();
      ctx.rect(a, 0, Math.max(1, b - a), h);
      ctx.clip();
      drawPeaks(ctx, peaks, { x: 0, y: 4, w, h: h - 8 }, color);
      ctx.restore();
    };
    draw();
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(draw);
    ro.observe(canvas);
    return () => ro.disconnect();
  }, [buffer, color, start, end]);

  const secAt = useCallback(
    (clientX: number) => {
      const r = wrapRef.current?.getBoundingClientRect();
      if (!r || !(r.width > 0) || !(duration > 0)) return 0;
      return Math.min(duration, Math.max(0, ((clientX - r.left) / r.width) * duration));
    },
    [duration],
  );

  const onHandleDown = (which: 'start' | 'end') => (e: React.PointerEvent) => {
    if (e.button !== 0 || !buffer) return;
    e.preventDefault();
    e.stopPropagation();
    drag.current = which;
    e.currentTarget.setPointerCapture?.(e.pointerId);
  };
  const onHandleMove = (e: React.PointerEvent) => {
    if (!drag.current) return;
    const sec = secAt(e.clientX);
    if (drag.current === 'start') {
      onChange({ startSec: Math.min(sec, end - MIN_SLICE) });
    } else {
      const next = Math.max(sec, start + MIN_SLICE);
      onChange({ endSec: next >= duration - 1e-4 ? null : next });
    }
  };
  const onHandleUp = () => {
    if (!drag.current) return;
    drag.current = null;
    onChangeEnd?.();
  };
  const nudge = (which: 'start' | 'end') => (e: React.KeyboardEvent) => {
    const step = (e.shiftKey ? 0.01 : 0.001) * Math.max(1, duration);
    const dir = e.key === 'ArrowRight' || e.key === 'ArrowUp' ? 1 : e.key === 'ArrowLeft' || e.key === 'ArrowDown' ? -1 : 0;
    if (!dir) return;
    e.preventDefault();
    if (which === 'start') onChange({ startSec: Math.max(0, Math.min(end - MIN_SLICE, start + dir * step)) });
    else {
      const next = Math.min(duration, Math.max(start + MIN_SLICE, end + dir * step));
      onChange({ endSec: next >= duration - 1e-4 ? null : next });
    }
    onChangeEnd?.();
  };

  return (
    <div className="daw-lcd relative min-h-[96px] flex-1 overflow-hidden">
      <div
        ref={wrapRef}
        className="absolute inset-y-0 right-2 left-2"
        title={buffer ? 'Drag the handles to trim. Double-click to reset.' : undefined}
        onDoubleClick={() => {
          if (!buffer) return;
          onChange({ startSec: 0, endSec: null });
          onChangeEnd?.();
        }}
      >
        <canvas ref={canvasRef} aria-hidden className="absolute inset-0 h-full w-full" />

        {!buffer && (
          <div className="absolute inset-0 flex items-center justify-center">
            <span className={cn('rounded-sm bg-surface-0 px-2 py-0.5 text-[11px]', status === 'missing' ? 'text-signal-warn' : 'text-muted-foreground')}>
              {status === 'loading' ? 'Loading…' : status === 'missing' ? 'Sample file not found. Drop a new one on the pad.' : 'Drop a sample on a pad, or press Enter in the browser.'}
            </span>
          </div>
        )}

        {buffer && (
          <>
            {/* Dimmed outside the trim range */}
            <div className="pointer-events-none absolute inset-y-0 left-0 bg-surface-0/55" style={{ width: pct(start) }} />
            <div className="pointer-events-none absolute inset-y-0 right-0 bg-surface-0/55" style={{ left: pct(end) }} />

            {/* Slice markers (chop preview) */}
            {slices.map((s, i) => (
              <div key={i} className="pointer-events-none absolute inset-y-0 w-px bg-signal-warn/80" style={{ left: pct(s.startSec) }}>
                <span className="absolute top-0.5 left-0.5 font-mono text-[11px] leading-none text-signal-warn">{i + 1}</span>
              </div>
            ))}

            {(['start', 'end'] as const).map((which) => {
              const sec = which === 'start' ? start : end;
              return (
                <div
                  key={which}
                  role="slider"
                  tabIndex={0}
                  aria-label={which === 'start' ? 'Trim start' : 'Trim end'}
                  aria-valuemin={0}
                  aria-valuemax={Number(duration.toFixed(3))}
                  aria-valuenow={Number(sec.toFixed(3))}
                  aria-valuetext={formatSeconds(sec)}
                  className="group/handle absolute inset-y-0 z-10 -ml-[6px] w-[12px] cursor-ew-resize outline-none"
                  style={{ left: pct(sec) }}
                  onPointerDown={onHandleDown(which)}
                  onPointerMove={onHandleMove}
                  onPointerUp={onHandleUp}
                  onPointerCancel={onHandleUp}
                  onKeyDown={nudge(which)}
                >
                  <span className="absolute inset-y-0 left-[5px] w-[2px] bg-foreground/85 group-hover/handle:bg-foreground group-focus-visible/handle:bg-primary" />
                  <span
                    className={cn(
                      'absolute left-[1px] h-[10px] w-[10px] rounded-[2px] bg-foreground group-focus-visible/handle:bg-primary',
                      which === 'start' ? 'top-0' : 'bottom-0',
                    )}
                  />
                </div>
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}
