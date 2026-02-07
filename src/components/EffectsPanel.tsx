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
  const setEffect = (key: keyof EffectParams, value: number) => {
    switch (key) {
      case 'reverbWet':
        audioEngine.setReverbWet(value);
        break;
      case 'chorusDepth':
        audioEngine.setChorusDepth(value);
        break;
      case 'filterCutoff':
        audioEngine.setFilterCutoff(value);
        break;
      case 'filterResonance':
        audioEngine.setFilterResonance(value);
        break;
    }
    onEffectChange(audioEngine.getEffectParams());
  };

  return (
    <Card className="flex flex-row items-center gap-6 border-border bg-card/60 backdrop-blur-sm px-4 py-2">
      <EffectSlider
        label="Reverb"
        value={effectParams.reverbWet}
        min={0}
        max={1}
        step={0.01}
        displayValue={`${Math.round(effectParams.reverbWet * 100)}%`}
        onChange={(v) => setEffect('reverbWet', v)}
      />
      <EffectSlider
        label="Chorus"
        value={effectParams.chorusDepth}
        min={0}
        max={1}
        step={0.01}
        displayValue={`${Math.round(effectParams.chorusDepth * 100)}%`}
        onChange={(v) => setEffect('chorusDepth', v)}
      />
      <EffectSlider
        label="Filter"
        value={effectParams.filterCutoff}
        min={60}
        max={18000}
        step={1}
        displayValue={effectParams.filterCutoff >= 1000
          ? `${(effectParams.filterCutoff / 1000).toFixed(1)}k`
          : `${Math.round(effectParams.filterCutoff)}`}
        onChange={(v) => setEffect('filterCutoff', v)}
      />
      <EffectSlider
        label="Resonance"
        value={effectParams.filterResonance}
        min={0}
        max={20}
        step={0.1}
        displayValue={effectParams.filterResonance.toFixed(1)}
        onChange={(v) => setEffect('filterResonance', v)}
      />
    </Card>
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
