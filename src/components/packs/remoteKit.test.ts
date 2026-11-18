import { describe, expect, it, vi } from 'vitest';
import { fakeManifest } from './fixtures';
import { remoteKitPlan, remotePadLoader } from './remoteKit';

describe('remoteKitPlan (pre-install kit preview)', () => {
  const m = fakeManifest();

  it('maps a catalog kit to pad names / settings plus the remote URL of each pad', () => {
    const plan = remoteKitPlan(m, 'main')!;
    expect(plan.kit.name).toBe('808 Basic');
    expect(plan.kit.pads.map((p) => p.sample?.name ?? null).slice(0, 4)).toEqual(['Kick', 'Snare', 'Closed Hat', null]);
    expect(plan.kit.pads[2].chokeGroup).toBe(1);
    expect(plan.urls.slice(0, 4)).toEqual([m.files[0].url, m.files[1].url, m.files[2].url, null]);
    expect(plan.urls).toHaveLength(16);
  });

  it('skips pads whose file is not in the pack, and unknown kits', () => {
    const bad = fakeManifest({ kits: [{ id: 'k', name: 'K', pads: [{ pad: 0, file: 'nope.wav' }, { pad: 99, file: m.files[0].path }] }] });
    const plan = remoteKitPlan(bad, 'k')!;
    expect(plan.urls.every((u) => u === null)).toBe(true);
    expect(plan.kit.pads.every((p) => p.sample === null)).toBe(true);
    expect(remoteKitPlan(m, 'missing')).toBeNull();
  });

  it('names an unnamed pad after its file label (review L-d)', () => {
    const files = m.files.map((f, i) => (i === 0 ? { ...f, label: 'Kick · tone 0' } : f));
    const named = fakeManifest({ files, kits: [{ id: 'k', name: 'K', pads: [{ pad: 0, file: files[0].path }] }] });
    expect(remoteKitPlan(named, 'k')!.kit.pads[0].sample?.name).toBe('Kick · tone 0');
  });

  it('loads each pad through the given fetcher (empty pads load nothing)', async () => {
    const plan = remoteKitPlan(m, 'main')!;
    const buf = {} as AudioBuffer;
    const fetch = vi.fn(async () => buf);
    const load = remotePadLoader(plan, fetch);
    expect(await load(0)).toBe(buf);
    expect(fetch).toHaveBeenCalledWith(m.files[0].url);
    expect(await load(5)).toBeNull();
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
