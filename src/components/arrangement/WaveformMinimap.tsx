import { useRef, useEffect } from 'react';
import { sampleManager } from '@/services/SampleManager';

interface WaveformMinimapProps {
  sampleId: string;
  lengthBeats: number;
  offsetSeconds: number;
  pxPerBeat: number;
  color: string;
  bpm: number;
}

export function WaveformMinimap({
  sampleId,
  lengthBeats,
  offsetSeconds,
  pxPerBeat,
  color,
  bpm,
}: WaveformMinimapProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const widthPx = Math.max(4, Math.round(lengthBeats * pxPerBeat));

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const dpr = window.devicePixelRatio || 1;
    const w = widthPx;
    const h = canvas.clientHeight;

    canvas.width = w * dpr;
    canvas.height = h * dpr;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    const buffer = sampleManager.getBuffer(sampleId);
    if (!buffer) return;

    // Compute which portion of the buffer to display
    const regionDurSeconds = (lengthBeats / bpm) * 60;
    const startSample = Math.floor(offsetSeconds * buffer.sampleRate);
    const endSample = Math.min(
      Math.floor((offsetSeconds + regionDurSeconds) * buffer.sampleRate),
      buffer.length
    );
    const visibleLength = endSample - startSample;
    if (visibleLength <= 0) return;

    const samplesPerPeak = visibleLength / w;
    const midY = h / 2;

    ctx.fillStyle = 'rgba(255, 255, 255, 0.5)';
    ctx.beginPath();

    // Draw top half
    ctx.moveTo(0, midY);
    for (let i = 0; i < w; i++) {
      const bucketStart = startSample + Math.floor(i * samplesPerPeak);
      const bucketEnd = startSample + Math.min(Math.floor((i + 1) * samplesPerPeak), visibleLength);
      let max = 0;
      for (let ch = 0; ch < buffer.numberOfChannels; ch++) {
        const data = buffer.getChannelData(ch);
        for (let j = bucketStart; j < bucketEnd; j++) {
          const abs = Math.abs(data[j]);
          if (abs > max) max = abs;
        }
      }
      const y = midY - max * midY;
      ctx.lineTo(i, y);
    }

    // Draw bottom half (mirror)
    for (let i = w - 1; i >= 0; i--) {
      const bucketStart = startSample + Math.floor(i * samplesPerPeak);
      const bucketEnd = startSample + Math.min(Math.floor((i + 1) * samplesPerPeak), visibleLength);
      let max = 0;
      for (let ch = 0; ch < buffer.numberOfChannels; ch++) {
        const data = buffer.getChannelData(ch);
        for (let j = bucketStart; j < bucketEnd; j++) {
          const abs = Math.abs(data[j]);
          if (abs > max) max = abs;
        }
      }
      const y = midY + max * midY;
      ctx.lineTo(i, y);
    }

    ctx.closePath();
    ctx.fill();
  }, [sampleId, lengthBeats, offsetSeconds, pxPerBeat, bpm, widthPx]);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0.5 pointer-events-none"
      style={{ width: widthPx - 4, height: '100%' }}
    />
  );
}
