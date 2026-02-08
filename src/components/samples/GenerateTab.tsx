import { useState, useCallback } from 'react';
import { Loader2, Volume2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { Badge } from '@/components/ui/badge';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';
import { DEFAULT_SIGNAL_PARAMS } from '@/types/samples';
import type { SignalGeneratorParams, SignalType } from '@/types/samples';
import { useSignalPreview } from '@/hooks/useSignalPreview';

interface GenerateTabProps {
  isProcessing: boolean;
  onGenerate: (params: SignalGeneratorParams) => void;
}

function ParamSlider({
  label,
  value,
  min,
  max,
  step,
  displayValue,
  disabled,
  suffix,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  displayValue: string;
  disabled?: boolean;
  suffix?: React.ReactNode;
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
        disabled={disabled}
        className="flex-1"
      />
      <Badge variant="secondary" className="w-14 justify-center font-mono text-[10px]">
        {displayValue}
      </Badge>
      {suffix}
    </div>
  );
}

export function GenerateTab({ isProcessing, onGenerate }: GenerateTabProps) {
  const [params, setParams] = useState<SignalGeneratorParams>({ ...DEFAULT_SIGNAL_PARAMS });
  const [detectedNote, setDetectedNote] = useState<string | null>(null);

  const isNoise = params.type === 'white-noise';

  const handleFrequencyDetected = useCallback((hz: number, noteName: string) => {
    setParams((p) => ({ ...p, frequency: Math.round(hz) }));
    setDetectedNote(noteName);
  }, []);

  const { startPreviewTone, stopPreviewTone } = useSignalPreview({
    type: params.type,
    amplitude: params.amplitude,
    frequency: params.frequency,
    active: true,
    onFrequencyDetected: handleFrequencyDetected,
  });

  return (
    <div className="flex flex-col gap-4">
      {/* Waveform type */}
      <div className="flex flex-col gap-1">
        <div className="flex items-center gap-2">
          <span className="w-16 text-[11px] font-semibold uppercase tracking-wider text-muted-foreground">
            Waveform
          </span>
          <Select
            value={params.type}
            onValueChange={(v) => {
              setParams((p) => ({ ...p, type: v as SignalType }));
              setDetectedNote(null);
            }}
          >
            <SelectTrigger className="h-7 w-36 text-xs">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="sine">Sine</SelectItem>
              <SelectItem value="square">Square</SelectItem>
              <SelectItem value="sawtooth">Sawtooth</SelectItem>
              <SelectItem value="triangle">Triangle</SelectItem>
              <SelectItem value="white-noise">White Noise</SelectItem>
            </SelectContent>
          </Select>
        </div>
        <p className="pl-16 text-[10px] text-muted-foreground/70">
          Play MIDI keys to preview
        </p>
      </div>

      {/* Frequency */}
      <ParamSlider
        label="Freq"
        value={params.frequency}
        min={20}
        max={20000}
        step={1}
        displayValue={params.frequency >= 1000 ? `${(params.frequency / 1000).toFixed(1)}k` : `${Math.round(params.frequency)}Hz`}
        disabled={isNoise}
        onChange={(v) => {
          setParams((p) => ({ ...p, frequency: v }));
          setDetectedNote(null);
        }}
        suffix={detectedNote && !isNoise ? (
          <Badge variant="outline" className="w-10 justify-center font-mono text-[10px]">
            {detectedNote}
          </Badge>
        ) : null}
      />

      {/* Duration */}
      <ParamSlider
        label="Duration"
        value={params.duration}
        min={0.1}
        max={30}
        step={0.1}
        displayValue={`${params.duration.toFixed(1)}s`}
        onChange={(v) => setParams((p) => ({ ...p, duration: v }))}
      />

      {/* Amplitude */}
      <ParamSlider
        label="Level"
        value={params.amplitude}
        min={0}
        max={1}
        step={0.01}
        displayValue={`${Math.round(params.amplitude * 100)}%`}
        onChange={(v) => setParams((p) => ({ ...p, amplitude: v }))}
      />

      {/* Preview + Generate buttons */}
      <div className="flex items-center gap-2 pt-2">
        <Button
          variant="outline"
          size="sm"
          className="gap-1.5"
          onPointerDown={startPreviewTone}
          onPointerUp={stopPreviewTone}
          onPointerLeave={stopPreviewTone}
        >
          <Volume2 className="h-3.5 w-3.5" />
          Hold to Preview
        </Button>
        <Button
          size="sm"
          disabled={isProcessing}
          onClick={() => onGenerate(params)}
          className="gap-1.5"
        >
          {isProcessing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
          {isProcessing ? 'Generating...' : 'Generate'}
        </Button>
      </div>
    </div>
  );
}
