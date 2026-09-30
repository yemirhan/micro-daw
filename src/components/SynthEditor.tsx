import { useMemo } from 'react';
import { Knob } from '@/components/ui/Knob';
import { SYNTH_PRESETS, PRESET_CATEGORIES } from '@/utils/constants';
import type { SynthPreset, SynthEngine, PresetCategory } from '@/types/audio';

interface SynthEditorProps {
  presetIndex: number;
  onPresetChange: (index: number) => void;
  onSynthParam: (key: string, value: unknown) => void;
}

interface KnobConfig {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  defaultValue: number;
  getValue: (preset: SynthPreset) => number;
  formatValue?: (v: number) => string;
}

const fmtSec = (v: number) => (v >= 1 ? `${v.toFixed(1)}s` : `${Math.round(v * 1000)}ms`);
const fmtHz = (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}Hz`);
const fmtFloat = (v: number) => v.toFixed(2);
const fmtOct = (v: number) => `${v.toFixed(1)} oct`;

function getEnvelope(p: SynthPreset) {
  if (p.engine === 'plucksynth') return null;
  return p.envelope;
}

const ADSR_KNOBS: KnobConfig[] = [
  { key: 'envelope.attack', label: 'Attack', min: 0.001, max: 2, step: 0.001, defaultValue: 0.01, getValue: (p) => getEnvelope(p)?.attack ?? 0.01, formatValue: fmtSec },
  { key: 'envelope.decay', label: 'Decay', min: 0.01, max: 3, step: 0.01, defaultValue: 0.3, getValue: (p) => getEnvelope(p)?.decay ?? 0.3, formatValue: fmtSec },
  { key: 'envelope.sustain', label: 'Sustain', min: 0, max: 1, step: 0.01, defaultValue: 0.5, getValue: (p) => getEnvelope(p)?.sustain ?? 0.5, formatValue: fmtFloat },
  { key: 'envelope.release', label: 'Release', min: 0.01, max: 5, step: 0.01, defaultValue: 0.5, getValue: (p) => getEnvelope(p)?.release ?? 0.5, formatValue: fmtSec },
];

const FM_KNOBS: KnobConfig[] = [
  { key: 'modulationIndex', label: 'Mod Idx', min: 0, max: 30, step: 0.1, defaultValue: 5, getValue: (p) => p.engine === 'fmsynth' ? p.modulationIndex : 5, formatValue: fmtFloat },
  { key: 'harmonicity', label: 'Harm', min: 0.1, max: 10, step: 0.1, defaultValue: 1, getValue: (p) => p.engine === 'fmsynth' ? p.harmonicity : 1, formatValue: fmtFloat },
];

const AM_KNOBS: KnobConfig[] = [
  { key: 'harmonicity', label: 'Harm', min: 0.1, max: 10, step: 0.1, defaultValue: 2, getValue: (p) => p.engine === 'amsynth' ? p.harmonicity : 2, formatValue: fmtFloat },
];

const MONO_FILTER_KNOBS: KnobConfig[] = [
  { key: 'filterEnvelope.baseFrequency', label: 'Base Freq', min: 50, max: 5000, step: 10, defaultValue: 200, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.baseFrequency : 200, formatValue: fmtHz },
  { key: 'filterEnvelope.octaves', label: 'Octaves', min: 0, max: 8, step: 0.1, defaultValue: 3, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.octaves : 3, formatValue: fmtOct },
  { key: 'filterEnvelope.attack', label: 'Filt Atk', min: 0.001, max: 2, step: 0.001, defaultValue: 0.01, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.attack : 0.01, formatValue: fmtSec },
  { key: 'filterEnvelope.decay', label: 'Filt Dec', min: 0.01, max: 3, step: 0.01, defaultValue: 0.2, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.decay : 0.2, formatValue: fmtSec },
  { key: 'filterEnvelope.sustain', label: 'Filt Sus', min: 0, max: 1, step: 0.01, defaultValue: 0.2, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.sustain : 0.2, formatValue: fmtFloat },
  { key: 'filterEnvelope.release', label: 'Filt Rel', min: 0.01, max: 3, step: 0.01, defaultValue: 0.1, getValue: (p) => p.engine === 'monosynth' ? p.filterEnvelope.release : 0.1, formatValue: fmtSec },
];

const PLUCK_KNOBS: KnobConfig[] = [
  { key: 'attackNoise', label: 'Noise', min: 0.1, max: 5, step: 0.1, defaultValue: 1, getValue: (p) => p.engine === 'plucksynth' ? p.attackNoise : 1, formatValue: fmtFloat },
  { key: 'resonance', label: 'Resonance', min: 0.5, max: 0.999, step: 0.001, defaultValue: 0.96, getValue: (p) => p.engine === 'plucksynth' ? p.resonance : 0.96, formatValue: fmtFloat },
  { key: 'dampening', label: 'Dampen', min: 200, max: 8000, step: 50, defaultValue: 3000, getValue: (p) => p.engine === 'plucksynth' ? p.dampening : 3000, formatValue: fmtHz },
  { key: 'release', label: 'Release', min: 0.1, max: 5, step: 0.1, defaultValue: 1, getValue: (p) => p.engine === 'plucksynth' ? p.release : 1, formatValue: fmtSec },
];

const MEMBRANE_KNOBS: KnobConfig[] = [
  { key: 'pitchDecay', label: 'Pitch Dec', min: 0.001, max: 0.5, step: 0.001, defaultValue: 0.05, getValue: (p) => p.engine === 'membranesynth' ? p.pitchDecay : 0.05, formatValue: fmtSec },
  { key: 'octaves', label: 'Octaves', min: 0.5, max: 10, step: 0.5, defaultValue: 6, getValue: (p) => p.engine === 'membranesynth' ? p.octaves : 6, formatValue: fmtOct },
];

function getKnobsForEngine(engine: SynthEngine): KnobConfig[] {
  switch (engine) {
    case 'synth':
      return ADSR_KNOBS;
    case 'fmsynth':
      return [...ADSR_KNOBS, ...FM_KNOBS];
    case 'amsynth':
      return [...ADSR_KNOBS, ...AM_KNOBS];
    case 'monosynth':
      return [...ADSR_KNOBS, ...MONO_FILTER_KNOBS];
    case 'plucksynth':
      return PLUCK_KNOBS;
    case 'membranesynth':
      return [...ADSR_KNOBS, ...MEMBRANE_KNOBS];
  }
}

const ENGINE_LABELS: Record<SynthEngine, string> = {
  synth: 'Basic',
  fmsynth: 'FM',
  amsynth: 'AM',
  monosynth: 'Mono',
  plucksynth: 'Pluck',
  membranesynth: 'Membrane',
};

export function SynthEditor({ presetIndex, onPresetChange, onSynthParam }: SynthEditorProps) {
  const preset = SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];
  const knobs = useMemo(() => getKnobsForEngine(preset.engine), [preset.engine]);
  const selectedCategory = preset.category;

  // Presets with index and category for quick-switch pills
  const categoryPresets = useMemo(() => {
    const result: { index: number; name: string; category: PresetCategory }[] = [];
    SYNTH_PRESETS.forEach((p, i) => {
      result.push({ index: i, name: p.name, category: p.category });
    });
    return result;
  }, []);

  const handleKnobChange = (knobKey: string, value: number) => {
    // Convert dotted key to nested object for .set()
    const parts = knobKey.split('.');
    if (parts.length === 2) {
      onSynthParam(parts[0], { [parts[1]]: value });
    } else {
      onSynthParam(knobKey, value);
    }
  };

  return (
    <div className="border-b border-border bg-card/60 px-4 py-2">
      {/* Category pills + engine badge */}
      <div className="flex items-center gap-2 mb-2">
        <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground/60 mr-1">
          {ENGINE_LABELS[preset.engine]}
        </span>
        <div className="flex items-center gap-1 flex-wrap">
          {PRESET_CATEGORIES.map(({ id, label }) => {
            const count = categoryPresets.filter((p) => p.category === id).length;
            if (count === 0) return null;
            const isActive = id === selectedCategory;
            return (
              <button
                key={id}
                className={`rounded-full px-2.5 py-0.5 text-[10px] font-medium transition-colors ${
                  isActive
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-muted/50 text-muted-foreground hover:bg-muted hover:text-foreground'
                }`}
                onClick={() => {
                  // Find first preset in this category
                  const first = categoryPresets.find((p) => p.category === id);
                  if (first) onPresetChange(first.index);
                }}
              >
                {label}
              </button>
            );
          })}
        </div>
      </div>

      {/* Knobs row */}
      <div className="flex items-start gap-3 overflow-x-auto pb-1">
        {knobs.map((config) => (
          <Knob
            key={config.key}
            value={config.getValue(preset)}
            min={config.min}
            max={config.max}
            step={config.step}
            defaultValue={config.defaultValue}
            label={config.label}
            formatValue={config.formatValue}
            size="sm"
            color="oklch(0.65 0.15 265)"
            onChange={(v) => handleKnobChange(config.key, v)}
          />
        ))}
      </div>
    </div>
  );
}
