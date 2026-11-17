import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { packService } from '@/services/PackService';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { fakeInstalled, fakeManifest } from './fixtures';
import { kitPreview } from './kitPreview';
import { remoteItem, soundKey } from './packsModel';
import { usePacksController } from './usePacksController';
import { createClassicKit } from '@/audio/sampler/classic';

const m = fakeManifest();
const item = remoteItem(m);
const kick = { kind: 'file' as const, file: m.files[0] };

afterEach(() => {
  vi.restoreAllMocks();
  kitPreview.stop();
});

describe('usePacksController', () => {
  it('keeps a failed sound preview as an inline error; a retry clears it (U1)', async () => {
    const remote = vi.spyOn(packService, 'previewRemote').mockRejectedValueOnce(new Error("Error invoking remote method 'packs:fetch-preview': Error: HTTP 404")).mockResolvedValueOnce();
    const { result } = renderHook(() => usePacksController());
    const key = soundKey(item.id, kick);
    act(() => result.current.playSound(item, kick));
    await waitFor(() => expect(result.current.soundError(key)).toBe('HTTP 404'));
    expect(remote).toHaveBeenCalledWith(m.files[0].url, key);
    act(() => result.current.playSound(item, kick));
    expect(result.current.soundError(key)).toBeNull();
    await waitFor(() => expect(remote).toHaveBeenCalledTimes(2));
    expect(result.current.soundError(key)).toBeNull();
  });

  it('previews a kit of a pack that is not installed from its remote files (U2)', async () => {
    const play = vi.spyOn(kitPreview, 'play').mockResolvedValue("Couldn't load this kit's sounds");
    const { result } = renderHook(() => usePacksController());
    act(() => result.current.previewKit(item, 'main'));
    expect(play).toHaveBeenCalledWith('tr808-fischer/main', expect.objectContaining({ name: '808 Basic' }), { loadBuffer: expect.any(Function) });
    await waitFor(() => expect(result.current.kitPreviewError('tr808-fischer/main')).toMatch(/couldn't load/i));
  });

  it('Get & load hands the request to packService and shows its pending key (review M2)', async () => {
    const get = vi.spyOn(packService, 'getAndLoad').mockImplementation(async (id, kitId) => {
      act(() => (packService as unknown as { set: (p: object) => void }).set({ pendingLoad: `${id}/${kitId}` }));
      return null;
    });
    const { result } = renderHook(() => usePacksController());
    await act(async () => result.current.getAndLoad(item, 'main'));
    expect(get).toHaveBeenCalledWith('tr808-fischer', 'main');
    expect(result.current.pendingLoad).toBe('tr808-fischer/main');
    act(() => packService.cancelPendingLoad());
    expect(result.current.pendingLoad).toBeNull();
  });

  it('navigates to the pads when packService loads the kit, only while the page is mounted (review M2)', async () => {
    const onLoaded = vi.fn();
    vi.spyOn(packService, 'install').mockResolvedValue(fakeInstalled(m));
    const setKit = vi.spyOn(arrangementEngine, 'setKit');
    const first = renderHook(() => usePacksController(onLoaded));
    await act(async () => {
      await packService.getAndLoad(item.id, 'main');
    });
    expect(setKit).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({ name: '808 Basic' }));
    expect(onLoaded).toHaveBeenCalledTimes(1);
    // Left the page: the next load still happens, but nothing navigates.
    first.unmount();
    await act(async () => {
      await packService.getAndLoad(item.id, 'main');
    });
    expect(setKit).toHaveBeenCalledTimes(2);
    expect(onLoaded).toHaveBeenCalledTimes(1);
  });

  it('only the latest Get & load wins: an older download finishing later loads nothing (review M2)', async () => {
    const finish: Record<string, (v: ReturnType<typeof fakeInstalled> | null) => void> = {};
    vi.spyOn(packService, 'install').mockImplementation((id) => new Promise((r) => (finish[id] = r)));
    const setKit = vi.spyOn(arrangementEngine, 'setKit');
    const onLoaded = vi.fn();
    const { result } = renderHook(() => usePacksController(onLoaded));
    const other = remoteItem(fakeManifest({ id: 'other-pack' }));
    act(() => result.current.getAndLoad(item, 'main'));
    act(() => result.current.getAndLoad(other, 'main'));
    await waitFor(() => expect(result.current.pendingLoad).toBe('other-pack/main'));
    await act(async () => finish['tr808-fischer'](fakeInstalled(m)));
    expect(setKit).not.toHaveBeenCalled();
    expect(onLoaded).not.toHaveBeenCalled();
    expect(result.current.pendingLoad).toBe('other-pack/main');
  });

  it('Load on pads cancels a pending Get & load (latest intent wins)', () => {
    vi.spyOn(arrangementEngine, 'setKit').mockImplementation(() => undefined);
    const cancel = vi.spyOn(packService, 'cancelPendingLoad');
    const { result } = renderHook(() => usePacksController());
    act(() => result.current.loadKit(createClassicKit('tr808')));
    expect(cancel).toHaveBeenCalled();
  });
});
