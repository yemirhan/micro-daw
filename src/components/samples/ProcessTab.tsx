import { useState } from 'react';
import { Loader2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { Badge } from '@/components/ui/badge';
import { Switch } from '@/components/ui/switch';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { DEFAULT_PROCESSING } from '@/types/samples';
import type { SampleProcessingParams, SampleFilterType } from '@/types/samples';

interface ProcessTabProps {
  isProcessing: boolean;
  onProcess: (params: SampleProcessingParams) => void;
}

function ParamSlider({
  label,
  value,
  min,
  max,
  step,
  displayValue,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  displayValue: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="flex flex-1 items-center gap-2">
      <span className="w-16 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
      <Slider
        min={min}
        max={max}
        step={step}
        value={[value]}
        onValueChange={([v]) => onChange(v)}
        className="flex-1"
      />
      <Badge variant="secondary" className="w-14 justify-center font-mono text-[10px]">
        {displayValue}
      </Badge>
    </div>
  );
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
    <div className="flex flex-col gap-4">
      {/* Filter */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={params.filter.enabled}
            onCheckedChange={(v) => update('filter', { enabled: v })}
          />
          <span className="text-xs font-semibold">Filter</span>
        </div>
        {params.filter.enabled && (
          <div className="flex flex-col gap-2 pl-10">
            <Select
              value={params.filter.type}
              onValueChange={(v) => update('filter', { type: v as SampleFilterType })}
            >
              <SelectTrigger className="h-7 w-32 text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="lowpass">Lowpass</SelectItem>
                <SelectItem value="highpass">Highpass</SelectItem>
                <SelectItem value="bandpass">Bandpass</SelectItem>
              </SelectContent>
            </Select>
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
          </div>
        )}
      </section>

      {/* Pitch Shift */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={params.pitchShift.enabled}
            onCheckedChange={(v) => update('pitchShift', { enabled: v })}
          />
          <span className="text-xs font-semibold">Pitch Shift</span>
        </div>
        {params.pitchShift.enabled && (
          <div className="pl-10">
            <ParamSlider
              label="Semitones"
              value={params.pitchShift.semitones}
              min={-24}
              max={24}
              step={1}
              displayValue={`${params.pitchShift.semitones > 0 ? '+' : ''}${params.pitchShift.semitones}`}
              onChange={(v) => update('pitchShift', { semitones: v })}
            />
          </div>
        )}
      </section>

      {/* Reverse */}
      <section className="flex items-center gap-2">
        <Switch
          checked={params.reverse.enabled}
          onCheckedChange={(v) => update('reverse', { enabled: v })}
        />
        <span className="text-xs font-semibold">Reverse</span>
      </section>

      {/* Normalize */}
      <section className="flex items-center gap-2">
        <Switch
          checked={params.normalize.enabled}
          onCheckedChange={(v) => update('normalize', { enabled: v })}
        />
        <span className="text-xs font-semibold">Normalize</span>
      </section>

      {/* Reverb */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={params.reverb.enabled}
            onCheckedChange={(v) => update('reverb', { enabled: v })}
          />
          <span className="text-xs font-semibold">Reverb</span>
        </div>
        {params.reverb.enabled && (
          <div className="flex flex-col gap-2 pl-10">
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
          </div>
        )}
      </section>

      {/* Delay */}
      <section className="flex flex-col gap-2">
        <div className="flex items-center gap-2">
          <Switch
            checked={params.delay.enabled}
            onCheckedChange={(v) => update('delay', { enabled: v })}
          />
          <span className="text-xs font-semibold">Delay</span>
        </div>
        {params.delay.enabled && (
          <div className="flex flex-col gap-2 pl-10">
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
          </div>
        )}
      </section>

      {/* Apply button */}
      <div className="pt-2">
        <Button
          size="sm"
          disabled={isProcessing || !anyEnabled}
          onClick={() => onProcess(params)}
          className="gap-1.5"
        >
          {isProcessing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {isProcessing ? 'Processing...' : 'Apply'}
        </Button>
      </div>
    </div>
  );
}
