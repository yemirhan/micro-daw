import { it } from 'vitest';
import { audioBufferToWav } from '@/utils/wavEncoder';
import { genChannel, mulberry32 } from './bufgen';
import { writeFixture } from './fixture';

/** Inputs and outputs travel as base64 bytes (f32 LE planar channels in, WAV bytes out). */
const b64 = (u8: Uint8Array): string => Buffer.from(u8).toString('base64');

interface Case { name: string; sr: number; channels: Float32Array[] }

const SPECIAL = [
  -1.5, -1, -0.99999, -0.5, -1 / 32768, -1 / 65536, -1e-9, -0, 0, 1e-9, 1 / 65536, 1 / 32767, 0.5, 0.99999, 1, 1.5,
  NaN, Infinity, -Infinity, 0.25, -0.25, 1 / 3, -1 / 3, 0.7071, -0.7071,
];

function ramp(len: number, from: number, to: number): Float32Array {
  const d = new Float32Array(len);
  for (let i = 0; i < len; i++) d[i] = from + ((to - from) * i) / Math.max(1, len - 1);
  return d;
}

function noise(seed: number, len: number, amp: number): Float32Array {
  const d = new Float32Array(len);
  const rnd = mulberry32(seed);
  for (let i = 0; i < len; i++) d[i] = (rnd() * 2 - 1) * amp;
  return d;
}

const CASES: Case[] = [
  { name: 'mono_special_44k', sr: 44100, channels: [Float32Array.from(SPECIAL)] },
  { name: 'stereo_special_48k', sr: 48000, channels: [Float32Array.from(SPECIAL), Float32Array.from(SPECIAL).reverse()] },
  {
    name: 'stereo_clicks_clip_48k_odd',
    sr: 48000,
    channels: [
      genChannel({ sr: 48000, len: 1001, channels: [] }, { seed: 11, noise: 0.05, clicks: [{ t: 0.001, amp: 1.4, decay: 300 }] }),
      genChannel({ sr: 48000, len: 1001, channels: [] }, { seed: 12, noise: 0.3, clicks: [{ t: 0.01, amp: -1.2, decay: 200 }] }),
    ],
  },
  { name: 'mono_empty_44k', sr: 44100, channels: [new Float32Array(0)] },
  { name: 'stereo_ramp_44k_len7', sr: 44100, channels: [ramp(7, -1, 1), ramp(7, 1, -1)] },
  { name: 'mono_ramp_clip_48k', sr: 48000, channels: [ramp(4097, -1.2, 1.2)] },
  { name: 'tri_noise_22k_len33', sr: 22050, channels: [noise(21, 33, 1.1), noise(22, 33, 0.01), noise(23, 33, 2)] },
  { name: 'mono_noise_96k_len513', sr: 96000, channels: [noise(31, 513, 0.9)] },
];

it('dumps audioBufferToWav bytes', async () => {
  const out = [];
  for (const c of CASES) {
    const buffer = {
      numberOfChannels: c.channels.length,
      sampleRate: c.sr,
      length: c.channels[0].length,
      duration: c.channels[0].length / c.sr,
      getChannelData: (i: number) => c.channels[i],
    } as unknown as AudioBuffer;
    const blob = audioBufferToWav(buffer);
    const bytes = new Uint8Array(
      typeof blob.arrayBuffer === 'function'
        ? await blob.arrayBuffer()
        : await new Promise<ArrayBuffer>((resolve) => {
            const r = new FileReader();
            r.onload = () => resolve(r.result as ArrayBuffer);
            r.readAsArrayBuffer(blob);
          }),
    );
    out.push({
      name: c.name,
      sampleRate: c.sr,
      channels: c.channels.map((d) => b64(new Uint8Array(d.buffer, d.byteOffset, d.byteLength))),
      type: blob.type,
      wav: b64(bytes),
    });
  }
  writeFixture('wav', out);
});
