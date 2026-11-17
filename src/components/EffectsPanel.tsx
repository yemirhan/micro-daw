import { useState } from 'react';
import { Knob } from '@/components/ui/Knob';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { audioEngine } from '@/services/AudioEngine';
import { DEFAULT_EFFECT_PARAMS as D, type EffectParams, type FilterType, type LfoWaveform } from '@/types/effects';
import { cn } from '@/lib/utils';

interface EffectsPanelProps {
  effectParams: EffectParams;
  onEffectChange: (params: EffectParams) => void;
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

const fmtHz = (v: number) => v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}`;
const fmtPct = (v: number) => `${Math.round(v * 100)}%`;
const fmtDb = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}dB`;
const fmtMs = (v: number) => `${Math.round(v * 1000)}ms`;
const fmtQ = (v: number) => v.toFixed(1);
const fmtRatio = (v: number) => `${v.toFixed(1)}:1`;
const fmtRate = (v: number) => `${v.toFixed(1)}Hz`;

const ACCENT = 'var(--primary)';

type Section = 'lfo' | 'delay' | 'distortion' | 'eq' | 'compressor';

export function EffectsPanel({ effectParams, onEffectChange }: EffectsPanelProps) {
  const [expandedSection, setExpandedSection] = useState<Section | null>(null);

  const setEffect = (key: keyof EffectParams, value: number) => {
    switch (key) {
      case 'reverbWet': audioEngine.setReverbWet(value); break;
      case 'chorusDepth': audioEngine.setChorusDepth(value); break;
      case 'filterCutoff': audioEngine.setFilterCutoff(value); break;
      case 'filterResonance': audioEngine.setFilterResonance(value); break;
      case 'filterLfoRate': audioEngine.setFilterLfoRate(value); break;
      case 'filterLfoDepth': audioEngine.setFilterLfoDepth(value); break;
      case 'delayTime': audioEngine.setDelayTime(value); break;
      case 'delayFeedback': audioEngine.setDelayFeedback(value); break;
      case 'delayWet': audioEngine.setDelayWet(value); break;
      case 'distortionAmount': audioEngine.setDistortionAmount(value); break;
      case 'distortionWet': audioEngine.setDistortionWet(value); break;
      case 'eqLow': audioEngine.setEqLow(value); break;
      case 'eqMid': audioEngine.setEqMid(value); break;
      case 'eqHigh': audioEngine.setEqHigh(value); break;
      case 'compThreshold': audioEngine.setCompThreshold(value); break;
      case 'compRatio': audioEngine.setCompRatio(value); break;
      case 'compAttack': audioEngine.setCompAttack(value); break;
      case 'compRelease': audioEngine.setCompRelease(value); break;
    }
    onEffectChange(audioEngine.getEffectParams());
  };

  const setFilterType = (type: FilterType) => {
    audioEngine.setFilterType(type);
    onEffectChange(audioEngine.getEffectParams());
  };

  const setLfoWave = (wave: LfoWaveform) => {
    audioEngine.setFilterLfoWave(wave);
    onEffectChange(audioEngine.getEffectParams());
  };

  const toggleSection = (name: Section) => {
    setExpandedSection((prev) => (prev === name ? null : name));
  };

  const sections: { id: Section; label: string; hasValue: boolean }[] = [
    { id: 'lfo', label: 'LFO', hasValue: effectParams.filterLfoDepth > 0 },
    { id: 'delay', label: 'Delay', hasValue: effectParams.delayWet > 0 },
    { id: 'distortion', label: 'Drive', hasValue: effectParams.distortionWet > 0 },
    { id: 'eq', label: 'EQ', hasValue: effectParams.eqLow !== 0 || effectParams.eqMid !== 0 || effectParams.eqHigh !== 0 },
    { id: 'compressor', label: 'Comp', hasValue: effectParams.compRatio > 1 },
  ];

  return (
    <div className="flex h-[68px] shrink-0 items-center gap-4 border-b border-hairline bg-surface-1 px-3">
      {/* Title */}
      <div className="flex w-[72px] shrink-0 flex-col gap-1">
        <span className="daw-label">Live FX</span>
        <Select value={effectParams.filterType ?? 'lowpass'} onValueChange={(v) => setFilterType(v as FilterType)}>
          <SelectTrigger size="sm" className="h-6 w-[64px] px-1.5 text-[11px] font-semibold" aria-label="Filter type">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {FILTER_TYPES.map((t) => (
              <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>

      {/* Main controls */}
      <div className="flex items-center gap-3">
        <Knob label="Cutoff" value={effectParams.filterCutoff} min={60} max={18000} step={1} defaultValue={D.filterCutoff}
          formatValue={fmtHz} color={ACCENT} size="sm"
          onChange={(v) => setEffect('filterCutoff', v)} />
        <Knob label="Reso" value={effectParams.filterResonance} min={0} max={20} step={0.1} defaultValue={D.filterResonance}
          formatValue={fmtQ} color={ACCENT} size="sm"
          onChange={(v) => setEffect('filterResonance', v)} />
        <div className="h-9 w-px bg-hairline-strong" />
        <Knob label="Reverb" value={effectParams.reverbWet} min={0} max={1} step={0.01} defaultValue={D.reverbWet}
          formatValue={fmtPct} color={ACCENT} size="sm"
          onChange={(v) => setEffect('reverbWet', v)} />
        <Knob label="Chorus" value={effectParams.chorusDepth} min={0} max={1} step={0.01} defaultValue={D.chorusDepth}
          formatValue={fmtPct} color={ACCENT} size="sm"
          onChange={(v) => setEffect('chorusDepth', v)} />
      </div>

      {/* Expanded section, inline */}
      {expandedSection && (
        <div className="flex h-[60px] items-center gap-3 rounded-md border border-border bg-surface-0 px-3 animate-fade-in">
          <span className="daw-label w-10 text-primary">{sections.find((x) => x.id === expandedSection)?.label}</span>

          {expandedSection === 'lfo' && (
            <>
              <Knob label="Rate" value={effectParams.filterLfoRate} min={0.1} max={20} step={0.1} defaultValue={D.filterLfoRate}
                formatValue={fmtRate} color={ACCENT} size="sm"
                onChange={(v) => setEffect('filterLfoRate', v)} />
              <Knob label="Depth" value={effectParams.filterLfoDepth} min={0} max={1} step={0.01} defaultValue={D.filterLfoDepth}
                formatValue={fmtPct} color={ACCENT} size="sm"
                onChange={(v) => setEffect('filterLfoDepth', v)} />
              <Select value={effectParams.filterLfoWave ?? 'sine'} onValueChange={(v) => setLfoWave(v as LfoWaveform)}>
                <SelectTrigger size="sm" className="h-6 w-[60px] px-1.5 text-[11px] font-semibold" aria-label="LFO waveform">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {LFO_WAVES.map((w) => (
                    <SelectItem key={w.value} value={w.value}>{w.label}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </>
          )}

          {expandedSection === 'delay' && (
            <>
              <Knob label="Time" value={effectParams.delayTime} min={0.01} max={1} step={0.01} defaultValue={D.delayTime}
                formatValue={fmtMs} color={ACCENT} size="sm"
                onChange={(v) => setEffect('delayTime', v)} />
              <Knob label="Feedback" value={effectParams.delayFeedback} min={0} max={0.95} step={0.01} defaultValue={D.delayFeedback}
                formatValue={fmtPct} color={ACCENT} size="sm"
                onChange={(v) => setEffect('delayFeedback', v)} />
              <Knob label="Mix" value={effectParams.delayWet} min={0} max={1} step={0.01} defaultValue={D.delayWet}
                formatValue={fmtPct} color={ACCENT} size="sm"
                onChange={(v) => setEffect('delayWet', v)} />
            </>
          )}

          {expandedSection === 'distortion' && (
            <>
              <Knob label="Drive" value={effectParams.distortionAmount} min={0} max={1} step={0.01} defaultValue={D.distortionAmount}
                formatValue={fmtPct} color={ACCENT} size="sm"
                onChange={(v) => setEffect('distortionAmount', v)} />
              <Knob label="Mix" value={effectParams.distortionWet} min={0} max={1} step={0.01} defaultValue={D.distortionWet}
                formatValue={fmtPct} color={ACCENT} size="sm"
                onChange={(v) => setEffect('distortionWet', v)} />
            </>
          )}

          {expandedSection === 'eq' && (
            <>
              <Knob label="Low" value={effectParams.eqLow} min={-12} max={12} step={0.5} defaultValue={D.eqLow}
                formatValue={fmtDb} color={ACCENT} size="sm"
                onChange={(v) => setEffect('eqLow', v)} />
              <Knob label="Mid" value={effectParams.eqMid} min={-12} max={12} step={0.5} defaultValue={D.eqMid}
                formatValue={fmtDb} color={ACCENT} size="sm"
                onChange={(v) => setEffect('eqMid', v)} />
              <Knob label="High" value={effectParams.eqHigh} min={-12} max={12} step={0.5} defaultValue={D.eqHigh}
                formatValue={fmtDb} color={ACCENT} size="sm"
                onChange={(v) => setEffect('eqHigh', v)} />
            </>
          )}

          {expandedSection === 'compressor' && (
            <>
              <Knob label="Thresh" value={effectParams.compThreshold} min={-60} max={0} step={1} defaultValue={D.compThreshold}
                formatValue={(v) => `${v}dB`} color={ACCENT} size="sm"
                onChange={(v) => setEffect('compThreshold', v)} />
              <Knob label="Ratio" value={effectParams.compRatio} min={1} max={20} step={0.5} defaultValue={D.compRatio}
                formatValue={fmtRatio} color={ACCENT} size="sm"
                onChange={(v) => setEffect('compRatio', v)} />
              <Knob label="Attack" value={effectParams.compAttack} min={0.001} max={1} step={0.001} defaultValue={D.compAttack}
                formatValue={fmtMs} color={ACCENT} size="sm"
                onChange={(v) => setEffect('compAttack', v)} />
              <Knob label="Release" value={effectParams.compRelease} min={0.01} max={1} step={0.01} defaultValue={D.compRelease}
                formatValue={fmtMs} color={ACCENT} size="sm"
                onChange={(v) => setEffect('compRelease', v)} />
            </>
          )}
        </div>
      )}

      {/* Section tabs */}
      <div className="ml-auto flex h-7 items-center gap-0.5 rounded-md border border-border bg-surface-0 p-0.5" role="tablist" aria-label="More effects">
        {sections.map((sec) => (
          <SectionToggle
            key={sec.id}
            label={sec.label}
            active={expandedSection === sec.id}
            hasValue={sec.hasValue}
            onClick={() => toggleSection(sec.id)}
          />
        ))}
      </div>
    </div>
  );
}

function SectionToggle({ label, active, hasValue, onClick }: { label: string; active: boolean; hasValue: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      className={cn(
        'relative flex h-[22px] items-center gap-1.5 rounded-[4px] px-2 text-[10.5px] font-bold uppercase tracking-[0.06em] transition-colors duration-150 focus-visible:outline-2 focus-visible:outline-ring',
        active ? 'bg-surface-3 text-foreground' : 'text-muted-foreground hover:bg-surface-2 hover:text-foreground',
      )}
      onClick={onClick}
      title={hasValue ? `${label} (active)` : label}
    >
      <span className={cn('size-1.5 rounded-full', hasValue ? 'bg-primary' : 'bg-surface-4')} />
      {label}
    </button>
  );
}
