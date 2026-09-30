import { useState } from 'react';
import { ChevronDown, ChevronRight, X } from 'lucide-react';
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

const fmtHz = (v: number) => v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}`;
const fmtPct = (v: number) => `${Math.round(v * 100)}%`;
const fmtDb = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}dB`;
const fmtMs = (v: number) => `${Math.round(v * 1000)}ms`;
const fmtQ = (v: number) => v.toFixed(1);
const fmtRatio = (v: number) => `${v.toFixed(1)}:1`;
const fmtRate = (v: number) => `${v.toFixed(1)}Hz`;

export function TrackEffectsPanel({ track, onClose }: TrackEffectsPanelProps) {
  const [expandedSections, setExpandedSections] = useState<Set<SectionName>>(new Set());
  const fx = track.effects ?? DEFAULT_TRACK_EFFECTS;

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

  return (
    <div
      className="border-b border-border bg-card/80 backdrop-blur-sm px-3 py-2 animate-in slide-in-from-top-1 duration-200"
      style={{ borderLeft: `3px solid ${track.color}` }}
    >
      <div className="flex items-center gap-2 mb-1.5">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
          FX: {track.name}
        </span>
        <Button variant="ghost" size="sm" className="ml-auto h-5 w-5 p-0" onClick={onClose}>
          <X className="h-3 w-3" />
        </Button>
      </div>

      {/* Section headers row */}
      <div className="flex flex-wrap gap-1 mb-1">
        {(['filter', 'eq', 'compressor', 'chorus', 'delay', 'distortion', 'reverb'] as SectionName[]).map((name) => {
          const isExpanded = expandedSections.has(name);
          const isEnabled = (fx[name] as { enabled: boolean }).enabled;
          return (
            <button
              key={name}
              className={cn(
                'flex items-center gap-0.5 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider transition-colors',
                isExpanded
                  ? 'bg-primary/20 text-primary'
                  : isEnabled
                    ? 'text-primary/70 hover:bg-primary/10'
                    : 'text-muted-foreground hover:bg-muted',
              )}
              onClick={() => toggleSection(name)}
            >
              {isExpanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
              {name === 'compressor' ? 'Comp' : name === 'distortion' ? 'Dist' : name}
            </button>
          );
        })}
      </div>

      {/* Expanded sections */}
      {expandedSections.has('filter') && (
        <EffectSection label="Filter" enabled={fx.filter.enabled} onToggle={() => toggleEnabled('filter')}>
          <Select value={fx.filter.type ?? 'lowpass'} onValueChange={(v) => setFx('filter', { type: v })}>
            <SelectTrigger className="h-6 w-16 text-[10px] border-border/50">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {FILTER_TYPES.map((t) => (
                <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Knob label="Cutoff" value={fx.filter.cutoff} min={60} max={18000} step={1} defaultValue={18000}
            formatValue={fmtHz} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { cutoff: v })} />
          <Knob label="Reso" value={fx.filter.resonance} min={0} max={20} step={0.1} defaultValue={1}
            formatValue={fmtQ} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { resonance: v })} />
          <div className="mx-1 h-6 w-px bg-border/50" />
          <Knob label="LFO Rate" value={fx.filter.lfoRate ?? 1} min={0.1} max={20} step={0.1} defaultValue={1}
            formatValue={fmtRate} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { lfoRate: v })} />
          <Knob label="LFO Dep" value={fx.filter.lfoDepth ?? 0} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('filter', { lfoDepth: v })} />
          <Select value={fx.filter.lfoWave ?? 'sine'} onValueChange={(v) => setFx('filter', { lfoWave: v })}>
            <SelectTrigger className="h-6 w-14 text-[10px] border-border/50">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LFO_WAVES.map((w) => (
                <SelectItem key={w.value} value={w.value}>{w.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </EffectSection>
      )}

      {expandedSections.has('eq') && (
        <EffectSection label="EQ" enabled={fx.eq.enabled} onToggle={() => toggleEnabled('eq')}>
          <Knob label="Low" value={fx.eq.low} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { low: v })} />
          <Knob label="Mid" value={fx.eq.mid} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { mid: v })} />
          <Knob label="High" value={fx.eq.high} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={accentColor} size="sm"
            onChange={(v) => setFx('eq', { high: v })} />
        </EffectSection>
      )}

      {expandedSections.has('compressor') && (
        <EffectSection label="Compressor" enabled={fx.compressor.enabled} onToggle={() => toggleEnabled('compressor')}>
          <Knob label="Thresh" value={fx.compressor.threshold} min={-60} max={0} step={1} defaultValue={-24}
            formatValue={(v) => `${v}dB`} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { threshold: v })} />
          <Knob label="Ratio" value={fx.compressor.ratio} min={1} max={20} step={0.5} defaultValue={4}
            formatValue={fmtRatio} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { ratio: v })} />
          <Knob label="Attack" value={fx.compressor.attack} min={0.001} max={1} step={0.001} defaultValue={0.003}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { attack: v })} />
          <Knob label="Release" value={fx.compressor.release} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('compressor', { release: v })} />
        </EffectSection>
      )}

      {expandedSections.has('chorus') && (
        <EffectSection label="Chorus" enabled={fx.chorus.enabled} onToggle={() => toggleEnabled('chorus')}>
          <Knob label="Depth" value={fx.chorus.depth} min={0} max={1} step={0.01} defaultValue={0.5}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('chorus', { depth: v })} />
        </EffectSection>
      )}

      {expandedSections.has('delay') && (
        <EffectSection label="Delay" enabled={fx.delay.enabled} onToggle={() => toggleEnabled('delay')}>
          <Knob label="Time" value={fx.delay.time} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { time: v })} />
          <Knob label="Fdbk" value={fx.delay.feedback} min={0} max={0.95} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { feedback: v })} />
          <Knob label="Mix" value={fx.delay.wet} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('delay', { wet: v })} />
        </EffectSection>
      )}

      {expandedSections.has('distortion') && (
        <EffectSection label="Distortion" enabled={fx.distortion.enabled} onToggle={() => toggleEnabled('distortion')}>
          <Knob label="Drive" value={fx.distortion.amount} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('distortion', { amount: v })} />
          <Knob label="Mix" value={fx.distortion.wet} min={0} max={1} step={0.01} defaultValue={0.5}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('distortion', { wet: v })} />
        </EffectSection>
      )}

      {expandedSections.has('reverb') && (
        <EffectSection label="Reverb" enabled={fx.reverb.enabled} onToggle={() => toggleEnabled('reverb')}>
          <Knob label="Wet" value={fx.reverb.wet} min={0} max={1} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={accentColor} size="sm"
            onChange={(v) => setFx('reverb', { wet: v })} />
        </EffectSection>
      )}
    </div>
  );
}

function EffectSection({
  label,
  enabled,
  onToggle,
  children,
}: {
  label: string;
  enabled: boolean;
  onToggle: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 border-t border-border/30 py-1.5">
      <div className="flex items-center gap-1.5 w-20 shrink-0">
        <Switch checked={enabled} onCheckedChange={onToggle} className="h-3 w-6 [&>span]:h-2.5 [&>span]:w-2.5" />
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</span>
      </div>
      <div className={cn('flex items-center gap-3 transition-opacity', !enabled && 'opacity-40')}>
        {children}
      </div>
    </div>
  );
}
