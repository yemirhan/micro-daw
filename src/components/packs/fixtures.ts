/** Test / screenshot fixtures for the Packs page. */
import type { InstalledPack, PackManifest } from '@/types/packs';
import type { ClassicMachineInfo } from './packsModel';

export const FAKE_MACHINE: ClassicMachineInfo = {
  id: 'tr808',
  name: '808-style',
  tagline: 'Booming analog kick, snappy snare, metallic hats',
  color: 'oklch(0.70 0.18 45)',
  voices: [
    { id: 'kick', label: 'Kick', gm: 36 },
    { id: 'snare', label: 'Snare', gm: 38 },
    { id: 'ch', label: 'Closed Hat', gm: 42 },
    { id: 'oh', label: 'Open Hat', gm: 46 },
  ],
  pads: ['kick', 'snare', 'ch', 'oh'],
};

export function fakeManifest(over: Partial<PackManifest> = {}): PackManifest {
  return {
    id: 'tr808-fischer',
    name: 'TR-808 recordings',
    tagline: 'Every voice of a real 808, multisampled',
    description: 'Recordings of a TR-808 by Michael Fischer.',
    tags: ['808', 'analog', 'hip-hop'],
    author: 'Michael Fischer',
    source: 'https://github.com/tidalcycles/sounds-tr808-fischer',
    license: 'CC0-1.0',
    color: 'oklch(0.7 0.15 30)',
    files: [
      { path: 'bd/BD0000.wav', url: 'https://raw.githubusercontent.com/x/y/sha/bd/BD0000.wav', size: 1024 * 1024 },
      { path: 'sd/SD0000.wav', url: 'https://raw.githubusercontent.com/x/y/sha/sd/SD0000.wav', size: 512 * 1024 },
      { path: 'ch/CH.wav', url: 'https://raw.githubusercontent.com/x/y/sha/ch/CH.wav' },
    ],
    kits: [
      {
        id: 'main',
        name: '808 Basic',
        pads: [
          { pad: 0, file: 'bd/BD0000.wav', name: 'Kick' },
          { pad: 1, file: 'sd/SD0000.wav', name: 'Snare' },
          { pad: 2, file: 'ch/CH.wav', name: 'Closed Hat', settings: { chokeGroup: 1 } },
        ],
      },
    ],
    version: 1,
    ...over,
  };
}

export function fakeInstalled(manifest: PackManifest, dir = `/data/packs/${manifest.id}`): InstalledPack {
  return { id: manifest.id, version: manifest.version, dir, installedAt: '2026-11-01T00:00:00Z', manifest };
}
