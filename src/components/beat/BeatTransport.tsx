import { useEffect, useRef, useState } from 'react';
import { Circle, Download, Play, Square } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Knob } from '@/components/ui/Knob';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { Tip } from '@/components/ui/tip';
import { cn } from '@/lib/utils';
import { MAX_BPM, MIN_BPM } from '@/utils/constants';
import { NOTE_REPEAT_RATES } from '@/services/NoteRepeat';
import type { LoopRecordSettings, NoteRepeatRate, QuantizeGrid } from '@/types/sampler';

const QUANTIZE: QuantizeGrid[] = ['off', '1/4', '1/8', '1/8T', '1/16', '1/16T', '1/32'];
const BARS: LoopRecordSettings['bars'][] = [1, 2, 4, 8];
const COUNT_IN: LoopRecordSettings['countInBars'][] = [0, 1, 2];
const MOD = typeof navigator !== 'undefined' && /Mac/i.test(navigator.platform || navigator.userAgent) ? '⌘' : 'Ctrl+';

export interface BeatTransportProps {
  playing: boolean;
  recording: boolean;
  countInRemaining: number;
  bpm: number;
  loop: LoopRecordSettings;
  patternStartBeat: number;
  subscribePosition?: (cb: (beat: number) => void) => () => void;
  onTogglePlay: () => void;
  onToggleRecord: () => void;
  onBpmChange: (bpm: number) => void;
  onLoopChange: (patch: Partial<LoopRecordSettings>) => void;
  /** Opens the export dialog (omit to hide the button). */
  onExport?: () => void;
  /** Note repeat toggle + rate (omit to hide the control). */
  noteRepeat?: {
    enabled: boolean;
    rate: NoteRepeatRate;
    onToggle: () => void;
    onRateChange: (rate: NoteRepeatRate) => void;
  };
  className?: string;
}

function MetronomeIcon({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" className={className} aria-hidden="true">
      <path d="M9 3h6l4 18H5L9 3z" />
      <path d="M12 16l5-9" />
      <path d="M7 16h10" />
    </svg>
  );
}

