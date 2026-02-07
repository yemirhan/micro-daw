import { useState, useEffect, useCallback } from 'react';
import { Maximize2, Minimize2, Undo2, Redo2 } from 'lucide-react';
import { MidiDeviceSelector } from '@/components/MidiDeviceSelector';
import { NoteDisplay } from '@/components/NoteDisplay';
import { VolumeControl } from '@/components/VolumeControl';
import { InstrumentSelector } from '@/components/InstrumentSelector';
import { ChordDisplay } from '@/components/ChordDisplay';
import { RoutingModeSelector } from '@/components/RoutingModeSelector';
import { Button } from '@/components/ui/button';
import type { MidiDeviceInfo } from '@/types/midi';
import type { ChordInfo } from '@/types/music';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { RoutingMode } from '@/utils/constants';

interface TopBarProps {
  devices: MidiDeviceInfo[];
  activeDeviceId: string | null;
  onSelectDevice: (id: string) => void;
  activeNotes: Map<number, ActiveNote>;
  volume: number;
  onVolumeChange: (db: number) => void;
  presetIndex: number;
  onPresetChange: (index: number) => void;
  detectedChord: ChordInfo | null;
  routingMode: RoutingMode;
  onRoutingModeChange: (mode: RoutingMode) => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
}

export function TopBar({
  devices,
  activeDeviceId,
  onSelectDevice,
  activeNotes,
  volume,
  onVolumeChange,
  presetIndex,
  onPresetChange,
  detectedChord,
  routingMode,
  onRoutingModeChange,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
}: TopBarProps) {
  const [isFullscreen, setIsFullscreen] = useState(false);

  useEffect(() => {
    if (!window.electronAPI) return;
    window.electronAPI.isFullscreen().then(setIsFullscreen);
    return window.electronAPI.onFullscreenChanged(setIsFullscreen);
  }, []);

  const toggleFullscreen = useCallback(() => {
    window.electronAPI?.toggleFullscreen();
  }, []);

  return (
    <div className="flex items-center justify-between border-b border-border bg-card/80 backdrop-blur-md pl-20 pr-4 py-2" style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}>
      <div className="flex items-center gap-4" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0"
            disabled={!canUndo}
            onClick={onUndo}
            title="Undo (Ctrl+Z)"
          >
            <Undo2 className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-6 w-6 p-0"
            disabled={!canRedo}
            onClick={onRedo}
            title="Redo (Ctrl+Shift+Z)"
          >
            <Redo2 className="h-3.5 w-3.5" />
          </Button>
        </div>
        <div className="h-4 w-px bg-border/60" />
        <MidiDeviceSelector
          devices={devices}
          activeDeviceId={activeDeviceId}
          onSelect={onSelectDevice}
        />
        <div className="h-4 w-px bg-border/60" />
        <ChordDisplay chord={detectedChord} />
      </div>
      <div className="flex items-center gap-4" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        <NoteDisplay activeNotes={activeNotes} />
        <div className="h-4 w-px bg-border/60" />
        <RoutingModeSelector mode={routingMode} onChange={onRoutingModeChange} />
        <div className="h-4 w-px bg-border/60" />
        <InstrumentSelector presetIndex={presetIndex} onChange={onPresetChange} />
        <div className="h-4 w-px bg-border/60" />
        <VolumeControl volume={volume} onChange={onVolumeChange} />
        <div className="h-4 w-px bg-border/60" />
        <Button
          variant="ghost"
          size="sm"
          className="h-6 w-6 p-0"
          onClick={toggleFullscreen}
          title={isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}
        >
          {isFullscreen ? <Minimize2 className="h-3.5 w-3.5" /> : <Maximize2 className="h-3.5 w-3.5" />}
        </Button>
      </div>
    </div>
  );
}
