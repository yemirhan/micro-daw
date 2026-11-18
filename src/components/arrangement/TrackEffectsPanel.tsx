import { useState } from 'react';
import { ChevronDown, X } from 'lucide-react';
import { Knob } from '@/components/ui/Knob';
import { Button } from '@/components/ui/button';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import type { Track } from '@/types/arrangement';
import type { TrackEffectState, FilterType, LfoWaveform } from '@/types/effects';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { cn } from '@/lib/utils';

interface TrackEffectsPanelProps {
  track: Track;
  onClose: () => void;
}

const FILTER_TYPES: { value: FilterType; label: string }[] = [
  { value: 'lowpass', label: 'LP' },
  { value: 'highpass', label: 'HP' },
  { value: 'bandpass', label: 'BP' },
  { value: 'notch', label: 'Notch' },
  { value: 'allpass', label: 'AP' },
];

const LFO_WAVES: { value: LfoWaveform; label: string }[] = [
  { value: 'sine', label: 'Sin' },
  { value: 'triangle', label: 'Tri' },
  { value: 'square', label: 'Sqr' },
  { value: 'sawtooth', label: 'Saw' },
];

type SectionName = 'filter' | 'eq' | 'compressor' | 'chorus' | 'delay' | 'distortion' | 'reverb';

const SECTION_ORDER: SectionName[] = ['filter', 'eq', 'compressor', 'chorus', 'delay', 'distortion', 'reverb'];
const SECTION_LABEL: Record<SectionName, string> = {
  filter: 'Filter',
  eq: 'EQ',
  compressor: 'Compressor',
  chorus: 'Chorus',
  delay: 'Delay',
  distortion: 'Distortion',
  reverb: 'Reverb',
};

const fmtHz = (v: number) => v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}`;
const fmtPct = (v: number) => `${Math.round(v * 100)}%`;
const fmtDb = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}dB`;
const fmtMs = (v: number) => `${Math.round(v * 1000)}ms`;
const fmtQ = (v: number) => v.toFixed(1);
const fmtRatio = (v: number) => `${v.toFixed(1)}:1`;
const fmtRate = (v: number) => `${v.toFixed(1)}Hz`;

