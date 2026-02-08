import { useCallback, useEffect, useMemo, useState } from 'react';
import { StartScreen } from '@/components/StartScreen';
import { TopBar } from '@/components/TopBar';
import { EffectsPanel } from '@/components/EffectsPanel';
import { TransportBar } from '@/components/TransportBar';
import { InstrumentDock } from '@/components/InstrumentDock';
import { ArrangementView } from '@/components/arrangement/ArrangementView';
import { PianoRollEditor } from '@/components/piano-roll-editor/PianoRollEditor';
import { ExportDialog } from '@/components/ExportDialog';
import { CommandPalette } from '@/components/CommandPalette';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { Sidebar } from '@/components/sidebar/Sidebar';
import { SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar';
import { LearnView } from '@/components/learn/LearnView';
import { PracticeView } from '@/components/practice/PracticeView';
import { SettingsView } from '@/components/settings/SettingsView';
import { useAppMode } from '@/hooks/useAppMode';
import { useSettings } from '@/hooks/useSettings';
import { useAutoUpdater } from '@/hooks/useAutoUpdater';
import { useAudioEngine } from '@/hooks/useAudioEngine';
import { useMidi } from '@/hooks/useMidi';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useDrumPads } from '@/hooks/useDrumPads';
import { useLearningMode } from '@/hooks/useLearningMode';
import { useArrangement } from '@/hooks/useArrangement';
import { useUndoRedo } from '@/hooks/useUndoRedo';
import { useAudioInput } from '@/hooks/useAudioInput';
import { useProject } from '@/hooks/useProject';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { DRUM_SOUNDS, SYNTH_PRESETS } from '@/utils/constants';
import type { CommandActions } from '@/config/commands';
import type { DrumPadId } from '@/types/drums';

