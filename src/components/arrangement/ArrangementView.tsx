import { useState, useRef, useCallback, useEffect } from 'react';
import { ArrangementToolbar } from './ArrangementToolbar';
import { TimelineRuler } from './TimelineRuler';
import { TrackHeader } from './TrackHeader';
import { TrackLane, getTrackHeight, laneGridStyle } from './TrackLane';
import { AUTOMATION_LANE_HEIGHT } from './AutomationLaneView';
import { Plus } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { getAutomationParameterLabel } from '@/utils/automationHelpers';
import { Playhead } from './Playhead';
import { RegionContextMenu } from './RegionContextMenu';
import { MarkerContextMenu } from './MarkerContextMenu';
import { GroupHeader } from './GroupHeader';
import { TrackEffectsPanel } from './TrackEffectsPanel';
import type { Track } from '@/types/arrangement';
import { DEFAULT_PX_PER_BEAT, MIN_PX_PER_BEAT, MAX_PX_PER_BEAT, DEFAULT_MARKER_NAMES } from '@/utils/constants';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { beatToPx } from '@/utils/arrangementHelpers';
import type { ArrangementTool } from './ArrangementToolbar';
import type { ArrangementController } from '@/hooks/useArrangement';

const NOOP: (..._args: unknown[]) => void = () => undefined;
const HEADER_WIDTH = 224;
const RULER_HEIGHT = 28;

interface ArrangementHistoryController {
  canUndo: boolean;
  canRedo: boolean;
  undo: () => void;
  redo: () => void;
}

interface ArrangementViewProps {
  arrangement: ArrangementController;
  history: ArrangementHistoryController;
  onEditRegion?: (trackId: string, regionId: string) => void;
  onExport: () => void;
}