export function TrackEffectsPanel({ track, onClose }: TrackEffectsPanelProps) {
  const fx = track.effects ?? DEFAULT_TRACK_EFFECTS;
  // Open the effects that are already switched on.
  const [expandedSections, setExpandedSections] = useState<Set<SectionName>>(
    () => new Set(SECTION_ORDER.filter((n) => (fx[n] as { enabled: boolean }).enabled)),
  );

  const toggleSection = (name: SectionName) => {
    setExpandedSections((prev) => {
      const next = new Set(prev);
      if (next.has(name)) next.delete(name);
      else next.add(name);
      return next;
    });
  };

  const setFx = (effectName: keyof TrackEffectState, params: Record<string, unknown>) => {
    arrangementEngine.setTrackEffect(track.id, effectName, params);
  };

  const toggleEnabled = (effectName: keyof TrackEffectState) => {
    const current = fx[effectName] as { enabled: boolean };
    setFx(effectName, { enabled: !current.enabled });
  };

  const accentColor = track.color;
  // A knob drag is one undo step ("Change Track Effect"), not one per move.
  const dragGroup = {
    onChangeStart: () => arrangementEngine.beginUndoGroup('Change Track Effect'),
    onChangeEnd: () => arrangementEngine.endUndoGroup(),
  };

  const section = (name: SectionName) => ({
    label: SECTION_LABEL[name],
    enabled: (fx[name] as { enabled: boolean }).enabled,
    expanded: expandedSections.has(name),
    onToggle: () => toggleEnabled(name),
    onExpand: () => toggleSection(name),
  });

  return (
    <div className="flex h-full w-[336px] flex-col border-l border-border bg-surface-1 shadow-[-12px_0_32px_-12px_oklch(0_0_0/0.5)] animate-in slide-in-from-right-4 fade-in-0 duration-150">
      <div className="flex h-10 shrink-0 items-center gap-2 border-b border-border px-3">
        <span className="h-4 w-1 rounded-full" style={{ backgroundColor: track.color }} />
        <div className="flex min-w-0 flex-col">
          <span className="daw-label leading-none">Track FX</span>
          <span className="truncate text-[13px] font-semibold leading-tight">{track.name}</span>
        </div>
        <Button variant="ghost" size="icon-sm" className="ml-auto" onClick={onClose} aria-label="Close track effects">
          <X />
        </Button>
      </div>

      <div className="flex-1 space-y-1.5 overflow-y-auto p-2">
      {/* Expanded sections */}
      <EffectSection {...section('filter')}>
          <Select value={fx.filter.type ?? 'lowpass'} onValueChange={(v) => setFx('filter', { type: v })}>
            <SelectTrigger size="sm" className="h-6 w-[64px] px-1.5 text-[11px] font-semibold" aria-label="Filter type">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FILTER_TYPES.map((t) => (
                <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Knob {...dragGroup} label="Cutoff" value={fx.filter.cutoff} min={60} max={18000} step={1} defaultValue={18000}
            formatValue={fmtHz} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { cutoff: v })} />
          <Knob {...dragGroup} label="Reso" value={fx.filter.resonance} min={0} max={20} step={0.1} defaultValue={1}
            formatValue={fmtQ} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { resonance: v })} />
          <div className="basis-full" />
          <Knob {...dragGroup} label="LFO Rate" value={fx.filter.lfoRate ?? 1} min={0.1} max={20} step={0.1} defaultValue={1}
            formatValue={fmtRate} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { lfoRate: v })} />
          <Knob {...dragGroup} label="LFO Dep" value={fx.filter.lfoDepth ?? 0} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { lfoDepth: v })} />
          <Select value={fx.filter.lfoWave ?? 'sine'} onValueChange={(v) => setFx('filter', { lfoWave: v })}>
            <SelectTrigger size="sm" className="h-6 w-[60px] px-1.5 text-[11px] font-semibold" aria-label="LFO waveform">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LFO_WAVES.map((w) => (
                <SelectItem key={w.value} value={w.value}>{w.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
      </EffectSection>

      <EffectSection {...section('eq')}>
          <Knob {...dragGroup} label="Low" value={fx.eq.low} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { low: v })} />
          <Knob {...dragGroup} label="Mid" value={fx.eq.mid} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { mid: v })} />
          <Knob {...dragGroup} label="High" value={fx.eq.high} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { high: v })} />
      </EffectSection>

      <EffectSection {...section('compressor')}>
          <Knob {...dragGroup} label="Thresh" value={fx.compressor.threshold} min={-60} max={0} step={1} defaultValue={-24}
            formatValue={(v) => `${v}dB`} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { threshold: v })} />
          <Knob {...dragGroup} label="Ratio" value={fx.compressor.ratio} min={1} max={20} step={0.5} defaultValue={4}
            formatValue={fmtRatio} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { ratio: v })} />
          <Knob {...dragGroup} label="Attack" value={fx.compressor.attack} min={0.001} max={1} step={0.001} defaultValue={0.003}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { attack: v })} />
          <Knob {...dragGroup} label="Release" value={fx.compressor.release} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { release: v })} />
      </EffectSection>

      <EffectSection {...section('chorus')}>
          <Knob {...dragGroup} label="Depth" value={fx.chorus.depth} min={0} max={1} step={0.01} defaultValue={0.5}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('chorus', { depth: v })} />
      </EffectSection>

      <EffectSection {...section('delay')}>
          <Knob {...dragGroup} label="Time" value={fx.delay.time} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { time: v })} />
          <Knob {...dragGroup} label="Fdbk" value={fx.delay.feedback} min={0} max={0.95} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { feedback: v })} />
          <Knob {...dragGroup} label="Mix" value={fx.delay.wet} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { wet: v })} />
      </EffectSection>

      <EffectSection {...section('distortion')}>
          <Knob {...dragGroup} label="Drive" value={fx.distortion.amount} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('distortion', { amount: v })} />
          <Knob {...dragGroup} label="Mix" value={fx.distortion.wet} min={0} max={1} step={0.01} defaultValue={0.5}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('distortion', { wet: v })} />
      </EffectSection>

      <EffectSection {...section('reverb')}>
          <Knob {...dragGroup} label="Wet" value={fx.reverb.wet} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('reverb', { wet: v })} />
      </EffectSection>
      </div>
    </div>
  );
}

function EffectSection({
  label,
  enabled,
  expanded,
  onToggle,
  onExpand,
  children,
}: {
  label: string;
  enabled: boolean;
  expanded: boolean;
  onToggle: () => void;
  onExpand: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className={cn('rounded-lg border bg-surface-2', enabled ? 'border-input' : 'border-border')}>
      <div className="flex h-8 items-center gap-2 pr-1 pl-2">
        <Switch checked={enabled} onCheckedChange={onToggle} size="sm" aria-label={`${label} on/off`} />
        <button
          type="button"
          className={cn(
            'flex h-7 flex-1 items-center justify-between rounded-md px-1.5 text-left text-[11px] font-bold uppercase tracking-[0.08em] transition-colors hover:text-foreground',
            enabled ? 'text-foreground' : 'text-muted-foreground',
          )}
          onClick={onExpand}
          aria-expanded={expanded}
        >
          {label}
          <ChevronDown className={cn('size-3.5 transition-transform duration-150', !expanded && '-rotate-90')} />
        </button>
      </div>
      {expanded && (
        <div className={cn('flex flex-wrap items-center gap-x-3 gap-y-2 border-t border-hairline px-3 py-2.5 transition-opacity', !enabled && 'opacity-45')}>
          {children}
        </div>
      )}
    </div>
  );
}
