import { useState, useRef, useCallback, useEffect } from 'react';
import { ArrangementToolbar } from './ArrangementToolbar';
import { TimelineRuler } from './TimelineRuler';
import { TrackHeader } from './TrackHeader';
import { TrackLane, getTrackHeight } from './TrackLane';
import { Playhead } from './Playhead';
import { RegionContextMenu } from './RegionContextMenu';
import { MarkerContextMenu } from './MarkerContextMenu';
import { GroupHeader } from './GroupHeader';
import type { Track, TrackInstrument, Region, LoopMarkers, Marker, TrackGroup, AutomationParameter } from '@/types/arrangement';
import type { ArrangementTransportState } from '@/types/arrangement';
import { DEFAULT_PX_PER_BEAT, MIN_PX_PER_BEAT, MAX_PX_PER_BEAT, DEFAULT_MARKER_NAMES } from '@/utils/constants';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { beatToPx } from '@/utils/arrangementHelpers';
import type { ArrangementTool } from './ArrangementToolbar';

const NOOP = () => {};

interface ArrangementViewProps {
  tracks: Track[];
  transportState: ArrangementTransportState;
  position: number;
  lengthBeats: number;
  recordingTrackId: string | null;
  armedTrackId: string | null;
  liveRegion: Region | null;
  bpm: number;
  onAddTrack: (type: 'synth' | 'drums' | 'audio', presetIndex?: number) => void;
  onRemoveTrack: (trackId: string) => void;
  onSetTrackInstrument: (trackId: string, instrument: TrackInstrument) => void;
  onSetTrackVolume: (trackId: string, db: number) => void;
  onSetTrackPan?: (trackId: string, pan: number) => void;
  onSetTrackMute: (trackId: string, muted: boolean) => void;
  onSetTrackSolo: (trackId: string, solo: boolean) => void;
  onMoveRegion: (trackId: string, regionId: string, newStartBeat: number) => void;
  onResizeRegion?: (trackId: string, regionId: string, newStartBeat?: number, newLengthBeats?: number) => void;
  onRemoveRegion: (trackId: string, regionId: string) => void;
  onSplitRegion: (trackId: string, regionId: string, splitBeat: number) => void;
  onDuplicateRegion: (trackId: string, regionId: string) => void;
  onArmTrack: (trackId: string | null) => void;
  canUndo: boolean;
  canRedo: boolean;
  onUndo: () => void;
  onRedo: () => void;
  onEditRegion?: (trackId: string, regionId: string) => void;
  onCopyRegion?: (trackId: string, regionId: string) => void;
  onPasteRegion?: (trackId: string, atBeat: number) => void;
  hasClipboard?: boolean;
  onExport: () => void;
  loopEnabled?: boolean;
  loopMarkers?: LoopMarkers;
  onLoopMarkersChange?: (startBeat: number, endBeat: number) => void;
  onQuantizeRegion?: (trackId: string, regionId: string, snapValue: number) => void;
  onAddAutomationLane?: (trackId: string, parameter: AutomationParameter) => void;
  onRemoveAutomationLane?: (trackId: string, parameter: AutomationParameter) => void;
  onSetAutomationPoint?: (trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number) => void;
  onDeleteAutomationPoint?: (trackId: string, parameter: AutomationParameter, pointIndex: number) => void;
  onToggleAutomationLaneVisibility?: (trackId: string, parameter: AutomationParameter) => void;
  onImportAudio?: () => void;
  // Markers
  markers?: Marker[];
  onAddMarker?: (name: string, beat: number, color?: string) => void;
  onRemoveMarker?: (id: string) => void;
  onUpdateMarker?: (id: string, updates: Partial<Pick<Marker, 'name' | 'beat' | 'color'>>) => void;
  onSeekToMarker?: (id: string) => void;
  // Groups
  groups?: TrackGroup[];
  onCreateGroup?: (name: string, trackIds: string[]) => void;
  onRemoveGroup?: (id: string) => void;
  onRenameGroup?: (id: string, name: string) => void;
  onToggleGroupCollapsed?: (id: string) => void;
  onSetGroupMute?: (groupId: string, muted: boolean) => void;
  onSetGroupSolo?: (groupId: string, solo: boolean) => void;
}

