import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { StartScreen } from '@/components/StartScreen';
import { TopBar } from '@/components/TopBar';
import { EffectsPanel } from '@/components/EffectsPanel';
import { TransportBar } from '@/components/TransportBar';
import { InstrumentDock } from '@/components/InstrumentDock';
import { ArrangementView } from '@/components/arrangement/ArrangementView';
import { PianoRollEditor } from '@/components/piano-roll-editor/PianoRollEditor';
import { ExportDialog } from '@/components/ExportDialog';
import { MixerView } from '@/components/mixer/MixerView';
import { CommandPalette } from '@/components/CommandPalette';
import { ErrorBoundary } from '@/components/ErrorBoundary';
import { Sidebar } from '@/components/sidebar/Sidebar';
import { SidebarProvider, SidebarTrigger } from '@/components/ui/sidebar';
import { SettingsView } from '@/components/settings/SettingsView';
import { SamplesView } from '@/components/samples/SamplesView';
import { DevView } from '@/components/dev/DevView';
import { TemplatePicker } from '@/components/TemplatePicker';
import { SynthEditor } from '@/components/SynthEditor';
import { BeatView } from '@/components/beat';
import { padColor, padName } from '@/components/beat/beatHelpers';
import { PADS_PER_BANK } from '@/types/sampler';
import { BrowserPanel } from '@/components/browser/BrowserPanel';
import { ControllerSetupDialog } from '@/components/controller/ControllerSetupDialog';
import { useAppMode } from '@/hooks/useAppMode';
import { useSettings } from '@/hooks/useSettings';
import { useAutoUpdater } from '@/hooks/useAutoUpdater';
import { useAudioEngine } from '@/hooks/useAudioEngine';
import { useMidi } from '@/hooks/useMidi';
import { useMidiNotes } from '@/hooks/useMidiNotes';
import { useDrumPads } from '@/hooks/useDrumPads';
import { useArrangement } from '@/hooks/useArrangement';
import { useUndoRedo } from '@/hooks/useUndoRedo';
import { useAudioInput } from '@/hooks/useAudioInput';
import { useProject } from '@/hooks/useProject';
import { useBeat } from '@/hooks/useBeat';
import { drumEngine } from '@/services/DrumEngine';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { noteRepeat } from '@/services/NoteRepeat';
import { installKnobRouting } from '@/services/KnobRouter';
import { registerDebugAppHooks } from '@/dev/debugHandle';
import { SYNTH_PRESETS } from '@/utils/constants';
import { detectChord } from '@/utils/chordDetection';
import type { CommandActions } from '@/config/commands';
import type { DrumPadId } from '@/types/drums';
import type { DawView } from '@/types/appMode';

const BROWSER_OPEN_KEY = 'micro-daw:browser-open';

function readBrowserOpen(): boolean {
  try {
    return localStorage.getItem(BROWSER_OPEN_KEY) !== 'false';
  } catch {
    return true;
  }
}

/** A modal dialog (command palette, controller setup, …) owns the keyboard. */
function modalOpen(): boolean {
  return !!document.querySelector('[role="dialog"][data-state="open"], [role="alertdialog"][data-state="open"]');
}

/** Typing into a text field (inputs, selects, contenteditable): single-key shortcuts stay off. */
function isTextTarget(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  );
}

