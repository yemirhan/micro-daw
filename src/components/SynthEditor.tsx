import { useMemo, useState } from 'react';
import { RotateCcw } from 'lucide-react';
import { Knob } from '@/components/ui/Knob';
import { SYNTH_PRESETS, PRESET_CATEGORIES } from '@/utils/constants';
import { resolvePreset } from '@/utils/synthFactory';
import { getPath, mergeOverrides, pathToObject } from '@/audio/presetParams';
import { audioEngine } from '@/services/AudioEngine';
import type { SynthEngine, SynthPreset } from '@/types/audio';

interface SynthEditorProps {
  presetIndex: number;
  onPresetChange: (index: number) => void;
  /** (dotted param path, value) -> applied live to the running instrument. */
  onSynthParam: (key: string, value: unknown) => void;
}

// ---------------------------------------------------------------------------
// Knob definitions
// ---------------------------------------------------------------------------

interface KnobDef {
  kind?: 'knob';
  path: string;
  label: string;
  min: number;
  max: number;
  step: number;
  fmt?: (v: number) => string;
  /** Log-scale knob (frequencies): the knob moves in log2 space. */
  log?: boolean;
}

interface ChoiceDef {
  kind: 'choice';
  path: string;
  label: string;
  options: { value: string | number; label: string }[];
}

type Control = KnobDef | ChoiceDef;

interface Section {
  title: string;
  controls: Control[];
}

const fmtSec = (v: number) => (v >= 1 ? `${v.toFixed(2)}s` : `${Math.round(v * 1000)}ms`);
const fmtHz = (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(1)}k` : `${Math.round(v)}Hz`);
const fmtPct = (v: number) => `${Math.round(v * 100)}%`;
const fmtF2 = (v: number) => v.toFixed(2);
const fmtF1 = (v: number) => v.toFixed(1);
const fmtInt = (v: number) => `${Math.round(v)}`;
const fmtSemi = (v: number) => `${v > 0 ? '+' : ''}${Math.round(v)}st`;
const fmtCents = (v: number) => `${v > 0 ? '+' : ''}${Math.round(v)}c`;
const fmtOct = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}oct`;
const fmtDb = (v: number) => `${v > 0 ? '+' : ''}${v.toFixed(1)}dB`;

const WAVES: ChoiceDef['options'] = [
  { value: 'sine', label: 'Sin' },
  { value: 'triangle', label: 'Tri' },
  { value: 'sawtooth', label: 'Saw' },
  { value: 'square', label: 'Sqr' },
  { value: 'pulse', label: 'Pls' },
];

const adsr = (prefix: string, maxAttack = 4, maxRelease = 6): KnobDef[] => [
  { path: `${prefix}.attack`, label: 'Attack', min: 0.001, max: maxAttack, step: 0.001, fmt: fmtSec },
  { path: `${prefix}.decay`, label: 'Decay', min: 0.01, max: 6, step: 0.01, fmt: fmtSec },
  { path: `${prefix}.sustain`, label: 'Sustain', min: 0, max: 1, step: 0.01, fmt: fmtPct },
  { path: `${prefix}.release`, label: 'Release', min: 0.01, max: maxRelease, step: 0.01, fmt: fmtSec },
];

