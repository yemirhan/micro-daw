import { useState, useRef, useEffect, useMemo } from 'react';
import { Trash2, SlidersHorizontal, Activity, FileAudio, ChevronDown, Drum } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Tip } from '@/components/ui/tip';
import { TrackToggle } from './TrackToggles';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';
import type { Track, TrackInstrument, AutomationParameter } from '@/types/arrangement';
import { SYNTH_PRESETS, PRESET_VIBES, MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';
import { getAutomationParameterLabel } from '@/utils/automationHelpers';
import type { PresetVibe } from '@/types/audio';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover';
import {
  Command,
  CommandInput,
  CommandList,
  CommandEmpty,
  CommandGroup,
  CommandItem,
} from '@/components/ui/command';

const ALL_AUTOMATION_PARAMS: AutomationParameter[] = [
  'volume', 'pan', 'reverbWet', 'delayWet', 'chorusDepth',
  'distortionWet', 'filterCutoff', 'eqLow', 'eqMid', 'eqHigh',
];

interface TrackHeaderProps {
  track: Track;
  isArmed: boolean;
  isRecording: boolean;
  onMuteToggle: () => void;
  onSoloToggle: () => void;
  onVolumeChange: (db: number) => void;
  onPanChange?: (pan: number) => void;
  onInstrumentChange: (instrument: TrackInstrument) => void;
  onDelete: () => void;
  onArmToggle: () => void;
  onFxToggle?: () => void;
  fxOpen?: boolean;
  onAddAutomationLane?: (parameter: AutomationParameter) => void;
  onRemoveAutomationLane?: (parameter: AutomationParameter) => void;
  onToggleAutomationLaneVisibility?: (parameter: AutomationParameter) => void;
}

export function TrackHeader({
  track,
  isArmed,
  isRecording,
  onMuteToggle,
  onSoloToggle,
  onVolumeChange,
  onPanChange,
  onInstrumentChange,
  onDelete,
  onArmToggle,
  onFxToggle,
  fxOpen,
  onAddAutomationLane,
  onRemoveAutomationLane,
  onToggleAutomationLaneVisibility: _onToggleAutomationLaneVisibility,
}: TrackHeaderProps) {
  const [instrumentOpen, setInstrumentOpen] = useState(false);
  const [vibeFilter, setVibeFilter] = useState<PresetVibe | null>(null);
  const [automationMenuOpen, setAutomationMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number }>({ top: 0, left: 0 });

  const vibeGroups = useMemo(() => {
    const groups = new Map<PresetVibe, { index: number; name: string; category: string }[]>();
    SYNTH_PRESETS.forEach((preset, i) => {
      if (!groups.has(preset.vibe)) groups.set(preset.vibe, []);
      groups.get(preset.vibe)!.push({ index: i, name: preset.name, category: preset.category });
    });
    return groups;
  }, []);

  const currentPresetName =
    track.instrument.type === 'drums'
      ? 'Drums'
      : (SYNTH_PRESETS[track.instrument.presetIndex]?.name ?? 'Unknown');

  const existingLanes = track.automation ?? [];
  const hasVisibleLanes = existingLanes.some((l) => l.visible);

  // Close menu on outside click
  useEffect(() => {
    if (!automationMenuOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setAutomationMenuOpen(false);
      }
    };
    window.addEventListener('pointerdown', handleClick);
    return () => window.removeEventListener('pointerdown', handleClick);
  }, [automationMenuOpen]);

  const toggleAutomationMenu = () => {
    if (!automationMenuOpen && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      setMenuPos({ top: rect.bottom + 4, left: rect.left });
    }
    setAutomationMenuOpen((v) => !v);
  };

  const handleAutomationParamClick = (param: AutomationParameter) => {
    const existing = existingLanes.find((l) => l.parameter === param);
    if (existing) {
      // Lane exists — remove it entirely (clears automation data + resets audio)
      onRemoveAutomationLane?.(param);
    } else {
      // Lane doesn't exist — add it (visible by default)
      onAddAutomationLane?.(param);
    }
  };

  const pan = track.pan ?? 0;
  const panLabel = pan > 0.005 ? `R${Math.round(pan * 100)}` : pan < -0.005 ? `L${Math.round(Math.abs(pan) * 100)}` : 'C';
  const isAudio = track.instrument.type === 'audio';

  return (
    <div
      className={cn(
        'group/track relative flex h-16 shrink-0 flex-col justify-center gap-1 border-b border-border bg-surface-1 pr-2 pl-3.5',
        'transition-colors duration-150 hover:bg-[color-mix(in_oklab,var(--surface-1)_80%,var(--surface-2))]',
        fxOpen && 'bg-surface-2',
      )}
    >
      {/* Colour strip */}
      <span aria-hidden className="absolute inset-y-0 left-0 w-1" style={{ backgroundColor: track.color, opacity: track.muted ? 0.35 : 1 }} />

      {/* Row 1: name + actions */}
      <div className="flex h-5 items-center gap-1">
        <span
          className={cn('min-w-0 flex-1 truncate text-[12px] font-semibold', track.muted ? 'text-muted-foreground' : 'text-foreground')}
          title={track.name}
        >
          {track.name}
        </span>

        {onFxToggle && (
          <Tip label="Track effects">
            <Button
              variant="ghost"
              size="icon-xs"
              className={cn('size-5', fxOpen && 'bg-primary/15 text-primary hover:bg-primary/20 hover:text-primary')}
              onClick={onFxToggle}
              aria-label="Track effects"
              aria-pressed={!!fxOpen}
            >
              <SlidersHorizontal className="size-3" />
            </Button>
          </Tip>
        )}

        {/* Automation toggle */}
        {onAddAutomationLane && (
          <div className="relative">
            <Tip label="Automation lanes" disabled={automationMenuOpen}>
              <Button
                ref={buttonRef}
                variant="ghost"
                size="icon-xs"
                className={cn('size-5', (hasVisibleLanes || automationMenuOpen) && 'text-primary hover:text-primary')}
                onClick={toggleAutomationMenu}
                aria-label="Automation lanes"
                aria-expanded={automationMenuOpen}
              >
                <Activity className="size-3" />
              </Button>
            </Tip>

            {automationMenuOpen && (
              <div
                ref={menuRef}
                className="fixed z-50 min-w-[168px] rounded-lg border border-border bg-popover p-1 shadow-[0_16px_40px_-10px_oklch(0_0_0/0.65)] animate-fade-in"
                style={{ top: menuPos.top, left: menuPos.left }}
                role="menu"
              >
                <div className="daw-label px-2 pt-1 pb-1.5">Automate</div>
                {ALL_AUTOMATION_PARAMS.map((param) => {
                  const lane = existingLanes.find((l) => l.parameter === param);
                  const isVisible = lane?.visible ?? false;
                  return (
                    <button
                      key={param}
                      role="menuitemcheckbox"
                      aria-checked={isVisible}
                      className={cn(
                        'flex h-7 w-full items-center gap-2 rounded-md px-2 text-[12px] transition-colors hover:bg-surface-3',
                        isVisible ? 'font-semibold text-foreground' : 'text-muted-foreground hover:text-foreground',
                      )}
                      onClick={() => handleAutomationParamClick(param)}
                    >
                      <span
                        className={cn(
                          'size-2 rounded-[2px] border',
                          isVisible ? 'border-primary bg-primary' : 'border-muted-foreground/50 bg-transparent',
                        )}
                      />
                      {getAutomationParameterLabel(param)}
                    </button>
                  );
                })}
                {existingLanes.length > 0 && (
                  <>
                    <div className="my-1 h-px bg-border" />
                    <button
                      className="flex h-7 w-full items-center gap-2 rounded-md px-2 text-[12px] text-muted-foreground transition-colors hover:bg-surface-3 hover:text-signal-rec"
                      onClick={() => {
                        existingLanes.forEach((l) => onRemoveAutomationLane?.(l.parameter));
                        setAutomationMenuOpen(false);
                      }}
                    >
                      Clear all
                    </button>
                  </>
                )}
              </div>
            )}
          </div>
        )}

        <Tip label="Delete track">
          <Button
            variant="ghost"
            size="icon-xs"
            className="size-5 opacity-0 transition-opacity group-hover/track:opacity-100 focus-visible:opacity-100 hover:text-signal-rec"
            onClick={onDelete}
            aria-label="Delete track"
          >
            <Trash2 className="size-3" />
          </Button>
        </Tip>
      </div>

      {/* Row 2: instrument */}
      <div className="flex h-4 items-center">
        {isAudio ? (
          <span className="flex min-w-0 items-center gap-1 text-[11px] text-muted-foreground">
            <FileAudio className="size-3 shrink-0" />
            <span className="truncate">Audio</span>
          </span>
        ) : (
          <Popover open={instrumentOpen} onOpenChange={setInstrumentOpen}>
            <PopoverTrigger asChild>
              <button
                className="-ml-1 flex h-4 max-w-full min-w-0 items-center gap-1 rounded-[3px] px-1 text-[11px] text-muted-foreground transition-colors hover:bg-surface-3 hover:text-foreground data-[state=open]:bg-surface-3 data-[state=open]:text-foreground"
                title="Change instrument"
              >
                {track.instrument.type === 'drums' && <Drum className="size-3 shrink-0" />}
                <span className="truncate">{currentPresetName}</span>
                <ChevronDown className="size-3 shrink-0 opacity-60" />
              </button>
            </PopoverTrigger>
            <PopoverContent className="w-[300px] p-0" align="start">
              <Command>
                <CommandInput placeholder="Search sounds…" className="h-10 text-[13px]" />
                <div className="flex gap-1 overflow-x-auto border-b border-border px-2 py-2" style={{ scrollbarWidth: 'none' }}>
                  <button
                    className={cn(
                      'h-6 shrink-0 rounded-sm px-2 text-[11px] font-semibold transition-colors',
                      vibeFilter === null ? 'bg-primary text-primary-foreground' : 'bg-surface-3 text-muted-foreground hover:bg-surface-4 hover:text-foreground',
                    )}
                    onClick={() => setVibeFilter(null)}
                  >
                    All
                  </button>
                  {PRESET_VIBES.map(({ id, label }) => {
                    const count = vibeGroups.get(id)?.length ?? 0;
                    if (count === 0) return null;
                    return (
                      <button
                        key={id}
                        className={cn(
                          'h-6 shrink-0 rounded-sm px-2 text-[11px] font-semibold transition-colors',
                          vibeFilter === id ? 'bg-primary text-primary-foreground' : 'bg-surface-3 text-muted-foreground hover:bg-surface-4 hover:text-foreground',
                        )}
                        onClick={() => setVibeFilter(vibeFilter === id ? null : id)}
                      >
                        {label}
                      </button>
                    );
                  })}
                </div>
                <CommandList className="max-h-[300px]">
                  <CommandEmpty>No sounds found.</CommandEmpty>
                  <CommandGroup heading="Drums">
                    <CommandItem
                      value="drums drum kit"
                      onSelect={() => {
                        onInstrumentChange({ type: 'drums', presetIndex: 0 });
                        setInstrumentOpen(false);
                      }}
                      className="flex items-center justify-between"
                    >
                      <span className={track.instrument.type === 'drums' ? 'font-semibold text-primary' : ''}>Drum kit</span>
                      <span className="text-[10.5px] text-muted-foreground">percussion</span>
                    </CommandItem>
                  </CommandGroup>
                  {PRESET_VIBES.map(({ id, label }) => {
                    const presets = vibeGroups.get(id);
                    if (!presets || presets.length === 0) return null;
                    if (vibeFilter !== null && vibeFilter !== id) return null;
                    return (
                      <CommandGroup key={id} heading={label} className="[&:not(:first-child)]:border-t [&:not(:first-child)]:border-border/60">
                        {presets.map(({ index, name, category }) => {
                          const current = track.instrument.type === 'synth' && track.instrument.presetIndex === index;
                          return (
                            <CommandItem
                              key={index}
                              value={`${name} ${category} ${id}`}
                              onSelect={() => {
                                onInstrumentChange({ type: 'synth', presetIndex: index });
                                setInstrumentOpen(false);
                              }}
                              className="flex items-center justify-between"
                            >
                              <span className={current ? 'font-semibold text-primary' : ''}>{name}</span>
                              <span className="text-[10.5px] capitalize text-muted-foreground">{category}</span>
                            </CommandItem>
                          );
                        })}
                      </CommandGroup>
                    );
                  })}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        )}
      </div>

      {/* Row 3: arm / mute / solo + volume + pan */}
      <div className="flex h-[18px] items-center gap-1">
        {!isAudio && (
          <TrackToggle
            kind="arm"
            active={isArmed || isRecording}
            pulsing={isRecording}
            onClick={onArmToggle}
            title={isArmed ? 'Disarm track' : 'Arm track for recording'}
          />
        )}
        <TrackToggle kind="mute" active={track.muted} onClick={onMuteToggle} title={track.muted ? 'Unmute track' : 'Mute track'} />
        <TrackToggle kind="solo" active={track.solo} onClick={onSoloToggle} title={track.solo ? 'Unsolo track' : 'Solo track'} />
        <Slider
          min={MIN_VOLUME}
          max={MAX_VOLUME}
          step={1}
          value={[track.volume]}
          onValueChange={([v]) => onVolumeChange(v)}
          className="ml-1 min-w-0 flex-1"
          aria-label="Track volume"
          title={`Volume ${track.volume} dB`}
        />
        <span className="w-[30px] text-right font-mono text-[10px] text-muted-foreground tabular-nums" title="Volume (dB)">
          {track.volume > 0 ? `+${track.volume}` : track.volume}
        </span>
        {onPanChange && (
          <PanDial value={pan} label={panLabel} onChange={onPanChange} />
        )}
      </div>
    </div>
  );
}