export function App() {
  const { mode, switchMode } = useAppMode();
  const audio = useAudioEngine();
  const drums = useDrumPads();
  const arrangement = useArrangement();
  const undoRedo = useUndoRedo();
  const audioInput = useAudioInput();
  const project = useProject();
  const { settings, updateSettings } = useSettings();
  const autoUpdater = useAutoUpdater(settings.general.autoCheckUpdates);
  const [editingRegion, setEditingRegion] = useState<{ trackId: string; regionId: string } | null>(null);
  const [showExportDialog, setShowExportDialog] = useState(false);
  const [showMixer, setShowMixer] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [showTemplatePicker, setShowTemplatePicker] = useState(false);
  const [synthEditorOpen, setSynthEditorOpen] = useState(false);
  const [introDone, setIntroDone] = useState(false);
  /** Beat view is the DAW's home screen; Song = arrangement + piano roll. */
  const [dawView, setDawView] = useState<DawView>('beat');
  const [browserOpen, setBrowserOpen] = useState(readBrowserOpen);
  const [controllerSetupOpen, setControllerSetupOpen] = useState(false);
  const beat = useBeat();

  const toggleBrowser = useCallback(() => {
    setBrowserOpen((open) => {
      try {
        localStorage.setItem(BROWSER_OPEN_KEY, String(!open));
      } catch {
        // ignore (private mode / blocked storage)
      }
      return !open;
    });
  }, []);
  const toggleDawView = useCallback(() => setDawView((v) => (v === 'beat' ? 'song' : 'beat')), []);

  const {
    activeNotes,
    noteOn,
    noteOff,
    drumHit,
    drumPress,
    drumRelease,
    allNotesOff,
    getSoundingNotes,
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
  const detectedChord = useMemo(() => detectChord([...activeNotes.keys()]), [activeNotes]);

  // Hardware knobs → selected pad params, swing, tempo, drum-track and master volume.
  const changeVolume = audio.changeVolume;
  useEffect(() => installKnobRouting({ setMasterVolume: changeVolume }), [changeVolume]);

  /** Sample browser "assign" (Enter / double-click): the active drum track's selected pad. */
  const assignFromBrowser = useCallback(
    (path: string, name: string) => beat.actions.assignFile(arrangementEngine.getSelectedPad(), { path, name }),
    [beat.actions],
  );

  /** Song-view dock pads show the active kit's bank A (names + colours). */
  const kitPads = beat.kit.pads;
  const songPadLooks = useMemo(
    () => kitPads.slice(0, PADS_PER_BANK).map((p, i) => ({ name: padName(p), color: padColor(p, i) })),
    [kitPads],
  );

  const beatActions = beat.actions;
  /** Metronome: the loop recorder's click in the beat view, the global click in the song view. */
  const toggleMetronome = useCallback(() => {
    if (dawView === 'beat') beatActions.setLoopSettings({ metronome: !arrangementEngine.getLoopSettings().metronome });
    else arrangement.toggleMetronome();
  }, [dawView, beatActions, arrangement]);
  const togglePlay = useCallback(() => {
    if (dawView === 'beat') {
      beatActions.togglePlay();
    } else if (arrangement.transportState === 'playing' || arrangement.transportState === 'recording') {
      arrangement.stop();
    } else {
      arrangement.play();
    }
  }, [dawView, beatActions, arrangement]);

  // Dev-only: let window.__microDaw (src/dev/debugHandle.ts) drive mode + read held notes.
  const debugStateRef = useRef({ mode, activeNotes });
  useEffect(() => {
    debugStateRef.current = { mode, activeNotes };
  }, [mode, activeNotes]);
  useEffect(() => {
    if (!import.meta.env.DEV || !audio.started) return;
    return registerDebugAppHooks({
      setMode: switchMode,
      getMode: () => debugStateRef.current.mode,
      getActiveNotes: () => [...debugStateRef.current.activeNotes.keys()],
      getSoundingNotes,
      allNotesOff,
    });
  }, [audio.started, switchMode, getSoundingNotes, allNotesOff]);

  // Switching modes (sidebar, Ctrl+1..3, command palette) silences
  // everything live: held and sustained notes.
  const previousModeRef = useRef(mode);
  useEffect(() => {
    if (previousModeRef.current === mode) return;
    previousModeRef.current = mode;
    allNotesOff();
  }, [mode, allNotesOff]);

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
      'switch-samples': () => switchMode('samples'),
      'switch-dev': () => switchMode('dev'),
      'toggle-view': () => {
        switchMode('daw');
        toggleDawView();
      },
      'switch-beat': () => {
        switchMode('daw');
        setDawView('beat');
      },
      'switch-song': () => {
        switchMode('daw');
        setDawView('song');
      },
      'toggle-browser': () => {
        switchMode('daw');
        toggleBrowser();
      },
      'controller-setup': () => setControllerSetupOpen(true),

      // Transport
      'play-stop': togglePlay,
      'loop-record': () => {
        setDawView('beat');
        beatActions.toggleRecord();
      },
      'toggle-note-repeat': () => noteRepeat.toggle(),
      'clear-pattern': () => beatActions.clearPattern(),
      'record': () => {
        if (arrangement.transportState === 'recording') {
          arrangement.stopRecording();
        } else if (arrangement.transportState === 'stopped') {
          arrangement.startRecording();
        }
      },
      'toggle-metronome': toggleMetronome,

      // Editing
      'undo': () => undoRedo.undo(),
      'redo': () => undoRedo.redo(),
      'add-synth-track': () => arrangement.addTrack('synth'),
      'add-drum-track': () => arrangement.addTrack('drums'),
      'add-audio-track': () => arrangement.addTrack('audio'),
      'import-audio': () => arrangement.importAudioFile(),

      // Markers
      'add-marker': () => {
        const markers = arrangementEngine.getMarkers();
        const names = ['Intro', 'Verse', 'Chorus', 'Bridge', 'Drop', 'Breakdown', 'Outro', 'Section'];
        arrangement.addMarker(names[markers.length % names.length], arrangement.position);
      },
      'next-marker': () => arrangement.seekToNextMarker(),
      'prev-marker': () => arrangement.seekToPreviousMarker(),
      'create-group': () => {
        // Group all tracks (or could be smarter based on selection)
        const tracks = arrangementEngine.getTracks();
        if (tracks.length > 0) {
          arrangement.createGroup('Group', tracks.map((t) => t.id));
        }
      },
      'new-from-template': () => setShowTemplatePicker(true),

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
  }, [project, switchMode, arrangement, undoRedo, audio, toggleDawView, toggleBrowser, togglePlay, toggleMetronome, beatActions]);

  // Keyboard shortcuts
  useEffect(() => {
    if (!audio.started) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (isTextTarget(e.target)) return;
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
      if (mod && e.code === 'Digit2') { e.preventDefault(); switchMode('samples'); return; }
      if (mod && e.code === 'Digit3') { e.preventDefault(); switchMode('dev'); return; }

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

      // Import audio (global)
      if (mod && e.code === 'KeyI') {
        e.preventDefault();
        arrangement.importAudioFile();
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
      // Single-key shortcuts: not while a dialog is open or a focused widget already used the key.
      if (e.defaultPrevented || modalOpen()) return;
      const plain = !mod && !e.altKey && !e.shiftKey;

      // Tab — Beat / Song view (like a DAW's session/arrangement switch).
      if (plain && e.code === 'Tab') {
        e.preventDefault();
        toggleDawView();
        return;
      }
      // B — sample browser panel
      if (plain && e.code === 'KeyB') {
        e.preventDefault();
        toggleBrowser();
        return;
      }
      // N — note repeat
      if (plain && e.code === 'KeyN') {
        e.preventDefault();
        noteRepeat.toggle();
        return;
      }

      if (dawView === 'beat') {
        // Space / M / Shift+Backspace act on the beat pattern (R is handled by the beat view).
        if (plain && e.code === 'Space') {
          e.preventDefault();
          if (!e.repeat) togglePlay();
          return;
        }
        if (plain && e.code === 'KeyM') {
          e.preventDefault();
          toggleMetronome();
          return;
        }
        if (e.shiftKey && !mod && e.code === 'Backspace') {
          e.preventDefault();
          beatActions.clearPattern();
          return;
        }
      }

      // Escape — close piano roll
      if (e.code === 'Escape' && editingRegion && !e.defaultPrevented) {
        e.preventDefault();
        setEditingRegion(null);
        return;
      }

      // Ctrl+G — create group
      if (mod && e.code === 'KeyG') {
        e.preventDefault();
        const tracks = arrangementEngine.getTracks();
        if (tracks.length > 0) {
          arrangement.createGroup('Group', tracks.map((t) => t.id));
        }
        return;
      }

      // Shift+M — add marker
      if (e.shiftKey && e.code === 'KeyM') {
        e.preventDefault();
        const markers = arrangementEngine.getMarkers();
        const names = ['Intro', 'Verse', 'Chorus', 'Bridge', 'Drop', 'Breakdown', 'Outro', 'Section'];
        arrangement.addMarker(names[markers.length % names.length], arrangement.position);
        return;
      }

      // ] — next marker, [ — previous marker
      if (e.code === 'BracketRight') {
        e.preventDefault();
        arrangement.seekToNextMarker();
        return;
      }
      if (e.code === 'BracketLeft') {
        e.preventDefault();
        arrangement.seekToPreviousMarker();
        return;
      }

      if (dawView !== 'song' || mod || e.altKey) return;
      switch (e.code) {
        case 'Space':
          e.preventDefault();
          togglePlay();
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
        case 'KeyL':
          e.preventDefault();
          arrangement.toggleLoop();
          break;
        case 'KeyX':
          e.preventDefault();
          setShowMixer((prev) => !prev);
          break;
      }
    };

    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [audio.started, arrangement, undoRedo, editingRegion, mode, switchMode, project, dawView, toggleDawView, toggleBrowser, togglePlay, toggleMetronome, beatActions]);

  // The intro stays mounted (as an overlay) until its exit transition finishes.
  const intro = introDone ? null : (
    <StartScreen onStart={handleStart} ready={audio.started} onExited={() => setIntroDone(true)} />
  );

  if (!audio.started) {
    return <>{null}{intro}</>;
  }

  // On-screen pads always hit the drum engine (never the synth, whatever the routing mode).
  const handleDrumHit = (padId: DrumPadId, velocity: number) => {
    drumHit(padId, velocity);
  };

  return (
    <>
    <SidebarProvider defaultOpen={false} className="h-full min-h-0 flex-col">
      <ErrorBoundary zoneName="Controls" fallbackVariant="inline">
        <TopBar
          mode={mode}
          midi={{
            devices: midi.devices,
            activeDeviceId: midi.activeDeviceId,
            onSelectDevice: midi.selectDevice,
            activeNotes,
            detectedChord,
          }}
          audio={{
            volume: audio.volume,
            onVolumeChange: audio.changeVolume,
            presetIndex: audio.presetIndex,
            onPresetChange: audio.changePreset,
            muted: audioInput.muted,
            onToggleMute: audioInput.toggleMute,
            onToggleSynthEditor: () => setSynthEditorOpen((prev) => !prev),
            synthEditorOpen,
          }}
          history={{
            canUndo: undoRedo.canUndo,
            canRedo: undoRedo.canRedo,
            onUndo: undoRedo.undo,
            onRedo: undoRedo.redo,
          }}
          project={{
            projectName: project.projectName,
            isDirty: project.isDirty,
            onSave: project.save,
          }}
          sidebarTrigger={<SidebarTrigger />}
          view={{ current: dawView, onChange: setDawView }}
          browser={{ open: browserOpen, onToggle: toggleBrowser }}
          onOpenControllerSetup={() => setControllerSetupOpen(true)}
          center={
            mode === 'daw' && dawView === 'song' ? (
              <TransportBar
                state={arrangement.transportState === 'recording' ? 'recording' : arrangement.transportState}
                bpm={arrangement.bpm}
                metronomeOn={arrangement.metronomeOn}
                positionBeats={arrangement.position}
                recordDisabled={!arrangement.armedTrackId}
                loopEnabled={arrangement.loopEnabled}
                isPunchRecording={arrangement.transportState === 'recording' && arrangement.loopEnabled}
                markers={arrangement.markers}
                onLoopToggle={arrangement.toggleLoop}
                onSeekToMarker={arrangement.seekToMarker}
                onRecord={arrangement.startRecording}
                onStopRecording={arrangement.stopRecording}
                onPlay={arrangement.play}
                onStop={arrangement.stop}
                onBpmChange={arrangement.setBpm}
                onMetronomeToggle={arrangement.toggleMetronome}
              />
            ) : undefined
          }
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
            <div className="flex min-h-0 flex-1">
              {browserOpen && (
                <ErrorBoundary zoneName="Browser" fallbackVariant="inline">
                  <BrowserPanel onAssign={assignFromBrowser} />
                </ErrorBoundary>
              )}

              <div className="flex min-w-0 flex-1 flex-col">
                {dawView === 'beat' ? (
                  <>
                    <ErrorBoundary zoneName="Beat" fallbackVariant="panel">
                      <BeatView
                        className="h-auto min-h-0 flex-1"
                        beat={beat}
                        onPadHit={drumPress}
                        onPadRelease={drumRelease}
                        onPadPreview={drumHit}
                        onOpenControllerSetup={() => setControllerSetupOpen(true)}
                        onExport={() => setShowExportDialog(true)}
                      />
                    </ErrorBoundary>

                    {/* Keys for the synth (MPK keys play it too); collapsed so the pads keep the room. */}
                    <ErrorBoundary zoneName="Instrument" fallbackVariant="inline">
                      <div className="flex shrink-0 flex-col bg-surface-1">
                        {synthEditorOpen && (
                          <SynthEditor
                            presetIndex={audio.presetIndex}
                            onPresetChange={audio.changePreset}
                            onSynthParam={audio.setSynthParam}
                          />
                        )}
                        <InstrumentDock
                          activeNotes={activeNotes}
                          onNoteOn={noteOn}
                          onNoteOff={noteOff}
                          activePads={drums.activePads}
                          onDrumHit={handleDrumHit}
                          routingMode={routingMode}
                          onRoutingModeChange={setRoutingMode}
                          detectedChord={detectedChord}
                          padsAvailable={false}
                          defaultCollapsed
                        />
                      </div>
                    </ErrorBoundary>
                  </>
                ) : (
                  <>
                    {editingRegion ? (
                      <ErrorBoundary zoneName="Piano Roll" fallbackVariant="panel" onReset={() => setEditingRegion(null)}>
                        <PianoRollEditor
                          trackId={editingRegion.trackId}
                          regionId={editingRegion.regionId}
                          tracks={arrangement.tracks}
                          positionBeats={arrangement.position}
                          transportState={arrangement.transportState}
                          loopEnabled={arrangement.loopEnabled}
                          loopMarkers={arrangement.loopMarkers}
                          onClose={() => setEditingRegion(null)}
                          onEditNotes={arrangement.editRegionNotes}
                          onPreviewNote={arrangement.previewNote}
                          onPlayFrom={arrangement.playFrom}
                          onStop={arrangement.stop}
                          onSeek={arrangement.seek}
                          onSetLoop={arrangement.setLoopMarkers}
                          onToggleLoop={arrangement.toggleLoop}
                        />
                      </ErrorBoundary>
                    ) : (
                      <ErrorBoundary zoneName="Arrangement" fallbackVariant="panel">
                        <ArrangementView
                          arrangement={arrangement}
                          history={undoRedo}
                          onEditRegion={(trackId, regionId) => setEditingRegion({ trackId, regionId })}
                          onExport={() => setShowExportDialog(true)}
                        />
                      </ErrorBoundary>
                    )}

                    {showMixer && (
                      <MixerView
                        tracks={arrangement.tracks}
                        onSetTrackVolume={arrangement.setTrackVolume}
                        onSetTrackPan={arrangement.setTrackPan}
                        onSetTrackMute={arrangement.setTrackMute}
                        onSetTrackSolo={arrangement.setTrackSolo}
                      />
                    )}

                    {/* Instrument rack: what you play live (synth editor, live FX, keys + pads) */}
                    <ErrorBoundary zoneName="Instrument" fallbackVariant="inline">
                      <div className="flex shrink-0 flex-col border-t border-border bg-surface-1">
                        {synthEditorOpen && (
                          <SynthEditor
                            presetIndex={audio.presetIndex}
                            onPresetChange={audio.changePreset}
                            onSynthParam={audio.setSynthParam}
                          />
                        )}
                        <EffectsPanel
                          effectParams={effectParams}
                          onEffectChange={setEffectParams}
                        />
                        {!editingRegion && (
                          <InstrumentDock
                            activeNotes={activeNotes}
                            onNoteOn={noteOn}
                            onNoteOff={noteOff}
                            activePads={drums.activePads}
                            onDrumHit={handleDrumHit}
                            routingMode={routingMode}
                            onRoutingModeChange={setRoutingMode}
                            detectedChord={detectedChord}
                            padLooks={songPadLooks}
                          />
                        )}
                      </div>
                    </ErrorBoundary>
                  </>
                )}
              </div>
            </div>
          )}

          {mode === 'daw' && showExportDialog && (
            <ExportDialog onClose={() => setShowExportDialog(false)} />
          )}

          {mode === 'daw' && showTemplatePicker && (
            <TemplatePicker
              onSelect={(template) => {
                setShowTemplatePicker(false);
                project.newProjectFromTemplate(template.arrangement);
              }}
              onClose={() => setShowTemplatePicker(false)}
            />
          )}

          <ControllerSetupDialog open={controllerSetupOpen} onOpenChange={setControllerSetupOpen} />

          <CommandPalette
            open={commandPaletteOpen}
            onOpenChange={setCommandPaletteOpen}
            mode={mode}
            actions={commandActions}
            recentProjects={project.recentProjects}
            onOpenRecent={project.openRecent}
          />

          {mode === 'samples' && (
            <SamplesView onSwitchToDAW={() => switchMode('daw')} />
          )}

          {mode === 'dev' && (
            <DevView
              activeDeviceId={midi.activeDeviceId}
              devices={midi.devices}
            />
          )}

          {mode === 'settings' && (
            <SettingsView
              settings={settings}
              onUpdateSettings={updateSettings}
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
    {intro}
    </>
  );
}
