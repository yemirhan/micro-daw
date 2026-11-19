import { describe, expect, it, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import '@/test/toneMock';
import { audioEngine } from '@/services/AudioEngine';
import { drumEngine } from '@/services/DrumEngine';
import { useSignalPreview } from './useSignalPreview';

const props = (active: boolean) => ({
  type: 'sine' as const,
  amplitude: 0.8,
  frequency: 440,
  active,
});

describe('useSignalPreview mute', () => {
  beforeEach(() => {
    audioEngine.unmute();
    drumEngine.unmute();
    audioEngine.setPreviewMuted(false);
    drumEngine.setPreviewMuted(false);
  });

  it("silences with its own mute and never touches the user's", () => {
    const h = renderHook(({ active }) => useSignalPreview(props(active)), {
      initialProps: { active: true },
    });
    // The user's mute (the top bar's state) stays off while the tab shows.
    expect(audioEngine.isMuted()).toBe(false);
    expect(drumEngine.isMuted()).toBe(false);
    h.unmount();
    expect(audioEngine.isMuted()).toBe(false);
  });

  it('keeps a mute the user set before opening the Generate tab', () => {
    audioEngine.mute();
    drumEngine.mute();
    const h = renderHook(({ active }) => useSignalPreview(props(active)), {
      initialProps: { active: true },
    });
    h.unmount();
    // Before the fix the cleanup called `unmute()`: the output came back on while the top bar
    // still showed "Muted".
    expect(audioEngine.isMuted()).toBe(true);
    expect(drumEngine.isMuted()).toBe(true);
  });

  it('a drum hit is silent while the preview mute is on, and plays again after', () => {
    const h = renderHook(({ active }) => useSignalPreview(props(active)), {
      initialProps: { active: true },
    });
    const silenced = (drumEngine as unknown as { silenced(): boolean }).silenced();
    expect(silenced).toBe(true);
    h.rerender({ active: false });
    expect((drumEngine as unknown as { silenced(): boolean }).silenced()).toBe(false);
    h.unmount();
  });
});
