import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { DEFAULT_PROCESSING } from '@/types/samples';
import type { SampleProcessingParams, SampleFilterType } from '@/types/samples';
import { EffectSection, ParamSlider } from './ParamControls';

interface ProcessTabProps {
  isProcessing: boolean;
  onProcess: (params: SampleProcessingParams) => void;
}

export function ProcessTab({ isProcessing, onProcess }: ProcessTabProps) {
  const [params, setParams] = useState<SampleProcessingParams>({ ...DEFAULT_PROCESSING });

  const update = <K extends keyof SampleProcessingParams>(
    key: K,
    patch: Partial<SampleProcessingParams[K]>,
  ) => {
    setParams((p) => ({ ...p, [key]: { ...p[key], ...patch } }));
  };

  const anyEnabled =
    params.filter.enabled ||
    params.pitchShift.enabled ||
    params.reverse.enabled ||
    params.normalize.enabled ||
    params.reverb.enabled ||
    params.delay.enabled;

  return (
    <div className="flex flex-col gap-2">
      <p className="mb-2 text-xs text-muted-foreground">
        Switch on the stages you want; they run top to bottom and create a new sample, leaving the original untouched.
      </p>

      <EffectSection
        title="Filter"
        description="Cut lows or highs"
        enabled={params.filter.enabled}
        onEnabledChange={(v) => update('filter', { enabled: v })}
      >
        <div className="flex h-8 items-center gap-3">
          <span className="w-20 shrink-0 text-[10.5px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Type</span>
          <Select value={params.filter.type} onValueChange={(v) => update('filter', { type: v as SampleFilterType })}>
            <SelectTrigger className="h-7 w-32 text-xs" aria-label="Filter type">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="lowpass">Lowpass</SelectItem>
              <SelectItem value="highpass">Highpass</SelectItem>
              <SelectItem value="bandpass">Bandpass</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <ParamSlider
          label="Cutoff"
          value={params.filter.cutoff}
          min={20}
          max={20000}
          step={1}
          displayValue={params.filter.cutoff >= 1000 ? `${(params.filter.cutoff / 1000).toFixed(1)}k` : `${Math.round(params.filter.cutoff)}Hz`}
          onChange={(v) => update('filter', { cutoff: v })}
        />
        <ParamSlider
          label="Resonance"
          value={params.filter.resonance}
          min={0.1}
          max={20}
          step={0.1}
          displayValue={params.filter.resonance.toFixed(1)}
          onChange={(v) => update('filter', { resonance: v })}
        />
      </EffectSection>

      <EffectSection
        title="Pitch shift"
        description="Up or down in semitones"
        enabled={params.pitchShift.enabled}
        onEnabledChange={(v) => update('pitchShift', { enabled: v })}
      >
        <ParamSlider
          label="Semitones"
          value={params.pitchShift.semitones}
          min={-24}
          max={24}
          step={1}
          displayValue={`${params.pitchShift.semitones > 0 ? '+' : ''}${params.pitchShift.semitones}`}
          onChange={(v) => update('pitchShift', { semitones: v })}
        />
      </EffectSection>

      <EffectSection
        title="Reverse"
        description="Play the sample backwards"
        enabled={params.reverse.enabled}
        onEnabledChange={(v) => update('reverse', { enabled: v })}
      />

      <EffectSection
        title="Normalize"
        description="Raise the peak to full scale"
        enabled={params.normalize.enabled}
        onEnabledChange={(v) => update('normalize', { enabled: v })}
      />

      <EffectSection
        title="Reverb"
        description="Add room and tail"
        enabled={params.reverb.enabled}
        onEnabledChange={(v) => update('reverb', { enabled: v })}
      >
        <ParamSlider
          label="Decay"
          value={params.reverb.decay}
          min={0.1}
          max={10}
          step={0.1}
          displayValue={`${params.reverb.decay.toFixed(1)}s`}
          onChange={(v) => update('reverb', { decay: v })}
        />
        <ParamSlider
          label="Wet"
          value={params.reverb.wet}
          min={0}
          max={1}
          step={0.01}
          displayValue={`${Math.round(params.reverb.wet * 100)}%`}
          onChange={(v) => update('reverb', { wet: v })}
        />
      </EffectSection>

      <EffectSection
        title="Delay"
        description="Echoes"
        enabled={params.delay.enabled}
        onEnabledChange={(v) => update('delay', { enabled: v })}
      >
        <ParamSlider
          label="Time"
          value={params.delay.time}
          min={0.01}
          max={1}
          step={0.01}
          displayValue={`${Math.round(params.delay.time * 1000)}ms`}
          onChange={(v) => update('delay', { time: v })}
        />
        <ParamSlider
          label="Feedback"
          value={params.delay.feedback}
          min={0}
          max={0.95}
          step={0.01}
          displayValue={`${Math.round(params.delay.feedback * 100)}%`}
          onChange={(v) => update('delay', { feedback: v })}
        />
        <ParamSlider
          label="Wet"
          value={params.delay.wet}
          min={0}
          max={1}
          step={0.01}
          displayValue={`${Math.round(params.delay.wet * 100)}%`}
          onChange={(v) => update('delay', { wet: v })}
        />
      </EffectSection>

      <div className="flex items-center gap-3 pt-3">
        <Button disabled={isProcessing || !anyEnabled} onClick={() => onProcess(params)} className="gap-1.5">
          {isProcessing && <Loader2 className="size-4 animate-spin" />}
          {isProcessing ? 'Processing…' : 'Apply to new sample'}
        </Button>
        {!anyEnabled && <span className="text-xs text-muted-foreground">Switch on at least one stage</span>}
      </div>
    </div>
  );
}
