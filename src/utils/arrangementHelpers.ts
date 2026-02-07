import type { RegionNote } from '@/types/arrangement';
import type { RecordedEvent } from '@/types/recorder';

export function beatToPx(beat: number, pxPerBeat: number): number {
  return beat * pxPerBeat;
}

export function pxToBeat(px: number, pxPerBeat: number): number {
  return px / pxPerBeat;
}

export function snapBeat(beat: number, snapValue: number): number {
  if (snapValue <= 0) return beat;
  return Math.round(beat / snapValue) * snapValue;
}

export function msToBeats(ms: number, bpm: number): number {
  return (ms / 60000) * bpm;
}

export function beatsToMs(beats: number, bpm: number): number {
  return (beats / bpm) * 60000;
}

export function formatBeatPosition(beat: number): string {
  const bar = Math.floor(beat / 4) + 1;
  const beatInBar = Math.floor(beat % 4) + 1;
  return `${bar}.${beatInBar}`;
}

/** Convert legacy ms-based recording events to beat-based RegionNotes */
export function recordingEventsToRegionNotes(events: RecordedEvent[], bpm: number): RegionNote[] {
  const notes: RegionNote[] = [];
  const openNotes = new Map<number, { startBeat: number; velocity: number; isDrum: boolean }>();

  for (const event of events) {
    const beatTime = msToBeats(event.time, bpm);

    if (event.type === 'noteOn') {
      openNotes.set(event.note, {
        startBeat: beatTime,
        velocity: event.velocity,
        isDrum: event.isDrum,
      });
    } else {
      const open = openNotes.get(event.note);
      if (open) {
        notes.push({
          note: event.note,
          velocity: open.velocity,
          startBeat: open.startBeat,
          durationBeats: Math.max(0.125, beatTime - open.startBeat),
          isDrum: open.isDrum,
        });
        openNotes.delete(event.note);
      }
    }
  }

  // Close any remaining open notes with default duration
  for (const [note, open] of openNotes) {
    notes.push({
      note,
      velocity: open.velocity,
      startBeat: open.startBeat,
      durationBeats: open.isDrum ? 0.25 : 0.5,
      isDrum: open.isDrum,
    });
  }

  return notes;
}
