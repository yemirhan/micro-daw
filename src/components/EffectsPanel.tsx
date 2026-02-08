import { useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { Card } from '@/components/ui/card';
import { Slider } from '@/components/ui/slider';
import { Badge } from '@/components/ui/badge';
import { audioEngine } from '@/services/AudioEngine';
import type { EffectParams } from '@/types/effects';

interface EffectsPanelProps {
  effectParams: EffectParams;
  onEffectChange: (params: EffectParams) => void;
}

export function EffectsPanel({ effectParams, onEffectChange }: EffectsPanelProps) {
  const [expandedSection, setExpandedSection] = useState<string | null>(null);

  const setEffect = (key: keyof EffectParams, value: number) => {
    switch (key) {
      case 'reverbWet': audioEngine.setReverbWet(value); break;
      case 'chorusDepth': audioEngine.setChorusDepth(value); break;
      case 'filterCutoff': audioEngine.setFilterCutoff(value); break;
      case 'filterResonance': audioEngine.setFilterResonance(value); break;
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

  const toggleSection = (name: string) => {
    setExpandedSection((prev) => (prev === name ? null : name));
  };

  return (
    <Card className="flex flex-col border-border bg-card/60 backdrop-blur-sm px-4 py-2 gap-2">
      {/* Main row — always visible */}
      <div className="flex items-center gap-6">
        <EffectSlider
          label="Reverb"
          value={effectParams.reverbWet}
          min={0} max={1} step={0.01}
          displayValue={`${Math.round(effectParams.reverbWet * 100)}%`}
          onChange={(v) => setEffect('reverbWet', v)}
        />
        <EffectSlider
          label="Chorus"
          value={effectParams.chorusDepth}
          min={0} max={1} step={0.01}
          displayValue={`${Math.round(effectParams.chorusDepth * 100)}%`}
          onChange={(v) => setEffect('chorusDepth', v)}
        />
        <EffectSlider
          label="Filter"
          value={effectParams.filterCutoff}
          min={60} max={18000} step={1}
          displayValue={effectParams.filterCutoff >= 1000
            ? `${(effectParams.filterCutoff / 1000).toFixed(1)}k`
            : `${Math.round(effectParams.filterCutoff)}`}
          onChange={(v) => setEffect('filterCutoff', v)}
        />
        <EffectSlider
          label="Resonance"
          value={effectParams.filterResonance}
          min={0} max={20} step={0.1}
          displayValue={effectParams.filterResonance.toFixed(1)}
          onChange={(v) => setEffect('filterResonance', v)}
        />

        {/* Section toggles */}
        <div className="flex items-center gap-1 ml-auto">
          <SectionToggle label="Delay" active={expandedSection === 'delay'} hasValue={effectParams.delayWet > 0} onClick={() => toggleSection('delay')} />
          <SectionToggle label="Dist" active={expandedSection === 'distortion'} hasValue={effectParams.distortionWet > 0} onClick={() => toggleSection('distortion')} />
          <SectionToggle label="EQ" active={expandedSection === 'eq'} hasValue={effectParams.eqLow !== 0 || effectParams.eqMid !== 0 || effectParams.eqHigh !== 0} onClick={() => toggleSection('eq')} />
          <SectionToggle label="Comp" active={expandedSection === 'compressor'} hasValue={effectParams.compThreshold > -24} onClick={() => toggleSection('compressor')} />
        </div>
      </div>

      {/* Expandable sections */}
      {expandedSection === 'delay' && (
        <div className="flex items-center gap-6 border-t border-border/50 pt-2">
          <EffectSlider label="Time" value={effectParams.delayTime} min={0.01} max={1} step={0.01}
            displayValue={`${Math.round(effectParams.delayTime * 1000)}ms`}
            onChange={(v) => setEffect('delayTime', v)} />
          <EffectSlider label="Feedback" value={effectParams.delayFeedback} min={0} max={0.95} step={0.01}
            displayValue={`${Math.round(effectParams.delayFeedback * 100)}%`}
            onChange={(v) => setEffect('delayFeedback', v)} />
          <EffectSlider label="Mix" value={effectParams.delayWet} min={0} max={1} step={0.01}
            displayValue={`${Math.round(effectParams.delayWet * 100)}%`}
            onChange={(v) => setEffect('delayWet', v)} />
        </div>
      )}

      {expandedSection === 'distortion' && (
        <div className="flex items-center gap-6 border-t border-border/50 pt-2">
          <EffectSlider label="Drive" value={effectParams.distortionAmount} min={0} max={1} step={0.01}
            displayValue={`${Math.round(effectParams.distortionAmount * 100)}%`}
            onChange={(v) => setEffect('distortionAmount', v)} />
          <EffectSlider label="Mix" value={effectParams.distortionWet} min={0} max={1} step={0.01}
            displayValue={`${Math.round(effectParams.distortionWet * 100)}%`}
            onChange={(v) => setEffect('distortionWet', v)} />
        </div>
      )}

      {expandedSection === 'eq' && (
        <div className="flex items-center gap-6 border-t border-border/50 pt-2">
          <EffectSlider label="Low" value={effectParams.eqLow} min={-12} max={12} step={0.5}
            displayValue={`${effectParams.eqLow > 0 ? '+' : ''}${effectParams.eqLow.toFixed(1)}dB`}
            onChange={(v) => setEffect('eqLow', v)} />
          <EffectSlider label="Mid" value={effectParams.eqMid} min={-12} max={12} step={0.5}
            displayValue={`${effectParams.eqMid > 0 ? '+' : ''}${effectParams.eqMid.toFixed(1)}dB`}
            onChange={(v) => setEffect('eqMid', v)} />
          <EffectSlider label="High" value={effectParams.eqHigh} min={-12} max={12} step={0.5}
            displayValue={`${effectParams.eqHigh > 0 ? '+' : ''}${effectParams.eqHigh.toFixed(1)}dB`}
            onChange={(v) => setEffect('eqHigh', v)} />
        </div>
      )}

      {expandedSection === 'compressor' && (
        <div className="flex items-center gap-6 border-t border-border/50 pt-2">
          <EffectSlider label="Thresh" value={effectParams.compThreshold} min={-60} max={0} step={1}
            displayValue={`${effectParams.compThreshold}dB`}
            onChange={(v) => setEffect('compThreshold', v)} />
          <EffectSlider label="Ratio" value={effectParams.compRatio} min={1} max={20} step={0.5}
            displayValue={`${effectParams.compRatio.toFixed(1)}:1`}
            onChange={(v) => setEffect('compRatio', v)} />
          <EffectSlider label="Attack" value={effectParams.compAttack} min={0.001} max={1} step={0.001}
            displayValue={`${Math.round(effectParams.compAttack * 1000)}ms`}
            onChange={(v) => setEffect('compAttack', v)} />
          <EffectSlider label="Release" value={effectParams.compRelease} min={0.01} max={1} step={0.01}
            displayValue={`${Math.round(effectParams.compRelease * 1000)}ms`}
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

interface EffectSliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  displayValue: string;
  onChange: (value: number) => void;
}

function EffectSlider({ label, value, min, max, step, displayValue, onChange }: EffectSliderProps) {
  return (
    <div className="flex flex-1 items-center gap-2">
      <span className="w-16 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">{label}</span>
      <Slider
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={([v]) => onChange(v)}
        className="flex-1"
      />
      <Badge variant="secondary" className="w-12 justify-center font-mono text-[10px]">
        {displayValue}
      </Badge>
    </div>
  );
}
