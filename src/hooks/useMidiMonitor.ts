import { useCallback, useEffect, useRef, useState } from 'react';
import { midiService } from '@/services/MidiService';
import { midiToNoteName } from '@/utils/noteHelpers';
import type { MidiMonitorEvent } from '@/types/midi';

const MAX_EVENTS = 500;
const FLUSH_INTERVAL = 50;

// Well-known CC names
const CC_NAMES: Record<number, string> = {
  1: 'Mod Wheel',
  2: 'Breath',
  7: 'Volume',
  10: 'Pan',
  11: 'Expression',
  64: 'Sustain',
  65: 'Portamento',
  66: 'Sostenuto',
  67: 'Soft Pedal',
  71: 'Resonance',
  74: 'Cutoff',
  91: 'Reverb',
  93: 'Chorus',
};

type FilterType = 'all' | 'noteon' | 'noteoff' | 'controlchange' | 'pitchbend' | 'aftertouch' | 'programchange';

function formatEvent(
  id: number,
  statusByte: number,
  dataBytes: number[],
  rawData: number[],
  timestamp: number,
): MidiMonitorEvent {
  const channel = (statusByte & 0x0F) + 1;
  const command = statusByte & 0xF0;

  let type = 'unknown';
  let label = 'Unknown';
  let detail = '';

  switch (command) {
    case 0x90: {
      const velocity = dataBytes[1] ?? 0;
      if (velocity === 0) {
        type = 'noteoff';
        label = 'Note Off';
        detail = `${midiToNoteName(dataBytes[0])} (${dataBytes[0]})`;
      } else {
        type = 'noteon';
        label = 'Note On';
        detail = `${midiToNoteName(dataBytes[0])} (${dataBytes[0]}) vel=${velocity}`;
      }
      break;
    }
    case 0x80:
      type = 'noteoff';
      label = 'Note Off';
      detail = `${midiToNoteName(dataBytes[0])} (${dataBytes[0]})`;
      break;
    case 0xB0: {
      type = 'controlchange';
      label = 'CC';
      const ccName = CC_NAMES[dataBytes[0]] ?? `CC#${dataBytes[0]}`;
      detail = `${ccName} = ${dataBytes[1]}`;
      break;
    }
    case 0xE0: {
      type = 'pitchbend';
      label = 'Pitch Bend';
      const value = ((dataBytes[1] << 7) | dataBytes[0]) - 8192;
      detail = `value=${value > 0 ? '+' : ''}${value}`;
      break;
    }
    case 0xD0:
      type = 'aftertouch';
      label = 'Ch Pressure';
      detail = `pressure=${dataBytes[0]}`;
      break;
    case 0xA0:
      type = 'aftertouch';
      label = 'Key Pressure';
      detail = `${midiToNoteName(dataBytes[0])} (${dataBytes[0]}) pressure=${dataBytes[1]}`;
      break;
    case 0xC0:
      type = 'programchange';
      label = 'Program';
      detail = `program=${dataBytes[0]}`;
      break;
    default:
      // System messages
      if (statusByte === 0xF8) { type = 'clock'; label = 'Clock'; }
      else if (statusByte === 0xFE) { type = 'activesensing'; label = 'Active Sense'; }
      else if (statusByte === 0xFA) { type = 'start'; label = 'Start'; }
      else if (statusByte === 0xFC) { type = 'stop'; label = 'Stop'; }
      else if (statusByte === 0xFB) { type = 'continue'; label = 'Continue'; }
      else if (statusByte === 0xF0) { type = 'sysex'; label = 'SysEx'; detail = `${rawData.length} bytes`; }
      else { detail = `status=0x${statusByte.toString(16).toUpperCase()}`; }
      break;
  }

  return {
    id,
    timestamp,
    type,
    channel: command < 0xF0 ? channel : undefined,
    statusByte,
    dataBytes,
    rawData,
    label,
    detail,
  };
}

export function useMidiMonitor(active: boolean) {
  const [events, setEvents] = useState<MidiMonitorEvent[]>([]);
  const [paused, setPaused] = useState(false);
  const [filterType, setFilterType] = useState<FilterType>('all');

  const bufferRef = useRef<MidiMonitorEvent[]>([]);
  const nextIdRef = useRef(0);
  const pausedRef = useRef(false);
  const flushTimerRef = useRef<ReturnType<typeof setInterval> | null>(null);

  // Keep pausedRef in sync
  useEffect(() => { pausedRef.current = paused; }, [paused]);

  // Subscribe to raw MIDI when active
  useEffect(() => {
    if (!active) return;

    const unsub = midiService.onRawMessage((e) => {
      if (pausedRef.current) return;

      const rawData = Array.from(e.message.rawData);
      const statusByte = e.message.statusByte;

      // Filter out clock and active sensing by default (they flood)
      if (statusByte === 0xF8 || statusByte === 0xFE) return;

      const dataBytes = Array.from(e.message.rawDataBytes);
      const id = nextIdRef.current++;
      const event = formatEvent(id, statusByte, dataBytes, rawData, performance.now());
      bufferRef.current.push(event);
    });

    // Flush buffer to React state periodically
    flushTimerRef.current = setInterval(() => {
      if (bufferRef.current.length === 0) return;
      const batch = bufferRef.current.splice(0);
      setEvents((prev) => {
        const merged = [...prev, ...batch];
        return merged.length > MAX_EVENTS ? merged.slice(-MAX_EVENTS) : merged;
      });
    }, FLUSH_INTERVAL);

    return () => {
      unsub();
      if (flushTimerRef.current) clearInterval(flushTimerRef.current);
      flushTimerRef.current = null;
      bufferRef.current = [];
    };
  }, [active]);

  const togglePause = useCallback(() => setPaused((p) => !p), []);
  const clear = useCallback(() => {
    setEvents([]);
    bufferRef.current = [];
  }, []);

  const filteredEvents = filterType === 'all'
    ? events
    : events.filter((e) => e.type === filterType);

  return { events: filteredEvents, paused, togglePause, clear, filterType, setFilterType };
}

export type { FilterType };