function Seg<T extends string | number>({
  label,
  value,
  options,
  format = String,
  mono = true,
  onChange,
}: {
  label: string;
  mono?: boolean;
  value: T;
  options: readonly T[];
  format?: (v: T) => string;
  onChange: (v: T) => void;
}) {
  return (
    <div role="radiogroup" aria-label={label} title={label} className="flex h-7 items-center rounded-md border border-border bg-surface-0 p-0.5">
      {options.map((o) => (
        <button
          key={String(o)}
          type="button"
          role="radio"
          aria-checked={value === o}
          onClick={() => onChange(o)}
          className={cn(
            'h-full min-w-[22px] rounded-[4px] px-1.5 text-[11px] outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
            mono && 'font-mono',
            value === o ? 'bg-surface-3 font-semibold text-foreground' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          {format(o)}
        </button>
      ))}
    </div>
  );
}

/** Labelled control. The label hides when the strip gets narrow (the control keeps its aria-label). */
function Group({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex items-center gap-1.5">
      <span aria-hidden className="hidden text-[11px] whitespace-nowrap text-muted-foreground @[1180px]/transport:inline">{label}</span>
      {children}
    </div>
  );
}

/**
 * One section of the strip (Transport | Loop | Record | Repeat): a small caps
 * caption + its controls, separated from the next section by a hairline. The
 * strip stays one row down to the 1360 px window with the browser open
 * (~1040 px); narrower (wide browser panel) the captions hide, then it wraps.
 */
function Section({ label, caption = true, className, children }: { label: string; caption?: boolean; className?: string; children: React.ReactNode }) {
  return (
    <div role="group" aria-label={label} data-section={label} className={cn('flex shrink-0 items-center gap-1.5', className)}>
      {caption && (
        <span aria-hidden className="hidden text-[11px] font-bold tracking-[0.08em] whitespace-nowrap text-muted-foreground uppercase @[1000px]/transport:inline">
          {label}
        </span>
      )}
      {children}
    </div>
  );
}

function SectionRule() {
  return <span aria-hidden className="h-7 w-px shrink-0 bg-foreground/15" />;
}

/** Swing: a small knob with its value beside it, so the strip stays one row high. */
function SwingControl({ swing, onChange }: { swing: number; onChange: (swing: number) => void }) {
  const pct = `${Math.round(50 + swing * 50)}%`;
  return (
    <Tip label="Swing: delays every other grid step (50% = straight). Double-click resets.">
      <div className="flex h-8 items-center gap-1.5">
        <span aria-hidden className="hidden text-[11px] text-muted-foreground @[1180px]/transport:inline">Swing</span>
        <Knob
          size="sm"
          label="Swing"
          value={swing}
          min={0}
          max={0.75}
          step={0.01}
          defaultValue={0}
          formatValue={() => pct}
          onChange={onChange}
          className="[&>span]:hidden"
        />
        <span className="w-[28px] font-mono text-[11px] text-foreground tabular-nums">{pct}</span>
      </div>
    </Tip>
  );
}

/** Tempo: type, drag vertically, scroll or arrow keys. */
function BpmField({ bpm, onChange }: { bpm: number; onChange: (bpm: number) => void }) {
  const [draft, setDraft] = useState<string | null>(null);
  const drag = useRef<{ y: number; start: number; moved: boolean } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const clamp = (v: number) => Math.round(Math.min(MAX_BPM, Math.max(MIN_BPM, v)));
  const bpmRef = useRef(bpm);
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    bpmRef.current = bpm;
    onChangeRef.current = onChange;
  });

  useEffect(() => {
    const el = inputRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      onChangeRef.current(Math.round(Math.min(MAX_BPM, Math.max(MIN_BPM, bpmRef.current + (e.deltaY < 0 ? 1 : -1)))));
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, []);

  const commit = () => {
    if (draft !== null) {
      const v = Number.parseFloat(draft);
      if (Number.isFinite(v)) onChange(clamp(v));
    }
    setDraft(null);
  };

  return (
    <label className="daw-lcd flex h-8 cursor-ns-resize items-center gap-1.5 pr-2 pl-2">
      <input
        ref={inputRef}
        aria-label="Tempo (BPM)"
        inputMode="numeric"
        value={draft ?? String(bpm)}
        onChange={(e) => setDraft(e.target.value.replace(/[^0-9.]/g, '').slice(0, 5))}
        onFocus={(e) => e.currentTarget.select()}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            commit();
            e.currentTarget.blur();
          } else if (e.key === 'Escape') {
            setDraft(null);
            e.currentTarget.blur();
          } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            setDraft(null);
            onChange(clamp(bpm + (e.key === 'ArrowUp' ? 1 : -1) * (e.shiftKey ? 10 : 1)));
          }
        }}
        onPointerDown={(e) => {
          if (document.activeElement === e.currentTarget) return;
          e.preventDefault();
          drag.current = { y: e.clientY, start: bpm, moved: false };
          e.currentTarget.setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const d = drag.current;
          if (!d) return;
          const dy = d.y - e.clientY;
          if (Math.abs(dy) > 2) d.moved = true;
          if (d.moved) onChange(clamp(d.start + dy / (e.shiftKey ? 8 : 2)));
        }}
        onPointerUp={(e) => {
          const d = drag.current;
          drag.current = null;
          if (d && !d.moved) e.currentTarget.focus();
        }}
        onPointerCancel={() => {
          drag.current = null;
        }}
        className="w-[30px] cursor-ns-resize bg-transparent text-right text-[15px] font-semibold text-foreground outline-none focus:cursor-text focus:text-primary"
      />
      <span className="text-[11px] font-semibold text-muted-foreground">BPM</span>
    </label>
  );
}

