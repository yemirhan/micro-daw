import { useState, useRef, useEffect, useMemo } from 'react';
import { Volume2, VolumeX, Trash2, SlidersHorizontal, Activity, FileAudio, ChevronDown } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';
import type { Track, TrackInstrument, AutomationParameter, AutomationLane } from '@/types/arrangement';
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
  onToggleAutomationLaneVisibility,
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

  return (
    <div
      className="flex h-16 shrink-0 flex-col justify-center border-b border-border px-2"
      style={{
        borderLeft: `3px solid ${track.color}`,
        boxShadow: `inset 4px 0 12px -4px ${track.color}33`,
      }}
    >
      <div className="flex items-center gap-1">
        {track.instrument.type !== 'audio' && (
          <button
            className={cn(
              'h-4 w-4 rounded-full border-2 transition-colors',
              isRecording
                ? 'border-red-500 bg-red-500 animate-pulse'
                : isArmed
                  ? 'border-red-500 bg-red-500'
                  : 'border-muted-foreground/50 bg-transparent',
            )}
            onClick={onArmToggle}
            title={isArmed ? 'Disarm track' : 'Arm track for recording'}
          />
        )}
        {track.instrument.type === 'audio' ? (
          <div className="flex items-center gap-1 flex-1 min-w-0 px-1">
            <FileAudio className="h-3 w-3 shrink-0 text-muted-foreground" />
            <span className="truncate text-[11px] font-semibold">{track.name}</span>
          </div>
        ) : (
          <Popover open={instrumentOpen} onOpenChange={setInstrumentOpen}>
            <PopoverTrigger asChild>
              <button className="flex h-5 flex-1 items-center justify-between min-w-0 rounded px-1 text-[11px] font-semibold hover:bg-accent/50 transition-colors">
                <span className="truncate">{currentPresetName}</span>
                <ChevronDown className="ml-0.5 h-2.5 w-2.5 shrink-0 opacity-50" />
              </button>
            </PopoverTrigger>
            <PopoverContent className="w-[300px] p-0" align="start">
              <Command>
                <CommandInput placeholder="Search sounds..." className="h-8 text-xs" />
                <div
                  className="flex gap-1.5 px-2 py-2 border-b border-border/50 overflow-x-auto"
                  style={{ scrollbarWidth: 'none', WebkitOverflowScrolling: 'touch' }}
                >
                  <button
                    className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${
                      vibeFilter === null
                        ? 'bg-primary text-primary-foreground shadow-sm'
                        : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
                    }`}
                    onClick={() => setVibeFilter(null)}
                  >
                    All
                  </button>
                  {PRESET_VIBES.map(({ id, label, emoji }) => {
                    const count = vibeGroups.get(id)?.length ?? 0;
                    if (count === 0) return null;
                    return (
                      <button
                        key={id}
                        className={`shrink-0 rounded-full px-2.5 py-1 text-[11px] font-medium transition-colors ${
                          vibeFilter === id
                            ? 'bg-primary text-primary-foreground shadow-sm'
                            : 'bg-muted/60 text-muted-foreground hover:bg-muted hover:text-foreground'
                        }`}
                        onClick={() => setVibeFilter(vibeFilter === id ? null : id)}
                      >
                        {emoji} {label}
                      </button>
                    );
                  })}
                </div>
                <CommandList className="max-h-[280px]">
                  <CommandEmpty>No sounds found.</CommandEmpty>
                  <CommandGroup heading="🥁 Drums" className="[&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground/80 [&_[cmdk-group-heading]]:pt-2.5 [&_[cmdk-group-heading]]:pb-1">
                    <CommandItem
                      value="drums drum kit"
                      onSelect={() => {
                        onInstrumentChange({ type: 'drums', presetIndex: 0 });
                        setInstrumentOpen(false);
                      }}
                      className="flex items-center justify-between"
                    >
                      <span className={track.instrument.type === 'drums' ? 'font-semibold' : ''}>Drums</span>
                      <span className="text-[10px] text-muted-foreground/60 uppercase">percussion</span>
                    </CommandItem>
                  </CommandGroup>
                  {PRESET_VIBES.map(({ id, label, emoji }) => {
                    const presets = vibeGroups.get(id);
                    if (!presets || presets.length === 0) return null;
                    if (vibeFilter !== null && vibeFilter !== id) return null;
                    return (
                      <CommandGroup
                        key={id}
                        heading={`${emoji} ${label}`}
                        className="[&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-muted-foreground/80 [&_[cmdk-group-heading]]:pt-2.5 [&_[cmdk-group-heading]]:pb-1 [&:not(:first-child)]:border-t [&:not(:first-child)]:border-border/30"
                      >
                        {presets.map(({ index, name, category }) => (
                          <CommandItem
                            key={index}
                            value={`${name} ${category} ${id}`}
                            onSelect={() => {
                              onInstrumentChange({ type: 'synth', presetIndex: index });
                              setInstrumentOpen(false);
                            }}
                            className="flex items-center justify-between"
                          >
                            <span className={track.instrument.type === 'synth' && track.instrument.presetIndex === index ? 'font-semibold' : ''}>{name}</span>
                            <span className="text-[10px] text-muted-foreground/60 uppercase">{category}</span>
                          </CommandItem>
                        ))}
                      </CommandGroup>
                    );
                  })}
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
        )}
        {onFxToggle && (
          <Button
            variant="ghost"
            size="sm"
            className={cn('h-5 w-5 p-0', fxOpen && 'text-primary')}
            onClick={onFxToggle}
            title="Track Effects"
          >
            <SlidersHorizontal className="h-3 w-3" />
          </Button>
        )}

        {/* Automation toggle */}
        {onAddAutomationLane && (
          <div className="relative">
            <Button
              ref={buttonRef}
              variant="ghost"
              size="sm"
              className={cn('h-5 w-5 p-0', hasVisibleLanes && 'text-primary')}
              onClick={toggleAutomationMenu}
              title="Automation Lanes"
            >
              <Activity className="h-3 w-3" />
            </Button>

            {automationMenuOpen && (
              <div
                ref={menuRef}
                className="fixed z-50 min-w-[140px] rounded-md border border-border bg-popover p-1 shadow-lg"
                style={{ top: menuPos.top, left: menuPos.left }}
              >
                {ALL_AUTOMATION_PARAMS.map((param) => {
                  const lane = existingLanes.find((l) => l.parameter === param);
                  const isVisible = lane?.visible ?? false;
                  return (
                    <button
                      key={param}
                      className={cn(
                        'flex w-full items-center gap-2 rounded-sm px-2 py-1 text-[11px] hover:bg-accent transition-colors',
                        isVisible && 'text-primary font-semibold',
                      )}
                      onClick={() => handleAutomationParamClick(param)}
                    >
                      <span
                        className={cn(
                          'h-2 w-2 rounded-full border',
                          isVisible
                            ? 'border-primary bg-primary'
                            : 'border-muted-foreground/50 bg-transparent',
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
                      className="flex w-full items-center gap-2 rounded-sm px-2 py-1 text-[11px] text-muted-foreground hover:bg-accent hover:text-destructive transition-colors"
                      onClick={() => {
                        existingLanes.forEach((l) => onRemoveAutomationLane?.(l.parameter));
                        setAutomationMenuOpen(false);
                      }}
                    >
                      Clear All
                    </button>
                  </>
                )}
              </div>
            )}
          </div>
        )}

        <Button variant="ghost" size="sm" className="h-5 w-5 p-0" onClick={onDelete}>
          <Trash2 className="h-3 w-3 text-muted-foreground" />
        </Button>
      </div>
      <div className="flex items-center gap-1 mt-0.5">
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0', track.muted && 'text-red-500')}
          onClick={onMuteToggle}
          title="Mute"
        >
          {track.muted ? <VolumeX className="h-3 w-3" /> : <Volume2 className="h-3 w-3" />}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          className={cn('h-5 w-5 p-0 text-[10px] font-bold', track.solo && 'text-yellow-500')}
          onClick={onSoloToggle}
          title="Solo"
        >
          S
        </Button>
        <Slider
          min={MIN_VOLUME}
          max={MAX_VOLUME}
          step={1}
          value={[track.volume]}
          onValueChange={([v]) => onVolumeChange(v)}
          className="w-12"
        />
        {onPanChange && (
          <Slider
            min={-1}
            max={1}
            step={0.01}
            value={[track.pan ?? 0]}
            onValueChange={([v]) => onPanChange(v)}
            className="w-10"
            title={`Pan: ${(track.pan ?? 0) > 0 ? `R${Math.round((track.pan ?? 0) * 100)}` : (track.pan ?? 0) < 0 ? `L${Math.round(Math.abs(track.pan ?? 0) * 100)}` : 'C'}`}
          />
        )}
      </div>
    </div>
  );
}
