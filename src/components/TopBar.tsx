import { useState, useEffect, useCallback, type ReactNode } from 'react';
import { Maximize2, Minimize2, Undo2, Redo2 } from 'lucide-react';
import { MidiDeviceSelector } from '@/components/MidiDeviceSelector';
import { NoteDisplay } from '@/components/NoteDisplay';
import { VolumeControl } from '@/components/VolumeControl';
import { InstrumentSelector } from '@/components/InstrumentSelector';
import { ChordDisplay } from '@/components/ChordDisplay';
import { Button } from '@/components/ui/button';
import type { MidiDeviceInfo } from '@/types/midi';
import type { ChordInfo } from '@/types/music';
import type { ActiveNote } from '@/hooks/useMidiNotes';
import type { AppMode } from '@/types/appMode';

interface TopBarProps {
  mode: AppMode;
  midi: {
    devices: MidiDeviceInfo[];
    activeDeviceId: string | null;
    onSelectDevice: (id: string) => void;
    activeNotes: Map<number, ActiveNote>;
    detectedChord: ChordInfo | null;
  };
  audio: {
    volume: number;
    onVolumeChange: (db: number) => void;
    presetIndex: number;
    onPresetChange: (index: number) => void;
    muted: boolean;
    onToggleMute: () => void;
  };
  history: {
    canUndo: boolean;
    canRedo: boolean;
    onUndo: () => void;
    onRedo: () => void;
  };
  project: {
    projectName: string;
    isDirty: boolean;
    onSave: () => void;
  };
  sidebarTrigger?: ReactNode;
}

export function TopBar({
  mode,
  midi,
  audio,
  history,
  project,
  sidebarTrigger,
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
    <div className="flex items-center justify-between border-b border-border bg-card/80 backdrop-blur-md pl-24 pr-4 py-2" style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}>
      <div className="flex items-center gap-3" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        {sidebarTrigger}
        <button
          onClick={project.onSave}
          className="flex items-center gap-1.5 rounded px-2 py-1 text-xs font-medium text-foreground/80 transition-colors hover:bg-muted hover:text-foreground"
          title={project.isDirty ? 'Save (Ctrl+S)' : project.projectName}
        >
          {project.isDirty && (
            <span className="inline-block h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: 'oklch(0.75 0.18 60)' }} />
          )}
          <span className="max-w-[120px] truncate">{project.projectName}</span>
        </button>
        <div className="h-4 w-px bg-border/60" />
        {mode === 'daw' && (
          <>
            <div className="flex items-center gap-0.5">
              <Button
                variant="ghost"
                size="sm"
                className="h-6 w-6 p-0"
                disabled={!history.canUndo}
                onClick={history.onUndo}
                title="Undo (Ctrl+Z)"
              >
                <Undo2 className="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 w-6 p-0"
                disabled={!history.canRedo}
                onClick={history.onRedo}
                title="Redo (Ctrl+Shift+Z)"
              >
                <Redo2 className="h-3.5 w-3.5" />
              </Button>
            </div>
            <div className="h-4 w-px bg-border/60" />
          </>
        )}
        <MidiDeviceSelector
          devices={midi.devices}
          activeDeviceId={midi.activeDeviceId}
          onSelect={midi.onSelectDevice}
        />
        <div className="h-4 w-px bg-border/60" />
        <ChordDisplay chord={midi.detectedChord} />
      </div>
      <div className="flex items-center gap-4" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
        <NoteDisplay activeNotes={midi.activeNotes} />
        <div className="h-4 w-px bg-border/60" />
        <InstrumentSelector presetIndex={audio.presetIndex} onChange={audio.onPresetChange} />
        <div className="h-4 w-px bg-border/60" />
        <VolumeControl volume={audio.volume} onChange={audio.onVolumeChange} muted={audio.muted} onToggleMute={audio.onToggleMute} />
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
