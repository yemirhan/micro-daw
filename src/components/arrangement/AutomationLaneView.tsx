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
      className="relative border-b border-border bg-surface-0"
      style={{ height: LANE_HEIGHT, width: totalWidth }}
      onPointerDown={stopPropagation}
    >
      {/* Label */}
      <span
        className="pointer-events-none absolute top-1 left-1.5 z-10 text-[9.5px] font-bold uppercase tracking-[0.08em]"
        style={{ color: `color-mix(in oklab, ${trackColor} 70%, white)` }}
      >
        {getAutomationParameterLabel(lane.parameter)}
      </span>

      {/* Grid lines */}
      <div className="pointer-events-none absolute inset-0 text-foreground" style={{ opacity: 0.06 }}>
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
        {/* Area under the curve */}
        {lane.points.length > 1 && (
          <path
            d={`${pathData} L ${beatToPx(lane.points[lane.points.length - 1].beat, pxPerBeat)} ${LANE_HEIGHT} L ${beatToPx(lane.points[0].beat, pxPerBeat)} ${LANE_HEIGHT} Z`}
            fill={trackColor}
            fillOpacity={0.12}
            stroke="none"
          />
        )}

        {/* Automation line */}
        {lane.points.length > 1 && (
          <path
            d={pathData}
            fill="none"
            stroke={trackColor}
            strokeWidth={1.5}
            strokeOpacity={0.9}
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
              r={3.5}
              fill={trackColor}
              stroke="var(--surface-0)"
              strokeWidth={1.5}
              className="cursor-pointer"
              onContextMenu={(e) => handlePointContextMenu(e as unknown as React.MouseEvent, i)}
            />
          );
        })}
      </svg>
    </div>
  );
}

export { LANE_HEIGHT as AUTOMATION_LANE_HEIGHT };