/** Loop position "bar.beat" inside the pattern, updated imperatively. */
function LoopPosition({ bars, patternStartBeat, playing, subscribePosition }: { bars: number; patternStartBeat: number; playing: boolean; subscribePosition?: BeatTransportProps['subscribePosition'] }) {
  const ref = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.textContent = '1.1';
    if (!playing || !subscribePosition) return;
    const len = bars * 4;
    let last = '';
    return subscribePosition((beat) => {
      const rel = (((beat - patternStartBeat) % len) + len) % len;
      const text = `${Math.floor(rel / 4) + 1}.${Math.floor(rel % 4) + 1}`;
      if (text !== last) el.textContent = last = text;
    });
  }, [bars, patternStartBeat, playing, subscribePosition]);
  return (
    <span className="flex items-baseline gap-1">
      <span ref={ref} className="inline-block w-[30px] text-[15px] font-semibold text-foreground tabular-nums">1.1</span>
      <span className="text-[11px] text-muted-foreground">/ {bars}</span>
    </span>
  );
}

/** Note repeat: "Repeat" toggle + rate. Held pads retrigger on the grid while the transport runs. */
function NoteRepeatControl({ enabled, rate, onToggle, onRateChange }: NonNullable<BeatTransportProps['noteRepeat']>) {
  return (
    <div className={cn('flex h-7 items-center rounded-md border border-border bg-surface-0 p-0.5', enabled && 'border-primary/50')}>
      <Tip
        label={enabled ? `Note repeat on: held pads retrigger every ${rate} while playing` : 'Note repeat: hold a pad to retrigger it on the grid while playing'}
        shortcut="N"
      >
        <button
          type="button"
          aria-label="Note repeat"
          aria-pressed={enabled}
          onClick={onToggle}
          className={cn(
            'h-full rounded-[4px] px-1.5 text-[11px] font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
            enabled ? 'bg-primary/20 text-primary' : 'text-muted-foreground hover:text-foreground',
          )}
        >
          Repeat
        </button>
      </Tip>
      <Select value={rate} onValueChange={(v) => onRateChange(v as NoteRepeatRate)}>
        <SelectTrigger size="sm" className="h-full w-[54px] gap-0.5 border-0 bg-transparent px-1 font-mono text-[11px] shadow-none [&_svg]:size-3!" aria-label="Note repeat rate">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {NOTE_REPEAT_RATES.map((r) => (
            <SelectItem key={r} value={r} className="font-mono text-[11px]">
              {r}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
}

/**
 * Beat-view transport, one row in four sections: Transport (play/stop, loop
 * record, position, tempo) | Loop (length, quantize, swing) | Record (count-in,
 * overdub/replace, click) | Repeat (note repeat + rate), then Export. (The kit menu
 * lives in the pads header and "Clear" in the step sequencer header.)
 */
export function BeatTransport({
  playing,
  recording,
  countInRemaining,
  bpm,
  loop,
  patternStartBeat,
  subscribePosition,
  onTogglePlay,
  onToggleRecord,
  onBpmChange,
  onLoopChange,
  onExport,
  noteRepeat,
  className,
}: BeatTransportProps) {
  const counting = countInRemaining > 0;
  const armed = recording || counting;

  return (
    <div className={cn('@container/transport flex min-w-0 flex-wrap items-center gap-x-2.5 gap-y-2', className)}>
      {/* Transport: play / record, position LCD, tempo (self-explanatory: no caption) */}
      <Section label="Transport" caption={false} className="gap-1.5">
        <Tip label={playing || armed ? 'Stop' : 'Play'} shortcut="Space">
          <Button
            variant="outline"
            size="icon"
            onClick={onTogglePlay}
            aria-label={playing || armed ? 'Stop' : 'Play'}
            aria-pressed={playing}
            className={cn(playing && 'border-signal-play/50 bg-signal-play/15 text-signal-play hover:bg-signal-play/20 hover:text-signal-play')}
          >
            {playing || armed ? <Square className="size-3.5 fill-current" /> : <Play className="fill-current" />}
          </Button>
        </Tip>
        <Tip label={armed ? 'Stop recording' : 'Record loop'} shortcut="R">
          <Button
            variant="outline"
            size="icon"
            onClick={onToggleRecord}
            aria-label={armed ? 'Stop recording' : 'Record loop'}
            aria-pressed={armed}
            className={cn(
              armed
                ? 'animate-rec-pulse border-transparent bg-signal-rec text-white hover:bg-signal-rec hover:text-white'
                : 'text-signal-rec hover:text-signal-rec',
            )}
          >
            <Circle className="size-3.5 fill-current" />
          </Button>
        </Tip>

        {/* Status LCD: count-in number or loop position. Fixed width: no layout shift. */}
        <div
          className={cn('daw-lcd flex h-8 w-[92px] items-center justify-between gap-2 px-2', armed && 'border-signal-rec/60')}
          aria-live="polite"
        >
          {counting ? (
            <>
              <span className="text-[11px] font-semibold text-signal-rec">COUNT-IN</span>
              <span className="text-[18px] leading-none font-bold text-signal-rec tabular-nums">{Math.ceil(countInRemaining)}</span>
            </>
          ) : (
            <>
              <LoopPosition bars={loop.bars} patternStartBeat={patternStartBeat} playing={playing} subscribePosition={subscribePosition} />
              {recording && <span className="size-2 animate-rec-pulse rounded-full bg-signal-rec" aria-label="Recording" />}
            </>
          )}
        </div>
        <BpmField bpm={bpm} onChange={onBpmChange} />
      </Section>

      <SectionRule />

      {/* Loop: length, quantize, swing */}
      <Section label="Loop">
        <Group label="Bars">
          <Seg label="Loop length in bars" value={loop.bars} options={BARS} onChange={(bars) => onLoopChange({ bars })} />
        </Group>
        <Group label="Quant">
          <Select value={loop.quantize} onValueChange={(v) => onLoopChange({ quantize: v as QuantizeGrid })}>
            <SelectTrigger size="sm" className="w-[64px] gap-0.5 bg-surface-0 px-1.5 font-mono text-[11px] [&_svg]:size-3!" aria-label="Quantize" title="Quantize recorded hits">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {QUANTIZE.map((q) => (
                <SelectItem key={q} value={q} className="font-mono text-[11px]">
                  {q === 'off' ? 'Off' : q}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </Group>
        <SwingControl swing={loop.swing} onChange={(swing) => onLoopChange({ swing })} />
      </Section>

      <SectionRule />

      {/* Record: count-in, overdub/replace, click */}
      <Section label="Record">
        <Group label="Count-in">
          <Seg label="Count-in bars" value={loop.countInBars} options={COUNT_IN} format={(v) => (v === 0 ? 'Off' : String(v))} onChange={(countInBars) => onLoopChange({ countInBars })} />
        </Group>
        <Seg
          label="Record mode"
          value={loop.overdub ? 'overdub' : 'replace'}
          options={['overdub', 'replace'] as const}
          format={(v) => (v === 'overdub' ? 'Overdub' : 'Replace')}
          mono={false}
          onChange={(v) => onLoopChange({ overdub: v === 'overdub' })}
        />
        <Tip label={loop.metronome ? 'Metronome on while recording' : 'Metronome off'} shortcut="M">
          <button
            type="button"
            aria-label="Metronome"
            aria-pressed={loop.metronome}
            className={cn(
              'flex size-7 items-center justify-center rounded-md border outline-none focus-visible:ring-2 focus-visible:ring-ring/70',
              loop.metronome
                ? 'border-primary/50 bg-primary/15 text-primary'
                : 'border-border bg-surface-0 text-muted-foreground hover:text-foreground',
            )}
            onClick={() => onLoopChange({ metronome: !loop.metronome })}
          >
            <MetronomeIcon className="size-4" />
          </button>
        </Tip>
      </Section>

      {noteRepeat && (
        <>
          <SectionRule />
          {/* Repeat: the toggle says what it is, so no caption */}
          <Section label="Note repeat" caption={false}>
            <NoteRepeatControl {...noteRepeat} />
          </Section>
        </>
      )}

      {onExport && (
        <div className="ml-auto flex shrink-0 items-center">
          <Tip label="Export WAV or MIDI" shortcut={`${MOD}E`}>
            <Button variant="outline" size="icon-sm" aria-label="Export" onClick={onExport}>
              <Download />
            </Button>
          </Tip>
        </div>
      )}
    </div>
  );
}
