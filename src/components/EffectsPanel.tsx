import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Knob } from '@/components/ui/Knob';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { audioEngine } from '@/services/AudioEngine';
import type { EffectParams, FilterType, LfoWaveform } from '@/types/effects';

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

const ACCENT = 'oklch(0.65 0.15 265)';

export function EffectsPanel({ effectParams, onEffectChange }: EffectsPanelProps) {
  const [expandedSection, setExpandedSection] = useState<string | null>(null);

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

  const toggleSection = (name: string) => {
    setExpandedSection((prev) => (prev === name ? null : name));
  };

  return (
    <Card className="flex flex-col border-border bg-card/60 backdrop-blur-sm px-4 py-2 gap-2">
      {/* Main row — always visible */}
      <div className="flex items-center gap-4">
        <Select value={effectParams.filterType ?? 'lowpass'} onValueChange={(v) => setFilterType(v as FilterType)}>
          <SelectTrigger className="h-6 w-16 text-[10px] border-border/50">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {FILTER_TYPES.map((t) => (
              <SelectItem key={t.value} value={t.value}>{t.label}</SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Knob label="Cutoff" value={effectParams.filterCutoff} min={60} max={18000} step={1} defaultValue={18000}
          formatValue={fmtHz} color={ACCENT} size="sm"
          onChange={(v) => setEffect('filterCutoff', v)} />
        <Knob label="Reso" value={effectParams.filterResonance} min={0} max={20} step={0.1} defaultValue={1}
          formatValue={fmtQ} color={ACCENT} size="sm"
          onChange={(v) => setEffect('filterResonance', v)} />
        <Knob label="Reverb" value={effectParams.reverbWet} min={0} max={1} step={0.01} defaultValue={0}
          formatValue={fmtPct} color={ACCENT} size="sm"
          onChange={(v) => setEffect('reverbWet', v)} />
        <Knob label="Chorus" value={effectParams.chorusDepth} min={0} max={1} step={0.01} defaultValue={0}
          formatValue={fmtPct} color={ACCENT} size="sm"
          onChange={(v) => setEffect('chorusDepth', v)} />

        {/* Section toggles */}
        <div className="flex items-center gap-1 ml-auto">
          <SectionToggle label="LFO" active={expandedSection === 'lfo'} hasValue={effectParams.filterLfoDepth > 0} onClick={() => toggleSection('lfo')} />
          <SectionToggle label="Delay" active={expandedSection === 'delay'} hasValue={effectParams.delayWet > 0} onClick={() => toggleSection('delay')} />
          <SectionToggle label="Dist" active={expandedSection === 'distortion'} hasValue={effectParams.distortionWet > 0} onClick={() => toggleSection('distortion')} />
          <SectionToggle label="EQ" active={expandedSection === 'eq'} hasValue={effectParams.eqLow !== 0 || effectParams.eqMid !== 0 || effectParams.eqHigh !== 0} onClick={() => toggleSection('eq')} />
          <SectionToggle label="Comp" active={expandedSection === 'compressor'} hasValue={effectParams.compThreshold > -24} onClick={() => toggleSection('compressor')} />
        </div>
      </div>

      {/* Filter LFO section */}
      {expandedSection === 'lfo' && (
        <div className="flex items-center gap-4 border-t border-border/50 pt-2">
          <Knob label="Rate" value={effectParams.filterLfoRate} min={0.1} max={20} step={0.1} defaultValue={1}
            formatValue={fmtRate} color={ACCENT} size="sm"
            onChange={(v) => setEffect('filterLfoRate', v)} />
          <Knob label="Depth" value={effectParams.filterLfoDepth} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={ACCENT} size="sm"
            onChange={(v) => setEffect('filterLfoDepth', v)} />
          <Select value={effectParams.filterLfoWave ?? 'sine'} onValueChange={(v) => setLfoWave(v as LfoWaveform)}>
            <SelectTrigger className="h-6 w-14 text-[10px] border-border/50">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {LFO_WAVES.map((w) => (
                <SelectItem key={w.value} value={w.value}>{w.label}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      {/* Delay section */}
      {expandedSection === 'delay' && (
        <div className="flex items-center gap-4 border-t border-border/50 pt-2">
          <Knob label="Time" value={effectParams.delayTime} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={ACCENT} size="sm"
            onChange={(v) => setEffect('delayTime', v)} />
          <Knob label="Feedback" value={effectParams.delayFeedback} min={0} max={0.95} step={0.01} defaultValue={0.3}
            formatValue={fmtPct} color={ACCENT} size="sm"
            onChange={(v) => setEffect('delayFeedback', v)} />
          <Knob label="Mix" value={effectParams.delayWet} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={ACCENT} size="sm"
            onChange={(v) => setEffect('delayWet', v)} />
        </div>
      )}

      {/* Distortion section */}
      {expandedSection === 'distortion' && (
        <div className="flex items-center gap-4 border-t border-border/50 pt-2">
          <Knob label="Drive" value={effectParams.distortionAmount} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={ACCENT} size="sm"
            onChange={(v) => setEffect('distortionAmount', v)} />
          <Knob label="Mix" value={effectParams.distortionWet} min={0} max={1} step={0.01} defaultValue={0}
            formatValue={fmtPct} color={ACCENT} size="sm"
            onChange={(v) => setEffect('distortionWet', v)} />
        </div>
      )}

      {/* EQ section */}
      {expandedSection === 'eq' && (
        <div className="flex items-center gap-4 border-t border-border/50 pt-2">
          <Knob label="Low" value={effectParams.eqLow} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={ACCENT} size="sm"
            onChange={(v) => setEffect('eqLow', v)} />
          <Knob label="Mid" value={effectParams.eqMid} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={ACCENT} size="sm"
            onChange={(v) => setEffect('eqMid', v)} />
          <Knob label="High" value={effectParams.eqHigh} min={-12} max={12} step={0.5} defaultValue={0}
            formatValue={fmtDb} color={ACCENT} size="sm"
            onChange={(v) => setEffect('eqHigh', v)} />
        </div>
      )}

      {/* Compressor section */}
      {expandedSection === 'compressor' && (
        <div className="flex items-center gap-4 border-t border-border/50 pt-2">
          <Knob label="Thresh" value={effectParams.compThreshold} min={-60} max={0} step={1} defaultValue={-24}
            formatValue={(v) => `${v}dB`} color={ACCENT} size="sm"
            onChange={(v) => setEffect('compThreshold', v)} />
          <Knob label="Ratio" value={effectParams.compRatio} min={1} max={20} step={0.5} defaultValue={4}
            formatValue={fmtRatio} color={ACCENT} size="sm"
            onChange={(v) => setEffect('compRatio', v)} />
          <Knob label="Attack" value={effectParams.compAttack} min={0.001} max={1} step={0.001} defaultValue={0.003}
            formatValue={fmtMs} color={ACCENT} size="sm"
            onChange={(v) => setEffect('compAttack', v)} />
          <Knob label="Release" value={effectParams.compRelease} min={0.01} max={1} step={0.01} defaultValue={0.25}
            formatValue={fmtMs} color={ACCENT} size="sm"
            onChange={(v) => setEffect('compRelease', v)} />
        </div>
      )}
    </Card>
  );
}

function SectionToggle({ label, active, hasValue, onClick }: { label: string; active: boolean; hasValue: boolean; onClick: () => void }) {
  return (
    <button
      className={`flex items-center gap-0.5 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wider transition-colors ${
        active
          ? 'bg-primary/20 text-primary'
          : hasValue
            ? 'text-primary/70 hover:bg-primary/10'
            : 'text-muted-foreground hover:bg-muted'
      }`}
      onClick={onClick}
    >
      {active ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
      {label}
    </button>
  );
}
