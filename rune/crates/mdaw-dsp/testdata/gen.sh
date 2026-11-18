#!/bin/sh
# Regenerates the decode test files (ffmpeg 8.x). Deterministic source: silence until 50 ms, then
# decaying tones (L: 440 Hz + 3 kHz partial, R: 660 Hz) — a clear onset for alignment checks.
# After regenerating, re-capture the Chromium reference: node rune/parity/chromium/capture-decode.mjs
set -e
cd "$(dirname "$0")"
L='if(gte(t,0.05),(0.6*sin(2*PI*440*t)+0.2*sin(2*PI*3000*t))*exp(-6*(t-0.05)),0)'
R='if(gte(t,0.05),0.7*sin(2*PI*660*t)*exp(-4*(t-0.05)),0)'
# gen <channels:st|mo> <rate> <duration> <output args...>
gen() {
  if [ "$1" = st ]; then expr="$L|$R"; else expr="$L"; fi
  rate=$2; dur=$3; shift 3
  ffmpeg -nostdin -hide_banner -loglevel error -y -f lavfi -i "aevalsrc='$expr':s=$rate:d=$dur" "$@"
}
gen st 44100 0.3 -c:a pcm_s16le wav_s16_44k_stereo.wav
gen mo 22050 0.3 -c:a pcm_u8 wav_u8_22k_mono.wav
gen st 48000 0.3 -c:a pcm_s24le wav_s24ext_48k_stereo.wav
gen st 96000 0.2 -c:a pcm_s24le wav_s24ext_96k_stereo.wav
gen mo 44100 0.3 -c:a pcm_s32le wav_s32_44k_mono.wav
gen mo 48000 0.3 -c:a pcm_f32le wav_f32_48k_mono.wav
gen mo 44100 0.2 -c:a pcm_f64le wav_f64_44k_mono.wav
gen st 44100 0.3 -c:a pcm_s16be aiff_s16_44k_stereo.aif
gen mo 48000 0.3 -c:a pcm_s24be aiff_s24_48k_mono.aiff
gen st 44100 0.3 -c:a pcm_s16le -f aiff aifc_sowt_44k_stereo.aif
gen st 44100 0.3 -c:a flac -sample_fmt s16 flac_s16_44k_stereo.flac
gen mo 22050 0.3 -c:a flac -sample_fmt s32 flac_s24_22k_mono.flac
gen st 44100 0.3 -c:a libmp3lame -b:a 192k mp3_44k_stereo.mp3
gen mo 48000 0.3 -c:a libmp3lame -b:a 128k mp3_48k_mono.mp3
gen st 44100 0.3 -c:a vorbis -strict -2 ogg_ffvorbis_44k_stereo.ogg
gen st 44100 0.3 -c:a aac -b:a 192k m4a_aac_44k_stereo.m4a
gen st 48000 0.3 -c:a alac m4a_alac_48k_stereo.m4a
# Made from the 16-bit WAV with other encoders:
# - libvorbis (libsndfile's sndfile-convert). ffmpeg's built-in `vorbis` encoder is experimental
#   and writes a wrong final granule (kept above as ogg_ffvorbis_* to document decoder behaviour).
sndfile-convert -vorbis wav_s16_44k_stereo.wav ogg_libvorbis_44k_stereo.ogg
#   ...and a 2 s multi-page stream (slower decay), via a temporary WAV.
ffmpeg -nostdin -hide_banner -loglevel error -y -f lavfi \
  -i "aevalsrc='if(gte(t,0.05),(0.6*sin(2*PI*440*t)+0.2*sin(2*PI*3000*t))*exp(-1.5*(t-0.05)),0)|if(gte(t,0.05),0.7*sin(2*PI*660*t)*exp(-1*(t-0.05)),0)':s=44100:d=2.0" \
  -c:a pcm_s16le long.tmp.wav
sndfile-convert -vorbis long.tmp.wav ogg_libvorbis_2s_44k_stereo.ogg
rm -f long.tmp.wav
# - Apple AAC (macOS afconvert): iTunes-style gapless info (iTunSMPB, 2112 priming), no edit list.
afconvert -f m4af -d aac -b 128000 wav_s16_44k_stereo.wav m4a_aacitunes_44k_stereo.m4a