export function ArrangementView({
  bpm,
  tracks,
  transportState,
  position,
  lengthBeats,
  recordingTrackId,
  armedTrackId,
  liveRegion,
  onAddTrack,
  onRemoveTrack,
  onSetTrackInstrument,
  onSetTrackVolume,
  onSetTrackPan,
  onSetTrackMute,
  onSetTrackSolo,
  onMoveRegion,
  onResizeRegion,
  onRemoveRegion,
  onSplitRegion,
  onDuplicateRegion,
  onArmTrack,
  canUndo,
  canRedo,
  onUndo,
  onRedo,
  onEditRegion,
  onCopyRegion,
  onPasteRegion,
  hasClipboard,
  onExport,
  loopEnabled,
  loopMarkers,
  onLoopMarkersChange,
  onQuantizeRegion,
  onAddAutomationLane,
  onRemoveAutomationLane,
  onSetAutomationPoint,
  onDeleteAutomationPoint,
  onToggleAutomationLaneVisibility,
  onImportAudio,
  markers: arrangementMarkers,
  onAddMarker,
  onRemoveMarker,
  onUpdateMarker,
  onSeekToMarker,
  groups,
  onCreateGroup,
  onRemoveGroup,
  onRenameGroup,
  onToggleGroupCollapsed,
  onSetGroupMute,
  onSetGroupSolo,
}: ArrangementViewProps) {
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
  const [markerContextMenu, setMarkerContextMenu] = useState<{
    x: number;
    y: number;
    markerId: string;
  } | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const lanesRef = useRef<HTMLDivElement>(null);
  const headerWidth = 200;

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
    const viewWidth = container.clientWidth;

    if (playheadX > scrollLeft + viewWidth - 50 || playheadX < scrollLeft) {
      container.scrollLeft = playheadX - 100;
    }
  }, [position, pxPerBeat, transportState]);

  // Pinch-to-zoom (Ctrl+wheel / trackpad pinch)
  useEffect(() => {
    const container = scrollRef.current;
    if (!container) return;

    const handleWheel = (e: WheelEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      e.preventDefault();

      const rect = container.getBoundingClientRect();
      const cursorX = e.clientX - rect.left + container.scrollLeft;
      const beatAtCursor = cursorX / pxPerBeat;

      const zoomDelta = -e.deltaY * 0.5;
      const newPxPerBeat = Math.round(
        Math.max(MIN_PX_PER_BEAT, Math.min(MAX_PX_PER_BEAT, pxPerBeat + zoomDelta))
      );

      if (newPxPerBeat !== pxPerBeat) {
        setPxPerBeat(newPxPerBeat);
        // Adjust scroll so the beat under the cursor stays in place
        const newCursorX = beatAtCursor * newPxPerBeat;
        container.scrollLeft = newCursorX - (e.clientX - rect.left);
      }
    };

    container.addEventListener('wheel', handleWheel, { passive: false });
    return () => container.removeEventListener('wheel', handleWheel);
  }, [pxPerBeat]);

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
  }, [rubberBand, selectedRegionIds, tracks, pxPerBeat, clearSelection]);

  // Calculate total height including group headers and accounting for collapsed groups
  const visibleTrackIds = new Set<string>(tracks.map((t) => t.id));
  if (groups) {
    for (const group of groups) {
      if (group.collapsed) {
        for (const trackId of group.trackIds) {
          visibleTrackIds.delete(trackId);
        }
      }
    }
  }
  const trackHeights = tracks.map((t) => collapsedTrackIds.has(t.id) ? 0 : getTrackHeight(t));
  const groupHeadersHeight = groups ? groups.filter((g) => {
    return g.trackIds.some((id) => tracks.some((t) => t.id === id));
  }).length * 28 : 0;
  const totalHeight = trackHeights.reduce((sum, h) => sum + h, 0) + groupHeadersHeight;

  return (
    <div data-tour="arrangement" className="flex flex-1 flex-col overflow-hidden">
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
        onAddMarker={onAddMarker ? handleAddMarkerAtPlayhead : undefined}
        onCreateGroup={onCreateGroup ? handleCreateGroupFromSelection : undefined}
      />

      <div className="flex flex-1 overflow-hidden">
        {/* Track headers */}
        <div className="shrink-0 overflow-y-auto border-r border-border" style={{ width: headerWidth }}>
          {/* Ruler spacer */}
          <div className="h-6 border-b border-border" />
          {(() => {
            // Build render order: groups first, then ungrouped tracks
            const renderedTrackIds = new Set<string>();
            const elements: React.ReactNode[] = [];

            if (groups && groups.length > 0) {
              for (const group of groups) {
                const groupTracks = group.trackIds
                  .map((id) => tracks.find((t) => t.id === id))
                  .filter(Boolean) as Track[];
                if (groupTracks.length === 0) continue;

                const allMuted = groupTracks.every((t) => t.muted);
                const anySolo = groupTracks.some((t) => t.solo);

                elements.push(
                  <GroupHeader
                    key={`group-${group.id}`}
                    group={group}
                    allMuted={allMuted}
                    anySolo={anySolo}
                    onToggleCollapsed={onToggleGroupCollapsed ?? NOOP}
                    onRename={onRenameGroup ?? NOOP}
                    onMuteToggle={onSetGroupMute ?? NOOP}
                    onSoloToggle={onSetGroupSolo ?? NOOP}
                    onRemove={onRemoveGroup ?? NOOP}
                  />
                );

                if (!group.collapsed) {
                  for (const track of groupTracks) {
                    renderedTrackIds.add(track.id);
                    const extraHeight = getTrackHeight(track) - 64;
                    elements.push(
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
                          onAddAutomationLane={onAddAutomationLane ? (param) => onAddAutomationLane(track.id, param) : undefined}
                          onRemoveAutomationLane={onRemoveAutomationLane ? (param) => onRemoveAutomationLane(track.id, param) : undefined}
                          onToggleAutomationLaneVisibility={onToggleAutomationLaneVisibility ? (param) => onToggleAutomationLaneVisibility(track.id, param) : undefined}
                        />
                        {extraHeight > 0 && (
                          <div className="border-b border-border/30" style={{ height: extraHeight }} />
                        )}
                      </div>
                    );
                  }
                } else {
                  for (const track of groupTracks) {
                    renderedTrackIds.add(track.id);
                  }
                }
              }
            }

            // Ungrouped tracks
            for (const track of tracks) {
              if (renderedTrackIds.has(track.id)) continue;
              const extraHeight = getTrackHeight(track) - 64;
              elements.push(
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
                    onAddAutomationLane={onAddAutomationLane ? (param) => onAddAutomationLane(track.id, param) : undefined}
                    onRemoveAutomationLane={onRemoveAutomationLane ? (param) => onRemoveAutomationLane(track.id, param) : undefined}
                    onToggleAutomationLaneVisibility={onToggleAutomationLaneVisibility ? (param) => onToggleAutomationLaneVisibility(track.id, param) : undefined}
                  />
                  {extraHeight > 0 && (
                    <div className="border-b border-border/30" style={{ height: extraHeight }} />
                  )}
                </div>
              );
            }

            return elements;
          })()}
          {tracks.length === 0 && (
            <div className="flex h-32 items-center justify-center text-xs text-muted-foreground">
              Add a track to begin
            </div>
          )}
        </div>

        {/* Timeline + Lanes */}
        <div ref={scrollRef} className="flex-1 overflow-auto" onClick={clearSelection}>
          <div className="relative" style={{ width: beatToPx(lengthBeats, pxPerBeat) }}>
            {/* Ruler */}
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

            {/* Track lanes with rubber-band selection */}
            <div
              ref={lanesRef}
              className="relative"
              onClick={(e) => e.stopPropagation()}
              onPointerDown={handleLanesPointerDown}
              onPointerMove={handleLanesPointerMove}
              onPointerUp={handleLanesPointerUp}
            >
              {/* Group header spacers + track lanes in same order as headers */}
              {(() => {
                const laneElements: React.ReactNode[] = [];
                const renderedLaneIds = new Set<string>();

                if (groups && groups.length > 0) {
                  for (const group of groups) {
                    const groupTracks = group.trackIds
                      .map((id) => tracks.find((t) => t.id === id))
                      .filter(Boolean) as Track[];
                    if (groupTracks.length === 0) continue;

                    // Group header spacer in lanes
                    laneElements.push(
                      <div key={`group-lane-${group.id}`} className="h-7 border-b border-border/30" />
                    );

                    if (!group.collapsed) {
                      for (const track of groupTracks) {
                        renderedLaneIds.add(track.id);
                        laneElements.push(
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
                      }
                    } else {
                      for (const track of groupTracks) {
                        renderedLaneIds.add(track.id);
                      }
                    }
                  }
                }

                // Ungrouped tracks
                for (const track of tracks) {
                  if (renderedLaneIds.has(track.id)) continue;
                  laneElements.push(
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
                }

                return laneElements;
              })()}

              {/* Rubber-band selection rectangle */}
              {rubberBand && (
                <div
                  className="absolute pointer-events-none rounded-sm border border-primary/60 z-20"
                  style={{
                    left: Math.min(rubberBand.startX, rubberBand.currentX),
                    top: Math.min(rubberBand.startY, rubberBand.currentY),
                    width: Math.abs(rubberBand.currentX - rubberBand.startX),
                    height: Math.abs(rubberBand.currentY - rubberBand.startY),
                    backgroundColor: 'oklch(0.60 0.15 265 / 0.15)',
                  }}
                />
              )}
            </div>

            {/* Playhead */}
            {(transportState === 'playing' || transportState === 'recording') && (
              <Playhead
                position={position}
                pxPerBeat={pxPerBeat}
                height={24 + totalHeight}
              />
            )}
          </div>
        </div>
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
