import { useState, useCallback } from 'react';
import { Loader2, Volume2, Wand2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { FilterChips, Kbd } from '@/components/ui/view-kit';
import { ParamSlider } from './ParamControls';
import { DEFAULT_SIGNAL_PARAMS } from '@/types/samples';
import type { SignalGeneratorParams, SignalType } from '@/types/samples';
import { useSignalPreview } from '@/hooks/useSignalPreview';

const WAVEFORMS: { value: SignalType; label: string }[] = [
  { value: 'sine', label: 'Sine' },
  { value: 'square', label: 'Square' },
  { value: 'sawtooth', label: 'Saw' },
  { value: 'triangle', label: 'Triangle' },
  { value: 'white-noise', label: 'Noise' },
];

interface GenerateTabProps {
  isProcessing: boolean;
  onGenerate: (params: SignalGeneratorParams) => void;
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
      <p className="text-xs text-muted-foreground">
        Render a test tone or noise into a new sample. Hold <span className="font-semibold text-foreground">Preview</span>{' '}
        or play your MIDI keyboard to hear it; a played key also sets the frequency.
      </p>

      <div className="flex flex-col gap-1 rounded-lg border border-border bg-surface-1 px-3.5 py-3">
        <div className="flex h-9 items-center gap-3">
          <span className="w-20 shrink-0 text-[10.5px] font-bold uppercase tracking-[0.08em] text-muted-foreground">Waveform</span>
          <FilterChips
            ariaLabel="Waveform"
            options={WAVEFORMS}
            value={params.type}
            onChange={(v) => {
              setParams((p) => ({ ...p, type: v }));
              setDetectedNote(null);
            }}
          />
        </div>

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
          suffix={
            <span className="w-10 shrink-0 font-mono text-[11px] font-semibold text-primary">
              {detectedNote && !isNoise ? detectedNote : ''}
            </span>
          }
        />
        <ParamSlider
          label="Duration"
          value={params.duration}
          min={0.1}
          max={30}
          step={0.1}
          displayValue={`${params.duration.toFixed(1)}s`}
          onChange={(v) => setParams((p) => ({ ...p, duration: v }))}
          suffix={<span className="w-10 shrink-0" />}
        />
        <ParamSlider
          label="Level"
          value={params.amplitude}
          min={0}
          max={1}
          step={0.01}
          displayValue={`${Math.round(params.amplitude * 100)}%`}
          onChange={(v) => setParams((p) => ({ ...p, amplitude: v }))}
          suffix={<span className="w-10 shrink-0" />}
        />
      </div>

      <div className="flex items-center gap-2">
        <Button
          variant="outline"
          className="gap-1.5"
          onPointerDown={startPreviewTone}
          onPointerUp={stopPreviewTone}
          onPointerLeave={stopPreviewTone}
        >
          <Volume2 className="size-4" />
          Hold to preview
        </Button>
        <Button disabled={isProcessing} onClick={() => onGenerate(params)} className="gap-1.5">
          {isProcessing ? <Loader2 className="size-4 animate-spin" /> : <Wand2 className="size-4" />}
          {isProcessing ? 'Generating…' : 'Generate'}
        </Button>
        <span className="ml-2 inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
          <Kbd>MIDI</Kbd> keys preview at their pitch
        </span>
      </div>
    </div>
  );
}
