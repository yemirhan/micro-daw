import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';
import { packService } from '@/services/PackService';
import { arrangementEngine } from '@/services/ArrangementEngine';
import { fakeInstalled, fakeManifest } from './fixtures';
import { kitPreview } from './kitPreview';
import { remoteItem, soundKey } from './packsModel';
import { usePacksController } from './usePacksController';

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

  it('Get & load installs, then loads the kit on the pads (U2)', async () => {
    let finish!: (v: ReturnType<typeof fakeInstalled> | null) => void;
    vi.spyOn(packService, 'install').mockImplementation(() => new Promise((r) => (finish = r)));
    const setKit = vi.spyOn(arrangementEngine, 'setKit');
    const onLoaded = vi.fn();
    const { result } = renderHook(() => usePacksController(onLoaded));
    act(() => result.current.getAndLoad(item, 'main'));
    expect(result.current.pendingLoad).toBe('tr808-fischer/main');
    expect(setKit).not.toHaveBeenCalled();
    await act(async () => finish(fakeInstalled(m)));
    expect(result.current.pendingLoad).toBeNull();
    expect(setKit).toHaveBeenCalledWith(expect.any(String), expect.objectContaining({ name: '808 Basic' }));
    expect(onLoaded).toHaveBeenCalledTimes(1);
  });

  it('Get & load does not load anything when the install fails or is cancelled', async () => {
    vi.spyOn(packService, 'install').mockResolvedValue(null);
    const setKit = vi.spyOn(arrangementEngine, 'setKit');
    const { result } = renderHook(() => usePacksController());
    await act(async () => result.current.getAndLoad(item, 'main'));
    expect(result.current.pendingLoad).toBeNull();
    expect(setKit).not.toHaveBeenCalled();
  });
});