export function App() {
  const { mode, switchMode } = useAppMode();
  const audio = useAudioEngine();
  const drums = useDrumPads();
  const arrangement = useArrangement();
  const undoRedo = useUndoRedo();
  const audioInput = useAudioInput();
  const project = useProject();
  const { settings, updateSettings, resetLessonProgress, resetPracticeProgress } = useSettings();
  const autoUpdater = useAutoUpdater(settings.general.autoCheckUpdates);
  const [editingRegion, setEditingRegion] = useState<{ trackId: string; regionId: string } | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);

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
    muted: audioInput.muted,
  });

  const midi = useMidi(callbacks);
  const learning = useLearningMode(activeNotes);

  const handleStart = useCallback(async () => {
    await audio.start();
    await drumEngine.start();
    await midi.enable();
  }, [audio, midi]);

  // Build command palette action map
  const commandActions: CommandActions = useMemo(() => {
    const actions: CommandActions = {
      // Project
      'new-project': () => project.newProject(),
      'open-project': () => project.open(),
      'save': () => project.save(),
      'save-as': () => project.saveAs(),

      // Navigation
      'switch-daw': () => switchMode('daw'),
      'switch-learn': () => switchMode('learn'),
      'switch-practice': () => switchMode('practice'),

      // Transport
      'play-stop': () => {
        if (arrangement.transportState === 'playing' || arrangement.transportState === 'recording') {
          arrangement.stop();
        } else {
          arrangement.play();
        }
      },
      'record': () => {
        if (arrangement.transportState === 'recording') {
          arrangement.stopRecording();
        } else if (arrangement.transportState === 'stopped') {
          arrangement.startRecording();
        }
      },
      'toggle-metronome': () => arrangement.toggleMetronome(),

      // Editing
      'undo': () => undoRedo.undo(),
      'redo': () => undoRedo.redo(),
      'add-synth-track': () => arrangement.addTrack('synth'),
      'add-drum-track': () => arrangement.addTrack('drums'),

      // Export
      'export-wav': () => setShowExportDialog(true),
      'export-midi': () => setShowExportDialog(true),

      // View
      'toggle-fullscreen': () => window.electronAPI?.toggleFullscreen(),
      'open-settings': () => switchMode('settings'),
    };

    // Instrument presets
    SYNTH_PRESETS.forEach((_, i) => {
      actions[`preset-${i}`] = () => audio.changePreset(i);
    });

    return actions;
  }, [project, switchMode, arrangement, undoRedo, audio]);

  // Keyboard shortcuts
  useEffect(() => {
    if (!audio.started) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      const mod = e.metaKey || e.ctrlKey;

      // Command palette
      if (mod && e.code === 'KeyK') {
        e.preventDefault();
        setCommandPaletteOpen((prev) => !prev);
        return;
      }

      // Project shortcuts
      if (mod && e.code === 'KeyS') {
        e.preventDefault();
        if (e.shiftKey) { project.saveAs(); } else { project.save(); }
        return;
      }
      if (mod && e.code === 'KeyO') { e.preventDefault(); project.open(); return; }
      if (mod && e.code === 'KeyN') { e.preventDefault(); project.newProject(); return; }

      // Settings: Cmd+,
      if (mod && e.key === ',') {
        e.preventDefault();
        switchMode(mode === 'settings' ? 'daw' : 'settings');
        return;
      }

      // Mode switching: Ctrl+1/2/3
      if (mod && e.code === 'Digit1') { e.preventDefault(); switchMode('daw'); return; }
      if (mod && e.code === 'Digit2') { e.preventDefault(); switchMode('learn'); return; }
      if (mod && e.code === 'Digit3') { e.preventDefault(); switchMode('practice'); return; }

      // Undo/Redo (global)
      if (mod && e.code === 'KeyZ') {
        e.preventDefault();
        if (e.shiftKey) {
          undoRedo.redo();
        } else {
          undoRedo.undo();
        }
        return;
      }

      // Export dialog (global)
      if (mod && e.code === 'KeyE') {
        e.preventDefault();
        setShowExportDialog((prev) => !prev);
        return;
      }

      // DAW-only shortcuts
      if (mode !== 'daw') return;

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
  }, [audio.started, arrangement, undoRedo, editingRegion, mode, switchMode, project]);

  if (!audio.started) {
    return <StartScreen onStart={handleStart} />;
  }

  const handleDrumHit = (padId: DrumPadId, velocity: number) => {
    noteOn(DRUM_SOUNDS[padId].midiNote, velocity);
  };

  return (
    <SidebarProvider defaultOpen={false} className="h-full min-h-0 flex-col">
      <ErrorBoundary zoneName="Controls" fallbackVariant="inline">
        <TopBar
          mode={mode}
          devices={midi.devices}
          activeDeviceId={midi.activeDeviceId}
          onSelectDevice={midi.selectDevice}
          activeNotes={activeNotes}
          volume={audio.volume}
          onVolumeChange={audio.changeVolume}
          presetIndex={audio.presetIndex}
          onPresetChange={audio.changePreset}
          detectedChord={learning.detectedChord}
          canUndo={undoRedo.canUndo}
          canRedo={undoRedo.canRedo}
          onUndo={undoRedo.undo}
          onRedo={undoRedo.redo}
          muted={audioInput.muted}
          onToggleMute={audioInput.toggleMute}
          projectName={project.projectName}
          isDirty={project.isDirty}
          onSave={project.save}
          sidebarTrigger={<SidebarTrigger className="-ml-1" />}
        />
      </ErrorBoundary>

      <div className="flex min-h-0 flex-1">
        <Sidebar
          mode={mode}
          onModeChange={switchMode}
          recentProjects={project.recentProjects}
          onOpenRecent={project.openRecent}
          onNewProject={project.newProject}
          onOpen={project.open}
          onSettingsOpen={() => switchMode('settings')}
        />

        <div className="flex min-w-0 flex-1 flex-col">
          {mode === 'daw' && (
            <>
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
                    onDrumHit={handleDrumHit}
                    routingMode={routingMode}
                    onRoutingModeChange={setRoutingMode}
                  />
                </ErrorBoundary>
              )}

              {showExportDialog && (
                <ExportDialog onClose={() => setShowExportDialog(false)} />
              )}
            </>
          )}

          <CommandPalette
            open={commandPaletteOpen}
            onOpenChange={setCommandPaletteOpen}
            mode={mode}
            actions={commandActions}
            recentProjects={project.recentProjects}
            onOpenRecent={project.openRecent}
          />

          {mode === 'learn' && (
            <LearnView
              activeNotes={activeNotes}
              onNoteOn={noteOn}
              onNoteOff={noteOff}
              activePads={drums.activePads}
              onDrumHit={handleDrumHit}
              detectedChord={learning.detectedChord}
            />
          )}

          {mode === 'practice' && (
            <PracticeView
              activeNotes={activeNotes}
              onNoteOn={noteOn}
              onNoteOff={noteOff}
              activePads={drums.activePads}
              onDrumHit={handleDrumHit}
              detectedChord={learning.detectedChord}
              highlightedNotes={learning.highlightedNotes}
              selectedRoot={learning.selectedRoot}
              setSelectedRoot={learning.setSelectedRoot}
              selectedScale={learning.selectedScale}
              setSelectedScale={learning.setSelectedScale}
            />
          )}

          {mode === 'settings' && (
            <SettingsView
              settings={settings}
              onUpdateSettings={updateSettings}
              onResetLessonProgress={resetLessonProgress}
              onResetPracticeProgress={resetPracticeProgress}
              updateStatus={autoUpdater.updateStatus}
              onCheckForUpdates={autoUpdater.checkForUpdates}
              onDownloadUpdate={autoUpdater.downloadUpdate}
              onInstallUpdate={autoUpdater.installUpdate}
              appVersion={autoUpdater.appVersion}
            />
          )}
        </div>
      </div>
    </SidebarProvider>
  );
}
