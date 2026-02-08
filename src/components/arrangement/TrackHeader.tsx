import { useState, useRef, useEffect } from 'react';
import { Volume2, VolumeX, Trash2, SlidersHorizontal, Activity, FileAudio } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Slider } from '@/components/ui/slider';
import { cn } from '@/lib/utils';
import type { Track, TrackInstrument, AutomationParameter, AutomationLane } from '@/types/arrangement';
import { SYNTH_PRESETS, MIN_VOLUME, MAX_VOLUME } from '@/utils/constants';
import { getAutomationParameterLabel } from '@/utils/automationHelpers';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select';

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
  const [automationMenuOpen, setAutomationMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [menuPos, setMenuPos] = useState<{ top: number; left: number }>({ top: 0, left: 0 });

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
          <Select
            value={`${track.instrument.type}-${track.instrument.presetIndex}`}
            onValueChange={(val) => {
              const [type, idx] = val.split('-');
              onInstrumentChange({ type: type as 'synth' | 'drums', presetIndex: Number(idx) });
            }}
          >
            <SelectTrigger className="h-5 flex-1 border-0 bg-transparent px-1 text-[11px] font-semibold">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SYNTH_PRESETS.map((p, i) => (
                <SelectItem key={`synth-${i}`} value={`synth-${i}`}>{p.name}</SelectItem>
              ))}
              <SelectItem value="drums-0">Drums</SelectItem>
            </SelectContent>
          </Select>
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
