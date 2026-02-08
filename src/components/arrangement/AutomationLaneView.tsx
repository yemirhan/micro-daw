import { useCallback, useRef } from 'react';
import type { AutomationLane, AutomationParameter } from '@/types/arrangement';
import { beatToPx, pxToBeat, snapBeat } from '@/utils/arrangementHelpers';
import { getAutomationParameterLabel } from '@/utils/automationHelpers';

interface AutomationLaneViewProps {
  lane: AutomationLane;
  trackId: string;
  lengthBeats: number;
  pxPerBeat: number;
  snapValue: number;
  trackColor: string;
  onSetPoint: (trackId: string, parameter: AutomationParameter, beat: number, value: number, snapValue: number) => void;
  onDeletePoint: (trackId: string, parameter: AutomationParameter, pointIndex: number) => void;
}

const LANE_HEIGHT = 48;

export function AutomationLaneView({
  lane,
  trackId,
  lengthBeats,
  pxPerBeat,
  snapValue,
  trackColor,
  onSetPoint,
  onDeletePoint,
}: AutomationLaneViewProps) {
  const totalWidth = beatToPx(lengthBeats, pxPerBeat);
  const svgRef = useRef<SVGSVGElement>(null);

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      if (!svgRef.current) return;
      const rect = svgRef.current.getBoundingClientRect();
      const x = e.clientX - rect.left;
      const y = e.clientY - rect.top;
      const beat = snapBeat(pxToBeat(x, pxPerBeat), snapValue);
      const value = 1 - y / LANE_HEIGHT; // top = 1, bottom = 0
      onSetPoint(trackId, lane.parameter, beat, Math.max(0, Math.min(1, value)), snapValue);
    },
    [trackId, lane.parameter, pxPerBeat, snapValue, onSetPoint],
  );

  // Stop propagation so the rubber-band selection handler in ArrangementView
  // doesn't steal pointer capture from this lane
  const stopPropagation = useCallback((e: React.PointerEvent) => {
    e.stopPropagation();
  }, []);

  const handlePointContextMenu = useCallback(
    (e: React.MouseEvent, index: number) => {
      e.preventDefault();
      e.stopPropagation();
      onDeletePoint(trackId, lane.parameter, index);
    },
    [trackId, lane.parameter, onDeletePoint],
  );

  // Build SVG path
  const pathData = lane.points
    .map((pt, i) => {
      const x = beatToPx(pt.beat, pxPerBeat);
      const y = (1 - pt.value) * LANE_HEIGHT;
      return `${i === 0 ? 'M' : 'L'} ${x} ${y}`;
    })
    .join(' ');

  return (
    <div
      className="relative border-b border-border/30"
      style={{ height: LANE_HEIGHT, width: totalWidth }}
      onPointerDown={stopPropagation}
    >
      {/* Label */}
      <span
        className="absolute left-1 top-0.5 text-[9px] font-semibold uppercase tracking-wider z-10 pointer-events-none"
        style={{ color: trackColor }}
      >
        {getAutomationParameterLabel(lane.parameter)}
      </span>

      {/* Grid lines */}
      <div className="absolute inset-0 pointer-events-none" style={{ opacity: 0.15 }}>
        <div className="absolute w-full" style={{ top: LANE_HEIGHT * 0.25, height: 1, backgroundColor: 'currentColor' }} />
        <div className="absolute w-full" style={{ top: LANE_HEIGHT * 0.5, height: 1, backgroundColor: 'currentColor' }} />
        <div className="absolute w-full" style={{ top: LANE_HEIGHT * 0.75, height: 1, backgroundColor: 'currentColor' }} />
      </div>

      <svg
        ref={svgRef}
        className="absolute inset-0 cursor-crosshair"
        width={totalWidth}
        height={LANE_HEIGHT}
        onClick={handleClick}
      >
        {/* Automation line */}
        {lane.points.length > 1 && (
          <path
            d={pathData}
            fill="none"
            stroke={trackColor}
            strokeWidth={1.5}
            strokeOpacity={0.8}
          />
        )}

        {/* Points */}
        {lane.points.map((pt, i) => {
          const cx = beatToPx(pt.beat, pxPerBeat);
          const cy = (1 - pt.value) * LANE_HEIGHT;
          return (
            <circle
              key={i}
              cx={cx}
              cy={cy}
              r={4}
              fill={trackColor}
              stroke="oklch(0.15 0.01 270)"
              strokeWidth={1.5}
              className="cursor-pointer hover:r-[6]"
              onContextMenu={(e) => handlePointContextMenu(e as unknown as React.MouseEvent, i)}
            />
          );
        })}
      </svg>
    </div>
  );
}

export { LANE_HEIGHT as AUTOMATION_LANE_HEIGHT };
