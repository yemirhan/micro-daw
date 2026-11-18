/**
 * Golden for the MIDI monitor (rune/crates/mdaw-midi/src/monitor.rs): the real `useMidiMonitor`
 * hook with `midiService.onRawMessage` captured; raw messages of every kind (and malformed ones)
 * are fed, the 50 ms flush runs, and the formatted events are dumped (timestamps dropped:
 * `performance.now()`), plus the type filters and the 500-event cap.
 */
import { it, vi } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { writeFixture } from './fixture';

const raw = vi.hoisted(() => ({ cb: null as null | ((e: unknown) => void) }));
vi.mock('@/services/MidiService', () => ({
  midiService: {
    onRawMessage: (cb: (e: unknown) => void) => {
      raw.cb = cb;
      return () => {
        raw.cb = null;
      };
    },
  },
}));

const { useMidiMonitor } = await import('@/hooks/useMidiMonitor');

function feed(bytes: number[]) {
  raw.cb?.({ message: { rawData: Uint8Array.from(bytes), statusByte: bytes[0], rawDataBytes: Uint8Array.from(bytes.slice(1)) } });
}

it('dumps monitor events', () => {
  vi.useFakeTimers();
  const messages: number[][] = [
    [0x90, 60, 100], [0x90, 60, 0], [0x80, 61, 64], [0x9f, 127, 1], [0xb0, 64, 127], [0xb3, 74, 0], [0xb0, 20, 5],
    [0xb0, 1, 1], [0xb0, 7, 2], [0xb0, 10, 3], [0xb0, 11, 4], [0xb0, 2, 4], [0xb0, 65, 1], [0xb0, 66, 1], [0xb0, 67, 1],
    [0xb0, 71, 1], [0xb0, 91, 1], [0xb0, 93, 1], [0xe0, 0, 64], [0xe0, 127, 127], [0xe0, 0, 0], [0xe5, 1, 64],
    [0xd0, 77], [0xa2, 61, 33], [0xc4, 9], [0xf8], [0xfe], [0xfa], [0xfc], [0xfb], [0xf0, 1, 2, 0xf7], [0xf2, 1, 2],
    [0xf6], [0x90, 0, 1], [0x90, 11, 2], [0x90, 12, 3], [0x90], [0x90, 5], [0xb0], [0xa0, 1], [0xe0], [0xe0, 5],
  ];
  for (let n = 0; n < 128; n += 7) messages.push([0x90, n, 64], [0xb0, n, n]);
  const hook = renderHook(({ active }: { active: boolean }) => useMidiMonitor(active), { initialProps: { active: true } });
  for (const m of messages) feed(m);
  act(() => vi.advanceTimersByTime(50));
  const strip = (e: Record<string, unknown>) => {
    const { timestamp: _t, ...rest } = e;
    return rest;
  };
  const all = hook.result.current.events.map((e) => strip(e as unknown as Record<string, unknown>));
  const filtered: Record<string, unknown[]> = {};
  for (const f of ['noteon', 'noteoff', 'controlchange', 'pitchbend', 'aftertouch', 'programchange'] as const) {
    act(() => hook.result.current.setFilterType(f));
    filtered[f] = hook.result.current.events.map((e) => e.id);
  }
  act(() => hook.result.current.setFilterType('all'));
  // cap: 600 more note-ons in two flushes
  for (let i = 0; i < 300; i++) feed([0x90, i % 128, 1 + (i % 127)]);
  act(() => vi.advanceTimersByTime(50));
  for (let i = 0; i < 300; i++) feed([0x91, i % 128, 1 + (i % 127)]);
  act(() => vi.advanceTimersByTime(50));
  const capped = hook.result.current.events.map((e) => e.id);
  // paused: nothing buffered
  act(() => hook.result.current.togglePause());
  feed([0x90, 1, 1]);
  act(() => vi.advanceTimersByTime(50));
  const pausedLen = hook.result.current.events.length;
  hook.unmount();
  vi.useRealTimers();
  writeFixture('midi_monitor', { messages, events: all, filtered, capped, pausedLen });
});
