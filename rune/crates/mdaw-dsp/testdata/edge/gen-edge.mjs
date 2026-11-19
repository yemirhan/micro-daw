#!/usr/bin/env node
/**
 * Regenerates the edge-case decode files in this directory from the files in ../ (deterministic).
 * Each case documents what Chromium's decodeAudioData does with damaged or unusual input (see
 * rune/parity/chromium/capture-decode.mjs, `edge` section of decode_reference.json):
 *   trunc_*   the source cut at a byte position (inside the audio data, not on a frame boundary)
 *   corrupt_* bytes overwritten in the middle of the audio data
 *   wav_*     hand-built WAV headers / sample values (zero/huge rate, zero channels, f32 NaN/Inf)
 *
 *   node rune/crates/mdaw-dsp/testdata/edge/gen-edge.mjs
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const src = (name) => readFileSync(path.join(HERE, '..', name));
const out = (name, bytes) => writeFileSync(path.join(HERE, name), bytes);

const cut = (name, frac) => {
  const b = src(name);
  return b.subarray(0, Math.floor(b.length * frac) | 1);
};
const corrupt = (name, frac, len) => {
  const b = Buffer.from(src(name));
  const at = Math.floor(b.length * frac);
  for (let i = 0; i < len; i++) b[at + i] = (i * 151 + 7) & 0xff;
  return b;
};

/** RIFF/WAVE with a 16-byte fmt chunk. `fmt` overrides header fields; `data` is the raw data bytes. */
function wav({ format = 1, channels = 1, rate = 44100, bits = 16, blockAlign, byteRate, data }) {
  blockAlign ??= (channels * bits) / 8;
  byteRate ??= rate * blockAlign;
  const h = Buffer.alloc(44);
  h.write('RIFF', 0);
  h.writeUInt32LE(36 + data.length, 4);
  h.write('WAVE', 8);
  h.write('fmt ', 12);
  h.writeUInt32LE(16, 16);
  h.writeUInt16LE(format, 20);
  h.writeUInt16LE(channels, 22);
  h.writeUInt32LE(rate >>> 0, 24);
  h.writeUInt32LE(byteRate >>> 0, 28);
  h.writeUInt16LE(blockAlign, 32);
  h.writeUInt16LE(bits, 34);
  h.write('data', 36);
  h.writeUInt32LE(data.length, 40);
  return Buffer.concat([h, data]);
}

/** 4410 frames of a 441 Hz sine at 0.5 as s16 (mono). */
function s16Sine(frames = 4410) {
  const d = Buffer.alloc(frames * 2);
  for (let i = 0; i < frames; i++) d.writeInt16LE(Math.round(Math.sin((2 * Math.PI * 441 * i) / 44100) * 16383), i * 2);
  return d;
}

// --- truncated / corrupted real files ---------------------------------------------------------
out('trunc_wav_s16_44k_stereo.wav', cut('wav_s16_44k_stereo.wav', 0.6));
out('trunc_flac_s16_44k_stereo.flac', cut('flac_s16_44k_stereo.flac', 0.6));
out('trunc_mp3_44k_stereo.mp3', cut('mp3_44k_stereo.mp3', 0.6));
out('trunc_ogg_libvorbis_2s_44k_stereo.ogg', cut('ogg_libvorbis_2s_44k_stereo.ogg', 0.5));
out('trunc_m4a_aacfrag_44k_stereo.m4a', cut('m4a_aacfrag_44k_stereo.m4a', 0.6));
out('trunc_m4a_aac_44k_stereo.m4a', cut('m4a_aac_44k_stereo.m4a', 0.6));
out('corrupt_flac_s16_44k_stereo.flac', corrupt('flac_s16_44k_stereo.flac', 0.5, 64));
out('corrupt_mp3_44k_stereo.mp3', corrupt('mp3_44k_stereo.mp3', 0.5, 64));
out('corrupt_ogg_libvorbis_2s_44k_stereo.ogg', corrupt('ogg_libvorbis_2s_44k_stereo.ogg', 0.5, 64));

// --- WAV headers ------------------------------------------------------------------------------
const zeroRate = Buffer.from(src('wav_s16_44k_stereo.wav'));
zeroRate.writeUInt32LE(0, 24); // the reviewers' reproducer: a valid file with the rate zeroed
out('wav_rate0_44k_stereo.wav', zeroRate);
out('wav_rate2000_2k_mono.wav', wav({ rate: 2000, data: s16Sine() }));
out('wav_rate3000_3k_mono.wav', wav({ rate: 3000, data: s16Sine() }));
out('wav_rate768000_768k_mono.wav', wav({ rate: 768000, data: s16Sine() }));
out('wav_rate800000_800k_mono.wav', wav({ rate: 800000, data: s16Sine() }));
out('wav_chan0_44k_mono.wav', wav({ channels: 0, blockAlign: 2, data: s16Sine() }));
out('wav_chan33_44k_mono.wav', wav({ channels: 33, data: Buffer.concat(Array(33).fill(s16Sine(441))) }));

// f32 with non-finite / out-of-range values (mono, 64 frames).
const special = [Number.NaN, Infinity, -Infinity, 2.5, -3, 1e-42, -0, 0.25];
const f = Buffer.alloc(64 * 4);
for (let i = 0; i < 64; i++) f.writeFloatLE(i < special.length ? special[i] : Math.sin(i / 3) * 0.5, i * 4);
out('wav_f32nonfinite_44k_mono.wav', wav({ format: 3, bits: 32, data: f }));
