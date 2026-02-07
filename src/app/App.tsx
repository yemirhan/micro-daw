import { useCallback, useEffect, useState } from 'react';
import { StartScreen } from '@/components/StartScreen';
import { TopBar } from '@/components/TopBar';
import { EffectsPanel } from '@/components/EffectsPanel';
import { TransportBar } from '@/components/TransportBar';
import { InstrumentDock } from '@/components/InstrumentDock';
import { ArrangementView } from '@/components/arrangement/ArrangementView';
import { PianoRollEditor } from '@/components/piano-roll-editor/PianoRollEditor';
import { ExportDialog } from '@/components/ExportDialog';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { useAudioEngine } from '@/hooks/useAudioEngine';
import { useMidi } from '@/hooks/useMidi';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useDrumPads } from '@/hooks/useDrumPads';
import { useLearningMode } from '@/hooks/useLearningMode';
import { useArrangement } from '@/hooks/useArrangement';
import { useUndoRedo } from '@/hooks/useUndoRedo';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { DRUM_SOUNDS } from '@/utils/constants';
import type { DrumPadId } from '@/types/drums';

export function App() {
  const audio = useAudioEngine();
  const drums = useDrumPads();
  const arrangement = useArrangement();
  const undoRedo = useUndoRedo();
  const [editingRegion, setEditingRegion] = useState<{ trackId: string; regionId: string } | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);

  const {
    activeNotes,
    noteOn,
    noteOff,
    callbacks,
    effectParams,
    setEffectParams,
    routingMode,
    setRoutingMode,
  } = useMidiNotes({
    onVolumeChange: audio.changeVolume,
    onDrumFlash: drums.flashPad,
  });

  const midi = useMidi(callbacks);
  const learning = useLearningMode(activeNotes);

  const handleStart = useCallback(async () => {
    await audio.start();
    await drumEngine.start();
    await midi.enable();
  }, [audio, midi]);

  // Keyboard shortcuts
  useEffect(() => {
    if (!audio.started) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const mod = e.metaKey || e.ctrlKey;

      // Undo/Redo
      if (mod && e.code === 'KeyZ') {
        e.preventDefault();
        if (e.shiftKey) {
          undoRedo.redo();
        } else {
          undoRedo.undo();
        }
        return;
      }

      // Export dialog
      if (mod && e.code === 'KeyE') {
        e.preventDefault();
        setShowExportDialog((prev) => !prev);
        return;
      }

      // Escape — close piano roll
      if (e.code === 'Escape' && editingRegion) {
        e.preventDefault();
        setEditingRegion(null);
        return;
      }

      switch (e.code) {
        case 'Space':
          e.preventDefault();
          if (arrangement.transportState === 'playing' || arrangement.transportState === 'recording') {
            arrangement.stop();
          } else {
            arrangement.play();
          }
          break;
        case 'KeyR':
          e.preventDefault();
          if (arrangement.transportState === 'recording') {
            arrangement.stopRecording();
          } else if (arrangement.transportState === 'stopped') {
            arrangement.startRecording();
          }
          break;
        case 'KeyM':
          e.preventDefault();
          arrangement.toggleMetronome();
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [audio.started, arrangement, undoRedo, editingRegion]);

  if (!audio.started) {
    return <StartScreen onStart={handleStart} />;
  }

  return (
    <div className="flex h-full flex-col">
      <ErrorBoundary zoneName="Controls" fallbackVariant="inline">
        <TopBar
          devices={midi.devices}
          activeDeviceId={midi.activeDeviceId}
          onSelectDevice={midi.selectDevice}
          activeNotes={activeNotes}
          volume={audio.volume}
          onVolumeChange={audio.changeVolume}
          presetIndex={audio.presetIndex}
          onPresetChange={audio.changePreset}
          detectedChord={learning.detectedChord}
          routingMode={routingMode}
          onRoutingModeChange={setRoutingMode}
          canUndo={undoRedo.canUndo}
          canRedo={undoRedo.canRedo}
          onUndo={undoRedo.undo}
          onRedo={undoRedo.redo}
        />
      </ErrorBoundary>

      <ErrorBoundary zoneName="Transport" fallbackVariant="inline">
        <EffectsPanel
          effectParams={effectParams}
          onEffectChange={setEffectParams}
        />

        <TransportBar
          state={arrangement.transportState === 'recording' ? 'recording' : arrangement.transportState}
          bpm={arrangement.bpm}
          metronomeOn={arrangement.metronomeOn}
          positionBeats={arrangement.position}
          recordDisabled={!arrangement.armedTrackId}
          onRecord={arrangement.startRecording}
          onStopRecording={arrangement.stopRecording}
          onPlay={arrangement.play}
          onStop={arrangement.stop}
          onBpmChange={arrangement.setBpm}
          onMetronomeToggle={arrangement.toggleMetronome}
        />
      </ErrorBoundary>

      {editingRegion ? (
        <ErrorBoundary zoneName="Piano Roll" fallbackVariant="panel" onReset={() => setEditingRegion(null)}>
          <PianoRollEditor
            trackId={editingRegion.trackId}
            regionId={editingRegion.regionId}
            arrangement={arrangementEngine.getArrangement()}
            onClose={() => setEditingRegion(null)}
            onUpdateNotes={arrangement.updateRegionNotes}
            onResizeRegion={arrangement.resizeRegion}
          />
        </ErrorBoundary>
      ) : (
        <ErrorBoundary zoneName="Arrangement" fallbackVariant="panel">
          <ArrangementView
            tracks={arrangement.tracks}
            transportState={arrangement.transportState}
            position={arrangement.position}
            lengthBeats={arrangement.lengthBeats}
            recordingTrackId={arrangement.recordingTrackId}
            armedTrackId={arrangement.armedTrackId}
            liveRegion={arrangement.liveRegion}
            onAddTrack={arrangement.addTrack}
            onRemoveTrack={arrangement.removeTrack}
            onSetTrackInstrument={arrangement.setTrackInstrument}
            onSetTrackVolume={arrangement.setTrackVolume}
            onSetTrackMute={arrangement.setTrackMute}
            onSetTrackSolo={arrangement.setTrackSolo}
            onMoveRegion={arrangement.moveRegion}
            onResizeRegion={arrangement.resizeRegion}
            onRemoveRegion={arrangement.removeRegion}
            onSplitRegion={arrangement.splitRegion}
            onDuplicateRegion={arrangement.duplicateRegion}
            onArmTrack={arrangement.armTrack}
            canUndo={undoRedo.canUndo}
            canRedo={undoRedo.canRedo}
            onUndo={undoRedo.undo}
            onRedo={undoRedo.redo}
            onEditRegion={(trackId, regionId) => setEditingRegion({ trackId, regionId })}
            onCopyRegion={arrangement.copyRegion}
            onPasteRegion={arrangement.pasteRegion}
            hasClipboard={arrangementEngine.hasClipboard()}
            onExport={() => setShowExportDialog(true)}
          />

          <InstrumentDock
            activeNotes={activeNotes}
            onNoteOn={noteOn}
            onNoteOff={noteOff}
            highlightedNotes={learning.highlightedNotes}
            activePads={drums.activePads}
            onDrumHit={(padId: DrumPadId, velocity: number) => {
              noteOn(DRUM_SOUNDS[padId].midiNote, velocity);
            }}
          />
        </ErrorBoundary>
      )}

      {showExportDialog && (
        <ExportDialog onClose={() => setShowExportDialog(false)} />
      )}
    </div>
  );
}