export function ArrangementView({
  arrangement,
  history,
  onEditRegion,
  onExport,
}: ArrangementViewProps) {
  const {
    bpm,
    tracks,
    transportState,
    position,
    lengthBeats,
    recordingTrackId,
    armedTrackId,
    liveRegion,
    addTrack: onAddTrack,
    removeTrack: onRemoveTrack,
    setTrackInstrument: onSetTrackInstrument,
    setTrackVolume: onSetTrackVolume,
    setTrackPan: onSetTrackPan,
    setTrackMute: onSetTrackMute,
    setTrackSolo: onSetTrackSolo,
    moveRegion: onMoveRegion,
    resizeRegion: onResizeRegion,
    removeRegion: onRemoveRegion,
    splitRegion: onSplitRegion,
    duplicateRegion: onDuplicateRegion,
    armTrack: onArmTrack,
    copyRegion: onCopyRegion,
    pasteRegion: onPasteRegion,
    loopEnabled,
    loopMarkers,
    setLoopMarkers: onLoopMarkersChange,
    quantizeRegion: onQuantizeRegion,
    addAutomationLane: onAddAutomationLane,
    removeAutomationLane: onRemoveAutomationLane,
    setAutomationPoint: onSetAutomationPoint,
    deleteAutomationPoint: onDeleteAutomationPoint,
    toggleAutomationLaneVisibility: onToggleAutomationLaneVisibility,
    importAudioFile: onImportAudio,
    markers: arrangementMarkers,
    addMarker: onAddMarker,
    removeMarker: onRemoveMarker,
    updateMarker: onUpdateMarker,
    seekToMarker: onSeekToMarker,
    groups,
    createGroup: onCreateGroup,
    removeGroup: onRemoveGroup,
    renameGroup: onRenameGroup,
    toggleGroupCollapsed: onToggleGroupCollapsed,
    setGroupMute: onSetGroupMute,
    setGroupSolo: onSetGroupSolo,
  } = arrangement;
  const { canUndo, canRedo, undo: onUndo, redo: onRedo } = history;
  const hasClipboard = arrangementEngine.hasClipboard();

  const [pxPerBeat, setPxPerBeat] = useState(DEFAULT_PX_PER_BEAT);
  const [snapValue, setSnapValue] = useState(1);
  const [tool, setTool] = useState<ArrangementTool>('pointer');
  const [selectedRegionIds, setSelectedRegionIds] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    trackId: string;
    regionId: string;
  } | null>(null);
  const [fxOpenTrackId, setFxOpenTrackId] = useState<string | null>(null);
  const [markerContextMenu, setMarkerContextMenu] = useState<{
    x: number;
    y: number;
    markerId: string;
  } | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lanesRef = useRef<HTMLDivElement>(null);
  const headerWidth = HEADER_WIDTH;

  // Track which track each selected region belongs to
  const selectedRegionTrackMap = useRef<Map<string, string>>(new Map());

  // Rubber-band selection state
  const [rubberBand, setRubberBand] = useState<{
    startX: number;
    startY: number;
    currentX: number;
    currentY: number;
  } | null>(null);
  const rubberBandStartRef = useRef<{ x: number; y: number; clientX: number; clientY: number } | null>(null);
  const isRubberBandActive = useRef(false);

  const handleSelectRegion = useCallback(
    (trackId: string, regionId: string, addToSelection: boolean) => {
      setSelectedRegionIds((prev) => {
        const next = new Set(prev);
        if (addToSelection) {
          if (next.has(regionId)) {
            next.delete(regionId);
            selectedRegionTrackMap.current.delete(regionId);
          } else {
            next.add(regionId);
            selectedRegionTrackMap.current.set(regionId, trackId);
          }
        } else {
          next.clear();
          selectedRegionTrackMap.current.clear();
          next.add(regionId);
          selectedRegionTrackMap.current.set(regionId, trackId);
        }
        return next;
      });
    },
    [],
  );

  const clearSelection = useCallback(() => {
    setSelectedRegionIds(new Set());
    selectedRegionTrackMap.current.clear();
  }, []);

  // Auto-scroll to follow playhead
  useEffect(() => {
    if (transportState !== 'playing' && transportState !== 'recording') return;
    const container = scrollRef.current;
    if (!container) return;

    const playheadX = beatToPx(position, pxPerBeat);
    const scrollLeft = container.scrollLeft;
    // The sticky track-header column covers the left part of the viewport.
    const viewWidth = container.clientWidth - headerWidth;

    if (playheadX > scrollLeft + viewWidth - 50 || playheadX < scrollLeft) {
      container.scrollLeft = playheadX - 100;
    }
  }, [position, pxPerBeat, transportState, headerWidth]);

  // Pinch-to-zoom (Ctrl+wheel / trackpad pinch)
  useEffect(() => {
    const container = scrollRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();

      const rect = container.getBoundingClientRect();
      const viewX = Math.max(0, e.clientX - rect.left - headerWidth);
      const cursorX = viewX + container.scrollLeft;
      const beatAtCursor = cursorX / pxPerBeat;

      const zoomDelta = -e.deltaY * 0.5;
      const newPxPerBeat = Math.round(
        Math.max(MIN_PX_PER_BEAT, Math.min(MAX_PX_PER_BEAT, pxPerBeat + zoomDelta))
      );

      if (newPxPerBeat !== pxPerBeat) {
        setPxPerBeat(newPxPerBeat);
        // Adjust scroll so the beat under the cursor stays in place
        const newCursorX = beatAtCursor * newPxPerBeat;
        container.scrollLeft = newCursorX - viewX;
      }
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [pxPerBeat, headerWidth]);

  // Keyboard shortcuts
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;

      // Ctrl+A — select all regions
      if ((e.metaKey || e.ctrlKey) && e.key === 'a') {
        e.preventDefault();
        const allIds = new Set<string>();
        selectedRegionTrackMap.current.clear();
        for (const track of tracks) {
          for (const region of track.regions) {
            allIds.add(region.id);
            selectedRegionTrackMap.current.set(region.id, track.id);
          }
        }
        setSelectedRegionIds(allIds);
        return;
      }

      if (e.metaKey || e.ctrlKey || e.altKey) return;

      if (e.key === 'Delete' || e.key === 'Backspace') {
        // Delete selected regions
        if (selectedRegionIds.size > 0) {
          for (const regionId of selectedRegionIds) {
            const trackId = selectedRegionTrackMap.current.get(regionId);
            if (trackId) onRemoveRegion(trackId, regionId);
          }
          clearSelection();
          return;
        }
        if (contextMenu) {
          onRemoveRegion(contextMenu.trackId, contextMenu.regionId);
          setContextMenu(null);
        }
        return;
      }

      // Escape — clear selection
      if (e.key === 'Escape') {
        clearSelection();
        return;
      }

      // Tool shortcuts
      if (e.key === 'v' || e.key === 'V') { setTool('pointer'); return; }
      if (e.key === 'c' || e.key === 'C') { setTool('cut'); return; }
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [contextMenu, onRemoveRegion, selectedRegionIds, clearSelection, tracks]);

  const handleRegionContextMenu = useCallback(
    (e: React.MouseEvent, trackId: string, regionId: string) => {
      setContextMenu({ x: e.clientX, y: e.clientY, trackId, regionId });
    },
    [],
  );

  const handleAddMarkerAtPlayhead = useCallback(() => {
    if (!onAddMarker) return;
    const existingCount = arrangementMarkers?.length ?? 0;
    const name = DEFAULT_MARKER_NAMES[existingCount % DEFAULT_MARKER_NAMES.length];
    onAddMarker(name, position);
  }, [onAddMarker, arrangementMarkers, position]);

  const handleMarkerClick = useCallback((id: string) => {
    onSeekToMarker?.(id);
  }, [onSeekToMarker]);

  const handleMarkerContextMenu = useCallback((e: React.MouseEvent, id: string) => {
    setMarkerContextMenu({ x: e.clientX, y: e.clientY, markerId: id });
  }, []);

  const handleMarkerDragEnd = useCallback((id: string, newBeat: number) => {
    onUpdateMarker?.(id, { beat: newBeat });
  }, [onUpdateMarker]);

  const handleCreateGroupFromSelection = useCallback(() => {
    if (!onCreateGroup) return;
    // Get unique track IDs from selected regions
    const trackIds = new Set<string>();
    for (const regionId of selectedRegionIds) {
      const tid = selectedRegionTrackMap.current.get(regionId);
      if (tid) trackIds.add(tid);
    }
    if (trackIds.size === 0) {
      // If no regions selected, group all tracks
      for (const track of tracks) {
        trackIds.add(track.id);
      }
    }
    if (trackIds.size > 0) {
      onCreateGroup('Group', [...trackIds]);
    }
  }, [onCreateGroup, selectedRegionIds, tracks]);

  // Build ordered track list with groups
  const collapsedTrackIds = new Set<string>();
  if (groups) {
    for (const group of groups) {
      if (group.collapsed) {
        for (const trackId of group.trackIds) {
          collapsedTrackIds.add(trackId);
        }
      }
    }
  }

  const trackHeights = tracks.map((t) => collapsedTrackIds.has(t.id) ? 0 : getTrackHeight(t));

  // --- Rubber-band selection handlers ---

  const handleLanesPointerDown = useCallback((e: React.PointerEvent) => {
    if (e.button !== 0 || !lanesRef.current) return;
    const rect = lanesRef.current.getBoundingClientRect();
    const contentX = e.clientX - rect.left;
    const contentY = e.clientY - rect.top;

    rubberBandStartRef.current = { x: contentX, y: contentY, clientX: e.clientX, clientY: e.clientY };
    isRubberBandActive.current = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }, []);

  const handleLanesPointerMove = useCallback((e: React.PointerEvent) => {
    if (!rubberBandStartRef.current || !lanesRef.current) return;

    const dx = e.clientX - rubberBandStartRef.current.clientX;
    const dy = e.clientY - rubberBandStartRef.current.clientY;

    if (!isRubberBandActive.current && (Math.abs(dx) > 3 || Math.abs(dy) > 3)) {
      isRubberBandActive.current = true;
    }

    if (isRubberBandActive.current) {
      const rect = lanesRef.current.getBoundingClientRect();
      const contentX = e.clientX - rect.left;
      const contentY = e.clientY - rect.top;
      setRubberBand({
        startX: rubberBandStartRef.current.x,
        startY: rubberBandStartRef.current.y,
        currentX: contentX,
        currentY: contentY,
      });
    }
  }, []);

  const handleLanesPointerUp = useCallback((e: React.PointerEvent) => {
    if (!rubberBandStartRef.current) return;

    if (isRubberBandActive.current && rubberBand) {
      // Select regions within rubber-band rectangle
      const x1 = Math.min(rubberBand.startX, rubberBand.currentX);
      const x2 = Math.max(rubberBand.startX, rubberBand.currentX);
      const y1 = Math.min(rubberBand.startY, rubberBand.currentY);
      const y2 = Math.max(rubberBand.startY, rubberBand.currentY);

      const newSelection = new Set<string>(e.shiftKey ? selectedRegionIds : []);
      if (!e.shiftKey) {
        selectedRegionTrackMap.current.clear();
      }

      let cumulativeTop = 0;
      tracks.forEach((track, trackIndex) => {
        const trackTop = cumulativeTop;
        const trackBottom = trackTop + trackHeights[trackIndex];

        // Check vertical overlap
        if (trackBottom <= y1 || trackTop >= y2) return;

        track.regions.forEach((region) => {
          const regionLeft = beatToPx(region.startBeat, pxPerBeat);
          const regionRight = beatToPx(region.startBeat + region.lengthBeats, pxPerBeat);

          // Check horizontal overlap
          if (regionRight > x1 && regionLeft < x2) {
            newSelection.add(region.id);
            selectedRegionTrackMap.current.set(region.id, track.id);
          }
        });
        cumulativeTop = trackBottom;
      });

      setSelectedRegionIds(newSelection);
    } else {
      // Simple click on empty space — clear selection
      if (!e.shiftKey) {
        clearSelection();
      }
    }

    setRubberBand(null);
    rubberBandStartRef.current = null;
    isRubberBandActive.current = false;
  }, [rubberBand, selectedRegionIds, tracks, trackHeights, pxPerBeat, clearSelection]);

  const contentWidth = beatToPx(lengthBeats, pxPerBeat);
  const fxTrack = fxOpenTrackId ? tracks.find((t) => t.id === fxOpenTrackId) ?? null : null;
  const isRolling = transportState === 'playing' || transportState === 'recording';

  const renderTrackHeader = (track: Track) => {
    const lanes = track.automation?.filter((l) => l.visible) ?? [];
    return (
      <div key={track.id}>
        <TrackHeader
          track={track}
          isArmed={armedTrackId === track.id}
          isRecording={recordingTrackId === track.id}
          onMuteToggle={() => onSetTrackMute(track.id, !track.muted)}
          onSoloToggle={() => onSetTrackSolo(track.id, !track.solo)}
          onVolumeChange={(db) => onSetTrackVolume(track.id, db)}
          onPanChange={onSetTrackPan ? (pan) => onSetTrackPan(track.id, pan) : undefined}
          onInstrumentChange={(inst) => onSetTrackInstrument(track.id, inst)}
          onDelete={() => onRemoveTrack(track.id)}
          onArmToggle={() => onArmTrack(armedTrackId === track.id ? null : track.id)}
          onFxToggle={() => setFxOpenTrackId((prev) => prev === track.id ? null : track.id)}
          fxOpen={fxOpenTrackId === track.id}
          onAddAutomationLane={onAddAutomationLane ? (param) => onAddAutomationLane(track.id, param) : undefined}
          onRemoveAutomationLane={onRemoveAutomationLane ? (param) => onRemoveAutomationLane(track.id, param) : undefined}
          onToggleAutomationLaneVisibility={onToggleAutomationLaneVisibility ? (param) => onToggleAutomationLaneVisibility(track.id, param) : undefined}
        />
        {/* One header cell per visible automation lane (same height as the lane) */}
        {lanes.map((lane) => (
          <div
            key={lane.parameter}
            className="relative flex items-start gap-1.5 border-b border-border bg-surface-1 pt-1.5 pr-2 pl-5"
            style={{ height: AUTOMATION_LANE_HEIGHT }}
          >
            <span aria-hidden className="absolute inset-y-0 left-0 w-1 opacity-40" style={{ backgroundColor: track.color }} />
            <span className="text-[10px] text-muted-foreground">↳</span>
            <span className="truncate text-[11px] font-medium text-muted-foreground">{getAutomationParameterLabel(lane.parameter)}</span>
          </div>
        ))}
      </div>
    );
  };

  const renderTrackLane = (track: Track) => (
    <TrackLane
      key={track.id}
      track={track}
      lengthBeats={lengthBeats}
      pxPerBeat={pxPerBeat}
      snapValue={snapValue}
      tool={tool}
      bpm={bpm}
      selectedRegionIds={selectedRegionIds}
      liveRegion={recordingTrackId === track.id ? liveRegion : null}
      onMoveRegion={onMoveRegion}
      onResizeRegion={onResizeRegion ? (tid, rid, newStart, newLen) => onResizeRegion(tid, rid, newStart, newLen) : undefined}
      onSplitRegion={onSplitRegion}
      onSelectRegion={handleSelectRegion}
      onRegionContextMenu={handleRegionContextMenu}
      onEditRegion={onEditRegion}
      onSetAutomationPoint={onSetAutomationPoint}
      onDeleteAutomationPoint={onDeleteAutomationPoint}
    />
  );

  // Render order shared by the header column and the lanes: groups first, then ungrouped tracks.
  const headerElements: React.ReactNode[] = [];
  const laneElements: React.ReactNode[] = [];
  {
    const rendered = new Set<string>();
    for (const group of groups ?? []) {
      const groupTracks = group.trackIds
        .map((id) => tracks.find((t) => t.id === id))
        .filter(Boolean) as Track[];
      if (groupTracks.length === 0) continue;
      headerElements.push(
        <GroupHeader
          key={`group-${group.id}`}
          group={group}
          allMuted={groupTracks.every((t) => t.muted)}
          anySolo={groupTracks.some((t) => t.solo)}
          onToggleCollapsed={onToggleGroupCollapsed ?? NOOP}
          onRename={onRenameGroup ?? NOOP}
          onMuteToggle={onSetGroupMute ?? NOOP}
          onSoloToggle={onSetGroupSolo ?? NOOP}
          onRemove={onRemoveGroup ?? NOOP}
        />,
      );
      laneElements.push(
        <div
          key={`group-lane-${group.id}`}
          className="h-7 border-b border-border"
          style={{ backgroundColor: `color-mix(in oklab, ${group.color} 6%, var(--surface-0))` }}
        />,
      );
      for (const track of groupTracks) {
        rendered.add(track.id);
        if (group.collapsed) continue;
        headerElements.push(renderTrackHeader(track));
        laneElements.push(renderTrackLane(track));
      }
    }
    for (const track of tracks) {
      if (rendered.has(track.id)) continue;
      headerElements.push(renderTrackHeader(track));
      laneElements.push(renderTrackLane(track));
    }
  }

  return (
    <div data-tour="arrangement" className="flex min-h-0 flex-1 flex-col overflow-hidden bg-surface-0">
      <ArrangementToolbar
        snapValue={snapValue}
        pxPerBeat={pxPerBeat}
        canUndo={canUndo}
        canRedo={canRedo}
        tool={tool}
        onSnapChange={setSnapValue}
        onZoomChange={setPxPerBeat}
        onAddSynthTrack={() => onAddTrack('synth')}
        onAddDrumTrack={() => onAddTrack('drums')}
        onImportAudio={onImportAudio}
        onUndo={onUndo}
        onRedo={onRedo}
        onExport={onExport}
        onToolChange={setTool}
        onAddMarker={handleAddMarkerAtPlayhead}
        onCreateGroup={handleCreateGroupFromSelection}
      />

      <div className="relative flex min-h-0 flex-1">
        {/* One scroll container: the header column sticks left, the ruler sticks top, so they never drift apart. */}
        <div ref={scrollRef} className="relative min-w-0 flex-1 overflow-auto">
          <div className="flex min-h-full" style={{ width: headerWidth + contentWidth }}>
            {/* Track headers */}
            <div className="sticky left-0 z-20 flex shrink-0 flex-col border-r border-border bg-surface-1 shadow-[2px_0_8px_-4px_oklch(0_0_0/0.5)]" style={{ width: headerWidth }}>
              <div
                className="sticky top-0 z-10 flex shrink-0 items-center justify-between border-b border-border bg-surface-1 px-3"
                style={{ height: RULER_HEIGHT }}
              >
                <span className="daw-label">Tracks</span>
                <span className="font-mono text-[10px] text-muted-foreground tabular-nums">{tracks.length}</span>
              </div>
              {headerElements}
              {tracks.length === 0 && (
                <div className="flex flex-col items-start gap-2 px-3 py-4">
                  <p className="text-[12px] font-semibold text-foreground">No tracks yet</p>
                  <p className="text-[11px] leading-relaxed text-muted-foreground">
                    Add a synth or drum track, arm it, and press R to record from your keyboard.
                  </p>
                  <div className="flex gap-1.5 pt-1">
                    <Button size="sm" onClick={() => onAddTrack('synth')}>
                      <Plus /> Synth track
                    </Button>
                    <Button size="sm" variant="outline" onClick={() => onAddTrack('drums')}>
                      <Plus /> Drums
                    </Button>
                  </div>
                </div>
              )}
              {/* Filler so the column's background reaches the bottom */}
              <div className="flex-1" />
            </div>

            {/* Timeline + lanes */}
            <div
              className="relative z-0 flex shrink-0 flex-col"
              style={{ width: contentWidth }}
              onClick={clearSelection}
            >
              <div className="sticky top-0 z-[25]">
                <TimelineRuler
                  lengthBeats={lengthBeats}
                  pxPerBeat={pxPerBeat}
                  snapValue={snapValue}
                  loopEnabled={loopEnabled}
                  loopMarkers={loopMarkers}
                  markers={arrangementMarkers}
                  onSeek={(beat) => arrangementEngine.setPosition(beat)}
                  onLoopMarkersChange={onLoopMarkersChange}
                  onMarkerClick={handleMarkerClick}
                  onMarkerContextMenu={handleMarkerContextMenu}
                  onMarkerDragEnd={handleMarkerDragEnd}
                />
              </div>

              {/* Track lanes with rubber-band selection */}
              <div
                ref={lanesRef}
                className="relative"
                onClick={(e) => e.stopPropagation()}
                onPointerDown={handleLanesPointerDown}
                onPointerMove={handleLanesPointerMove}
                onPointerUp={handleLanesPointerUp}
              >
                {laneElements}

                {/* Rubber-band selection rectangle */}
                {rubberBand && (
                  <div
                    className="pointer-events-none absolute z-20 rounded-[2px] border border-signal-select/80 bg-signal-select/10"
                    style={{
                      left: Math.min(rubberBand.startX, rubberBand.currentX),
                      top: Math.min(rubberBand.startY, rubberBand.currentY),
                      width: Math.abs(rubberBand.currentX - rubberBand.startX),
                      height: Math.abs(rubberBand.currentY - rubberBand.startY),
                    }}
                  />
                )}
              </div>

              {/* Empty grid below the last track */}
              <div className="min-h-16 flex-1" style={laneGridStyle(pxPerBeat)} />

              {/* Playhead (a quieter cue line while stopped) */}
              <Playhead
                position={position}
                pxPerBeat={pxPerBeat}
                height="100%"
                idle={!isRolling}
                recording={transportState === 'recording'}
              />
            </div>
          </div>
        </div>

        {/* Per-track effects drawer */}
        {fxTrack && (
          <div className="absolute inset-y-0 right-0 z-40">
            <TrackEffectsPanel key={fxTrack.id} track={fxTrack} onClose={() => setFxOpenTrackId(null)} />
          </div>
        )}
      </div>

      {/* Context menu */}
      {contextMenu && (
        <RegionContextMenu
          x={contextMenu.x}
          y={contextMenu.y}
          onSplit={() => onSplitRegion(contextMenu.trackId, contextMenu.regionId, position)}
          onDuplicate={() => onDuplicateRegion(contextMenu.trackId, contextMenu.regionId)}
          onDelete={() => onRemoveRegion(contextMenu.trackId, contextMenu.regionId)}
          onClose={() => setContextMenu(null)}
          onCopy={onCopyRegion ? () => onCopyRegion(contextMenu.trackId, contextMenu.regionId) : undefined}
          onPaste={onPasteRegion ? () => onPasteRegion(contextMenu.trackId, position) : undefined}
          canPaste={hasClipboard}
          onQuantize={onQuantizeRegion ? () => onQuantizeRegion(contextMenu.trackId, contextMenu.regionId, snapValue) : undefined}
          isAudioRegion={(() => {
            const t = tracks.find((tr) => tr.id === contextMenu.trackId);
            const r = t?.regions.find((rg) => rg.id === contextMenu.regionId);
            return !!r?.audio;
          })()}
        />
      )}

      {/* Marker context menu */}
      {markerContextMenu && arrangementMarkers && (
        <MarkerContextMenu
          x={markerContextMenu.x}
          y={markerContextMenu.y}
          marker={arrangementMarkers.find((m) => m.id === markerContextMenu.markerId)!}
          onRename={(id, name) => onUpdateMarker?.(id, { name })}
          onChangeColor={(id, color) => onUpdateMarker?.(id, { color })}
          onDelete={(id) => { onRemoveMarker?.(id); setMarkerContextMenu(null); }}
          onClose={() => setMarkerContextMenu(null)}
        />
      )}
    </div>
  );
}
