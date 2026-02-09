import { useMemo } from 'react';
import type { Track, Region, AutomationParameter } from '@/types/arrangement';
import type { ArrangementTool } from './ArrangementToolbar';
import { RegionBlock } from './RegionBlock';
import { LiveRegionBlock } from './LiveRegionBlock';
import { AutomationLaneView, AUTOMATION_LANE_HEIGHT } from './AutomationLaneView';
import { beatToPx } from '@/utils/arrangementHelpers';

const NOOP = () => {};

interface TrackLaneProps {
  track: Track;
  lengthBeats: number;
  pxPerBeat: number;
  snapValue: number;
  tool: ArrangementTool;
  bpm?: number;
  selectedRegionIds?: Set<string>;
  liveRegion?: Region | null;
  onMoveRegion: (trackId: string, regionId: string, newStartBeat: number) => void;
  onResizeRegion?: (trackId: string, regionId: string, newStartBeat: number, newLengthBeats: number) => void;
  onSplitRegion: (trackId: string, regionId: string, splitBeat: number) => void;
  onSelectRegion?: (trackId: string, regionId: string, addToSelection: boolean) => void;
  onRegionContextMenu: (e: React.MouseEvent, trackId: string, regionId: string) => void;
  onEditRegion?: (trackId: string, regionId: string) => void;
  onSetAutomationPoint?: (trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number) => void;
  onDeleteAutomationPoint?: (trackId: string, parameter: AutomationParameter, pointIndex: number) => void;
}

export function TrackLane({
  track,
  lengthBeats,
  pxPerBeat,
  snapValue,
  bpm,
  selectedRegionIds,
  liveRegion,
  onMoveRegion,
  onResizeRegion,
  onSplitRegion,
  onSelectRegion,
  onRegionContextMenu,
  onEditRegion,
  onSetAutomationPoint,
  onDeleteAutomationPoint,
  tool,
}: TrackLaneProps) {
  const totalWidth = beatToPx(lengthBeats, pxPerBeat);

  const gridLines = useMemo(() => {
    const lines: number[] = [];
    for (let beat = 0; beat <= lengthBeats; beat++) {
      if (beat % 4 === 0) lines.push(beat);
    }
    return lines;
  }, [lengthBeats]);

  const visibleLanes = track.automation?.filter((l) => l.visible) ?? [];

  return (
    <div style={{ width: totalWidth }}>
      {/* Region area */}
      <div
        className="relative h-16 border-b border-border bg-background/50"
        style={{ width: totalWidth }}
      >
        {/* Grid lines */}
        {gridLines.map((beat) => (
          <div
            key={beat}
            className="absolute top-0 h-full w-px bg-border/30"
            style={{ left: beatToPx(beat, pxPerBeat) }}
          />
        ))}

        {/* Regions */}
        {track.regions.map((region) => (
          <RegionBlock
            key={region.id}
            region={region}
            trackColor={track.color}
            pxPerBeat={pxPerBeat}
            snapValue={snapValue}
            tool={tool}
            bpm={bpm}
            selected={selectedRegionIds?.has(region.id) ?? false}
            onMove={(regionId, newStartBeat) => onMoveRegion(track.id, regionId, newStartBeat)}
            onResize={onResizeRegion ? (regionId, newStart, newLen) => onResizeRegion(track.id, regionId, newStart, newLen) : undefined}
            onCut={(regionId, splitBeat) => onSplitRegion(track.id, regionId, splitBeat)}
            onSelect={onSelectRegion ? (regionId, add) => onSelectRegion(track.id, regionId, add) : undefined}
            onContextMenu={(e, regionId) => onRegionContextMenu(e, track.id, regionId)}
            onDoubleClick={onEditRegion ? (regionId) => onEditRegion(track.id, regionId) : undefined}
          />
        ))}

        {/* Live recording region */}
        {liveRegion && (
          <LiveRegionBlock
            region={liveRegion}
            trackColor={track.color}
            pxPerBeat={pxPerBeat}
          />
        )}
      </div>

      {/* Automation lanes */}
      {visibleLanes.map((lane) => (
        <AutomationLaneView
          key={lane.parameter}
          lane={lane}
          trackId={track.id}
          lengthBeats={lengthBeats}
          pxPerBeat={pxPerBeat}
          snapValue={snapValue}
          trackColor={track.color}
          onSetPoint={onSetAutomationPoint ?? NOOP}
          onDeletePoint={onDeleteAutomationPoint ?? NOOP}
        />
      ))}
    </div>
  );
}

export function getTrackHeight(track: Track): number {
  const visibleLanes = track.automation?.filter((l) => l.visible)?.length ?? 0;
  return 64 + visibleLanes * AUTOMATION_LANE_HEIGHT;
}