const characterSection = (extra: KnobDef[] = []): Section => ({
  title: 'Character',
  controls: [
    { path: 'character.drive', label: 'Drive', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    { path: 'character.chorus', label: 'Chorus', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    { path: 'character.presence', label: 'Presence', min: -12, max: 12, step: 0.5, fmt: fmtDb },
    ...extra,
    { path: 'gain', label: 'Level', min: -18, max: 12, step: 0.5, fmt: fmtDb },
  ],
});

const SUBTRACTIVE: Section[] = [
  {
    title: 'Osc A',
    controls: [
      { kind: 'choice', path: 'oscA.wave', label: 'Wave', options: WAVES },
      { path: 'oscA.level', label: 'Level', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'oscA.unison', label: 'Unison', min: 1, max: 7, step: 1, fmt: fmtInt },
      { path: 'oscA.spread', label: 'Detune', min: 0, max: 60, step: 1, fmt: fmtCents },
    ],
  },
  {
    title: 'Osc B',
    controls: [
      { kind: 'choice', path: 'oscB.wave', label: 'Wave', options: WAVES },
      { path: 'oscB.level', label: 'Level', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'oscB.semi', label: 'Semi', min: -24, max: 24, step: 1, fmt: fmtSemi },
      { path: 'oscB.fine', label: 'Fine', min: -50, max: 50, step: 1, fmt: fmtCents },
      { path: 'sub.level', label: 'Sub', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'noise.level', label: 'Noise', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    ],
  },
  {
    title: 'Filter',
    controls: [
      {
        kind: 'choice',
        path: 'filter.type',
        label: 'Type',
        options: [
          { value: 'lowpass', label: 'LP' },
          { value: 'highpass', label: 'HP' },
          { value: 'bandpass', label: 'BP' },
        ],
      },
      { path: 'filter.cutoff', label: 'Cutoff', min: 40, max: 18000, step: 0.01, fmt: fmtHz, log: true },
      { path: 'filter.resonance', label: 'Reso', min: 0, max: 20, step: 0.1, fmt: fmtF1 },
      { path: 'filter.envAmount', label: 'Env Amt', min: -4, max: 8, step: 0.1, fmt: fmtOct },
      { path: 'filter.keyTrack', label: 'Key Trk', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'filter.velocity', label: 'Vel', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    ],
  },
  { title: 'Filter Env', controls: adsr('filterEnv') },
  {
    title: 'Amp',
    controls: [...adsr('amp'), { path: 'amp.velocity', label: 'Vel', min: 0, max: 1, step: 0.01, fmt: fmtPct }],
  },
  {
    title: 'Mod',
    controls: [
      { path: 'glide', label: 'Glide', min: 0, max: 1, step: 0.005, fmt: fmtSec },
      { path: 'lfo.rate', label: 'LFO Rate', min: 0.05, max: 20, step: 0.05, fmt: (v) => `${v.toFixed(1)}Hz` },
      { path: 'lfo.vibrato', label: 'Vibrato', min: 0, max: 100, step: 1, fmt: fmtCents },
      { path: 'lfo.filter', label: 'LFO>Filt', min: 0, max: 4800, step: 10, fmt: fmtCents },
    ],
  },
  characterSection(),
];

const FM: Section[] = [
  {
    title: 'FM',
    controls: [
      {
        kind: 'choice',
        path: 'carrier.wave',
        label: 'Carrier',
        options: WAVES.filter((w) => w.value !== 'pulse'),
      },
      { path: 'ratio', label: 'Ratio', min: 0.25, max: 14, step: 0.01, fmt: fmtF2 },
      { path: 'index', label: 'Index', min: 0, max: 12, step: 0.05, fmt: fmtF2 },
      { path: 'velocityToIndex', label: 'Vel>Idx', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'keyScale', label: 'Key Scl', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'detune', label: 'Detune', min: -30, max: 30, step: 1, fmt: fmtCents },
    ],
  },
  { title: 'Mod Env', controls: adsr('modEnv') },
  {
    title: 'Tine',
    controls: [
      { path: 'ping.level', label: 'Level', min: 0, max: 1, step: 0.01, fmt: fmtPct },
      { path: 'ping.ratio', label: 'Ratio', min: 0.5, max: 16, step: 0.01, fmt: fmtF2 },
      { path: 'ping.decay', label: 'Decay', min: 0.005, max: 2, step: 0.005, fmt: fmtSec },
    ],
  },
  {
    title: 'Amp',
    controls: [...adsr('amp'), { path: 'amp.velocity', label: 'Vel', min: 0, max: 1, step: 0.01, fmt: fmtPct }],
  },
  {
    title: 'Vibrato',
    controls: [
      { path: 'lfo.rate', label: 'Rate', min: 0.05, max: 20, step: 0.05, fmt: (v) => `${v.toFixed(1)}Hz` },
      { path: 'lfo.vibrato', label: 'Depth', min: 0, max: 100, step: 1, fmt: fmtCents },
    ],
  },
  characterSection([
    { path: 'character.tremolo', label: 'Tremolo', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    { path: 'character.tremoloRate', label: 'Trem Hz', min: 0.5, max: 12, step: 0.1, fmt: fmtF1 },
  ]),
];

const PLUCK: Section[] = [
  {
    title: 'String',
    controls: [
      { path: 'attackNoise', label: 'Pick', min: 0.1, max: 8, step: 0.1, fmt: fmtF1 },
      { path: 'dampening', label: 'Bright', min: 300, max: 12000, step: 0.01, fmt: fmtHz, log: true },
      { path: 'resonance', label: 'Sustain', min: 0.8, max: 0.999, step: 0.001, fmt: (v) => v.toFixed(3) },
      { path: 'release', label: 'Release', min: 0.05, max: 5, step: 0.01, fmt: fmtSec },
      { path: 'velocity', label: 'Vel', min: 0, max: 1, step: 0.01, fmt: fmtPct },
    ],
  },
  characterSection([{ path: 'character.tone', label: 'Tone', min: 500, max: 20000, step: 0.01, fmt: fmtHz, log: true }]),
];

const SAMPLER: Section[] = [
  {
    title: 'Piano',
    controls: [
      { path: 'release', label: 'Release', min: 0.05, max: 3, step: 0.01, fmt: fmtSec },
      { path: 'velocityCurve', label: 'Touch', min: 0.6, max: 2.5, step: 0.05, fmt: fmtF2 },
    ],
  },
  characterSection([{ path: 'character.tone', label: 'Tone', min: 500, max: 20000, step: 0.01, fmt: fmtHz, log: true }]),
];

const SECTIONS: Record<SynthEngine, Section[]> = {
  subtractive: SUBTRACTIVE,
  fm: FM,
  pluck: PLUCK,
  sampler: SAMPLER,
};

const ENGINE_LABELS: Record<SynthEngine, string> = {
  subtractive: 'Analog',
  fm: 'FM',
  pluck: 'String',
  sampler: 'Sampled',
};

const ACCENT = 'var(--primary)';

function numberAt(preset: SynthPreset, path: string, fallback: number): number {
  const v = getPath(preset, path);
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

const NO_OVERRIDES: Record<string, unknown> = {};

function initialOverrides(presetIndex: number): Record<string, unknown> {
  return audioEngine.getPresetIndex() === presetIndex ? audioEngine.getSynthOverrides() : {};
}

export function SynthEditor({ presetIndex, onPresetChange, onSynthParam }: SynthEditorProps) {
  // Local mirror of the live edits so knobs follow what you hear. Reset when the preset changes.
  const [edits, setEdits] = useState(() => ({ presetIndex, overrides: initialOverrides(presetIndex) }));
  const overrides = edits.presetIndex === presetIndex ? edits.overrides : NO_OVERRIDES;
  if (edits.presetIndex !== presetIndex) setEdits({ presetIndex, overrides: initialOverrides(presetIndex) });

  const base = SYNTH_PRESETS[presetIndex] ?? SYNTH_PRESETS[0];
  const preset = useMemo(() => resolvePreset(presetIndex, overrides), [presetIndex, overrides]);
  const sections = SECTIONS[base.engine];
  const isMono = preset.engine === 'subtractive' && preset.mono;

  const categoryPresets = useMemo(
    () => SYNTH_PRESETS.map((p, index) => ({ index, name: p.name, category: p.category })),
    [],
  );
  const inCategory = categoryPresets.filter((p) => p.category === base.category);

  const change = (path: string, value: unknown) => {
    setEdits((prev) => ({
      presetIndex,
      overrides: mergeOverrides(prev.presetIndex === presetIndex ? prev.overrides : {}, pathToObject(path, value)),
    }));
    onSynthParam(path, value);
  };

  const reset = () => {
    setEdits({ presetIndex, overrides: {} });
    onPresetChange(presetIndex);
  };

  const edited = Object.keys(overrides).length > 0;

  return (
    <div className="border-b border-hairline bg-surface-1 px-3 pt-2.5 pb-3" data-testid="synth-editor">
      {/* Header: engine, categories, presets in category */}
      <div className="mb-2 flex items-center gap-2">
        <span className="daw-label">Sound</span>
        <span className="mr-1 rounded-sm bg-surface-3 px-1.5 py-0.5 font-mono text-[10px] font-semibold uppercase tracking-[0.06em] text-foreground/80">
          {ENGINE_LABELS[base.engine]}
          {isMono ? ' · Mono' : ''}
        </span>
        <div className="flex flex-wrap items-center gap-1">
          {PRESET_CATEGORIES.map(({ id, label }) => {
            const first = categoryPresets.find((p) => p.category === id);
            if (!first) return null;
            const isActive = id === base.category;
            return (
              <button
                key={id}
                className={`h-6 rounded-sm px-2 text-[11px] font-semibold transition-colors duration-150 ${
                  isActive
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-surface-3 text-muted-foreground hover:bg-surface-4 hover:text-foreground'
                }`}
                onClick={() => onPresetChange(first.index)}
              >
                {label}
              </button>
            );
          })}
        </div>
        <button
          className="ml-auto flex h-7 items-center gap-1.5 rounded-md px-2 text-[12px] font-medium text-muted-foreground transition-colors duration-150 hover:bg-surface-3 hover:text-foreground disabled:opacity-40"
          onClick={reset}
          disabled={!edited}
          title="Reset to the preset's original sound"
        >
          <RotateCcw className="size-3.5" /> {edited ? 'Reset edits' : 'Reset'}
        </button>
      </div>

      <div className="mb-2.5 flex flex-wrap items-center gap-0.5">
        {inCategory.map((p) => (
          <button
            key={p.index}
            className={`h-6 rounded-md px-2 text-[12px] transition-colors duration-150 ${
              p.index === presetIndex
                ? 'bg-surface-3 font-semibold text-foreground shadow-[inset_0_0_0_1px_var(--primary)]'
                : 'text-muted-foreground hover:bg-surface-2 hover:text-foreground'
            }`}
            onClick={() => onPresetChange(p.index)}
          >
            {p.name}
          </button>
        ))}
        {base.description && (
          <span className="ml-2 truncate text-[11px] text-muted-foreground">{base.description}</span>
        )}
      </div>

      {/* Parameter sections */}
      <div className="flex items-stretch gap-2 overflow-x-auto pb-0.5">
        {sections.map((section) => (
          <div key={section.title} className="flex shrink-0 flex-col gap-2 rounded-lg border border-border bg-surface-2 px-2.5 pt-1.5 pb-2">
            <span className="daw-label">{section.title}</span>
            <div className="flex items-start gap-2.5">
              {section.controls.map((control) =>
                control.kind === 'choice' ? (
                  <ChoiceControl
                    key={control.path}
                    def={control}
                    value={getPath(preset, control.path) as string | number | undefined}
                    onChange={(v) => change(control.path, v)}
                  />
                ) : (
                  <KnobControl
                    key={control.path}
                    def={control}
                    value={numberAt(preset, control.path, control.min)}
                    defaultValue={numberAt(base, control.path, control.min)}
                    onChange={(v) => change(control.path, v)}
                  />
                ),
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function KnobControl({
  def,
  value,
  defaultValue,
  onChange,
}: {
  def: KnobDef;
  value: number;
  defaultValue: number;
  onChange: (v: number) => void;
}) {
  if (def.log) {
    const lo = Math.log2(def.min);
    const hi = Math.log2(def.max);
    const toKnob = (hz: number) => Math.log2(Math.min(def.max, Math.max(def.min, hz)));
    return (
      <Knob
        value={toKnob(value)}
        min={lo}
        max={hi}
        step={0.01}
        defaultValue={toKnob(defaultValue)}
        label={def.label}
        formatValue={(v) => (def.fmt ?? fmtF2)(Math.pow(2, v))}
        size="sm"
        color={ACCENT}
        onChange={(v) => onChange(Math.round(Math.pow(2, v)))}
      />
    );
  }
  return (
    <Knob
      value={value}
      min={def.min}
      max={def.max}
      step={def.step}
      defaultValue={defaultValue}
      label={def.label}
      formatValue={def.fmt}
      size="sm"
      color={ACCENT}
      onChange={onChange}
    />
  );
}

function ChoiceControl({
  def,
  value,
  onChange,
}: {
  def: ChoiceDef;
  value: string | number | undefined;
  onChange: (v: string | number) => void;
}) {
  return (
    <div className="flex flex-col items-center gap-1">
      <div className="flex flex-col gap-0.5 rounded-md border border-border bg-surface-0 p-0.5">
        {def.options.map((opt) => (
          <button
            key={String(opt.value)}
            className={`rounded-[3px] px-1.5 font-mono text-[9.5px] font-semibold leading-4 transition-colors duration-100 ${
              value === opt.value ? 'bg-primary text-primary-foreground' : 'text-muted-foreground hover:bg-surface-3 hover:text-foreground'
            }`}
            onClick={() => onChange(opt.value)}
          >
            {opt.label}
          </button>
        ))}
      </div>
      <span className="text-[9.5px] font-bold uppercase tracking-[0.08em] text-muted-foreground">{def.label}</span>
    </div>
  );
}
