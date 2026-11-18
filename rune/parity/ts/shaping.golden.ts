/**
 * Master-bus soft clip golden: the 4096-point clipper curve exactly as `installMasterBus` builds it
 * (Tone.WaveShaper.setMap: curve[i] = f32(softClip(((i / 4095) * 2 - 1) * CLIP_RANGE))), softClip
 * samples in f64 and the drive gain. Kick / snare curves come from Chromium (shaping_chromium.json).
 */
import { it } from 'vitest';
import { MASTER_DRIVE_DB, softClip } from '@/audio/masterBus';
import { writeFixture } from './fixture';

it('dumps the master soft clip curve', () => {
  const len = 4096;
  const curve = new Float32Array(len);
  for (let i = 0; i < len; i++) curve[i] = softClip(((i / (len - 1)) * 2 - 1) * 2);
  const softClipSamples: [number, number][] = [];
  for (let i = -300; i <= 300; i++) {
    const x = i * 0.0101;
    softClipSamples.push([x, softClip(x)]);
  }
  for (const x of [0.85, -0.85, 0.8500001, 0.98, 1, 2, 5, 100, -100, 0]) softClipSamples.push([x, softClip(x)]);
  writeFixture('shaping', {
    masterCurve: Array.from(curve),
    softClip: softClipSamples,
    driveGain: Math.pow(10, MASTER_DRIVE_DB / 20) / 2,
  });
});
