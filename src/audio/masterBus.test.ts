import { describe, expect, it, vi } from 'vitest';
import * as Tone from 'tone';
import {
  MASTER_REFERENCE_DB,
  ensureMasterBus,
  getMasterVolumeDb,
  installMasterBus,
  masterGainDb,
  onMasterVolumeChange,
  setMasterVolumeDb,
} from './masterBus';
import { MAX_VOLUME, MIN_VOLUME } from '@/utils/constants';

describe('master volume', () => {
  it('maps the fader to a bus gain that is unity at the default level', () => {
    expect(masterGainDb(MASTER_REFERENCE_DB)).toBe(0);
    expect(masterGainDb(MIN_VOLUME)).toBe(MIN_VOLUME - MASTER_REFERENCE_DB);
    expect(masterGainDb(MAX_VOLUME + 50)).toBe(MAX_VOLUME - MASTER_REFERENCE_DB);
  });

  it('drives the first node of the real-time bus (everything passes through it), persists and notifies', () => {
    const bus = ensureMasterBus();
    const dest = Tone.getDestination() as unknown as { chain: ReturnType<typeof vi.fn> };
    expect(dest.chain.mock.calls.at(-1)?.[0]).toBe(bus.master);
    const cb = vi.fn();
    const off = onMasterVolumeChange(cb);
    setMasterVolumeDb(-20);
    expect(getMasterVolumeDb()).toBe(-20);
    expect(bus.master.volume.rampTo).toHaveBeenLastCalledWith(-20 - MASTER_REFERENCE_DB, expect.any(Number));
    expect(cb).toHaveBeenCalledWith(-20);
    expect(localStorage.getItem('micro-daw:masterVolume')).toBe('-20');
    setMasterVolumeDb(-20);
    expect(cb).toHaveBeenCalledTimes(1);
    off();
    setMasterVolumeDb(MASTER_REFERENCE_DB);
  });

  it('offline renders get a unity master unless asked otherwise', () => {
    const dest = { chain: vi.fn() } as unknown as Tone.ToneAudioNode;
    const bus = installMasterBus(dest);
    expect((bus.master as unknown as { __args: unknown[] }).__args[0]).toBe(0);
  });
});