/** Tiny bipolar pan control: drag vertically or use arrow keys; double-click to centre. */
function PanDial({ value, label, onChange }: { value: number; label: string; onChange: (pan: number) => void }) {
  const drag = useRef<{ y: number; start: number } | null>(null);
  const clamp = (v: number) => Math.max(-1, Math.min(1, Math.round(v * 100) / 100));
  const angle = value * 135;
  return (
    <svg
      viewBox="0 0 18 18"
      className="size-[18px] shrink-0 cursor-ns-resize rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring/70"
      role="slider"
      tabIndex={0}
      aria-label="Pan"
      aria-valuemin={-1}
      aria-valuemax={1}
      aria-valuenow={value}
      aria-valuetext={label}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        e.preventDefault();
        e.stopPropagation();
        drag.current = { y: e.clientY, start: value };
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        if (!drag.current) return;
        onChange(clamp(drag.current.start + (drag.current.y - e.clientY) / 80));
      }}
      onPointerUp={() => {
        drag.current = null;
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onDoubleClick={() => onChange(0)}
      onKeyDown={(e) => {
        if (e.key === 'ArrowUp' || e.key === 'ArrowRight') { e.preventDefault(); onChange(clamp(value + 0.05)); }
        if (e.key === 'ArrowDown' || e.key === 'ArrowLeft') { e.preventDefault(); onChange(clamp(value - 0.05)); }
      }}
    >
      <title>{`Pan ${label}`}</title>
      <circle cx="9" cy="9" r="7.5" fill="var(--surface-3)" stroke="var(--border)" />
      <line
        x1="9"
        y1="9"
        x2="9"
        y2="2.5"
        stroke={Math.abs(value) > 0.005 ? 'var(--primary)' : 'var(--foreground)'}
        strokeWidth="1.75"
        strokeLinecap="round"
        transform={`rotate(${angle} 9 9)`}
      />
    </svg>
  );
}
