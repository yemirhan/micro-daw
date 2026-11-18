import { describe, expect, it } from 'vitest';
import type { Track } from '@/types/arrangement';
import { DEFAULT_TRACK_EFFECTS } from '@/types/effects';
import { toneMock, type MockToneNode } from '@/test/toneMock';
import { createTrackAudioNodes, removeFilterLfo, setTrackCompressorEnabled, setTrackMuted } from './trackAudioFactory';

function track(compressor: boolean): Track {
  return {
    id: 't1',
    name: 'Audio',
    instrument: { type: 'audio' } as Track['instrument'],
    regions: [],
    volume: 0,
    pan: 0,
    muted: false,
    solo: false,
    color: '#fff',
    effects: { ...DEFAULT_TRACK_EFFECTS, compressor: { ...DEFAULT_TRACK_EFFECTS.compressor, enabled: compressor } },
  };
}

const node = (n: unknown) => n as unknown as MockToneNode;
const lastTarget = (n: unknown) => node(n).connect.mock.calls.at(-1)?.[0];

describe('track compressor bypass (review round 2: #10)', () => {
  it('a disabled compressor is not in the signal path (no 6 ms DynamicsCompressor look-ahead)', () => {
    const audio = createTrackAudioNodes(track(false), toneMock.destination as never);
    expect(audio.compressorInline).toBe(false);
    expect(lastTarget(audio.eq)).toBe(audio.chorus);
    expect(node(audio.eq).connect.mock.calls.some((c: unknown[]) => c[0] === audio.compressor)).toBe(false);
  });

  it('an enabled compressor sits between EQ and chorus', () => {
    const audio = createTrackAudioNodes(track(true), toneMock.destination as never);
    expect(audio.compressorInline).toBe(true);
    expect(lastTarget(audio.eq)).toBe(audio.compressor);
    expect(lastTarget(audio.compressor)).toBe(audio.chorus);
  });

  it('toggling rewires EQ (idempotent)', () => {
    const audio = createTrackAudioNodes(track(false), toneMock.destination as never);
    setTrackCompressorEnabled(audio, true);
    expect(audio.compressorInline).toBe(true);
    expect(node(audio.eq).disconnect).toHaveBeenCalledTimes(1);
    expect(lastTarget(audio.eq)).toBe(audio.compressor);
    setTrackCompressorEnabled(audio, true);
    expect(node(audio.eq).disconnect).toHaveBeenCalledTimes(1);
    setTrackCompressorEnabled(audio, false);
    expect(lastTarget(audio.eq)).toBe(audio.chorus);
    expect(audio.compressorInline).toBe(false);
  });

  it('an enabled compressor at ratio 1 does nothing, so it stays bypassed', () => {
    const t = track(true);
    t.effects = { ...t.effects!, compressor: { ...t.effects!.compressor, ratio: 1 } };
    const audio = createTrackAudioNodes(t, toneMock.destination as never);
    expect(audio.compressorInline).toBe(false);
    expect(lastTarget(audio.eq)).toBe(audio.chorus);
  });
});

describe('track mute and filter LFO (Electron bugs fixed in both apps, 2026-11-18)', () => {
  it('mute is a gate after the volume: volume writes never un-mute', () => {
    const t = { ...track(false), muted: true };
    const audio = createTrackAudioNodes(t, toneMock.destination as never);
    expect(lastTarget(audio.volume)).toBe(audio.mute);
    expect(lastTarget(audio.mute)).toBe(audio.meter);
    expect(node(audio.mute).__args[0]).toBe(0); // built muted
    setTrackMuted(audio, true);
    audio.volume.volume.value = -6;
    expect(node(audio.mute).gain.value).toBe(0);
    setTrackMuted(audio, false);
    expect(node(audio.mute).gain.value).toBe(1);
    expect(audio.volume.volume.value).toBe(-6);
    setTrackMuted(audio, true);
    expect(node(audio.mute).gain.value).toBe(0);
  });

  it('removing the LFO clears the frequency override and restores the static cutoff', () => {
    const t = track(false);
    t.effects = { ...t.effects!, filter: { ...t.effects!.filter, enabled: true, cutoff: 1200, lfoDepth: 0.5 } };
    const audio = createTrackAudioNodes(t, toneMock.destination as never);
    const lfo = audio.lfo!;
    expect(lfo).not.toBeNull();
    const freq = node(audio.filter).frequency;
    freq.overridden = true; // what Tone's connectSignal sets
    removeFilterLfo(audio, 1200);
    expect(audio.lfo).toBeNull();
    expect(node(lfo).dispose).toHaveBeenCalled();
    expect(freq.overridden).toBe(false);
    expect(freq.value).toBe(1200);
  });
});
