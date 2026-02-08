import type { AutomationPoint, AutomationParameter } from '@/types/arrangement';

/** Linear interpolation between automation points at a given beat */
export function getAutomationValue(points: AutomationPoint[], beat: number): number {
  if (points.length === 0) return 0.5;
  if (points.length === 1) return points[0].value;

  // Before first point
  if (beat <= points[0].beat) return points[0].value;
  // After last point
  if (beat >= points[points.length - 1].beat) return points[points.length - 1].value;

  // Find surrounding points
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i];
    const b = points[i + 1];
    if (beat >= a.beat && beat <= b.beat) {
      const t = (beat - a.beat) / (b.beat - a.beat);
      return a.value + t * (b.value - a.value);
    }
  }

  return points[points.length - 1].value;
}

/** Map normalized 0-1 value to real parameter range */
export function mapAutomationValue(param: AutomationParameter, normalized: number): number {
  switch (param) {
    case 'volume':
      return -40 + normalized * 40; // -40 to 0 dB
    case 'pan':
      return -1 + normalized * 2; // -1 to 1
    case 'filterCutoff':
      return 60 + normalized * (18000 - 60); // 60 to 18000 Hz
    case 'eqLow':
    case 'eqMid':
    case 'eqHigh':
      return -12 + normalized * 24; // -12 to 12 dB
    default:
      return normalized; // 0-1 for wet/depth params
  }
}

/** Insert or update a point at the given beat, maintaining sort order */
export function setAutomationPoint(
  points: AutomationPoint[],
  beat: number,
  value: number,
  snapValue: number,
): AutomationPoint[] {
  const snapped = snapValue > 0 ? Math.round(beat / snapValue) * snapValue : beat;
  const clamped = Math.max(0, Math.min(1, value));

  const newPoints = [...points];
  const existing = newPoints.findIndex((p) => Math.abs(p.beat - snapped) < 0.001);

  if (existing >= 0) {
    newPoints[existing] = { beat: snapped, value: clamped };
  } else {
    newPoints.push({ beat: snapped, value: clamped });
    newPoints.sort((a, b) => a.beat - b.beat);
  }

  return newPoints;
}

/** Delete a point by index */
export function deleteAutomationPoint(points: AutomationPoint[], index: number): AutomationPoint[] {
  return points.filter((_, i) => i !== index);
}

/** Get display name for automation parameter */
export function getAutomationParameterLabel(param: AutomationParameter): string {
  switch (param) {
    case 'volume': return 'Volume';
    case 'pan': return 'Pan';
    case 'reverbWet': return 'Reverb';
    case 'delayWet': return 'Delay';
    case 'chorusDepth': return 'Chorus';
    case 'distortionWet': return 'Distortion';
    case 'filterCutoff': return 'Filter';
    case 'eqLow': return 'EQ Low';
    case 'eqMid': return 'EQ Mid';
    case 'eqHigh': return 'EQ High';
  }
}
