// B3 real-Tone probe: a Song audio clip loaded from a pack survives a pack update (and removal),
// in WAV export and in live playback (player swapped to new audio).
export default async (page, { log }) => {
  const out = await page.evaluate(async () => {
    const src = await (await fetch('/src/audio/sampler/SamplerKit.ts')).text();
    const Tone = await import(src.match(/from\s+"([^"]*deps\/tone\.js[^"]*)"/)[1]);
    const errs = [];
    window.addEventListener('error', (ev) => errs.push('E:' + String(ev.message).slice(0, 160)));
    window.addEventListener('unhandledrejection', (ev) => errs.push('R:' + String(ev.reason).slice(0, 160)));
    const wav = (seconds, fn, sr = 44100) => {
      const n = Math.round(seconds * sr); const buf = new ArrayBuffer(44 + n * 2); const v = new DataView(buf);
      const w = (o, s) => { for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i)); };
      w(0, 'RIFF'); v.setUint32(4, 36 + n * 2, true); w(8, 'WAVE'); w(12, 'fmt ');
      v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
      v.setUint32(24, sr, true); v.setUint32(28, sr * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
      w(36, 'data'); v.setUint32(40, n * 2, true);
      for (let i = 0; i < n; i++) v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, fn(i, sr))) * 32767, true);
      return buf;
    };
    const PATH = '/probe/packs/uzu-drumkit/kick/10_bd.wav';
    let current = wav(1, (i, sr) => 0.5 * Math.sin((2 * Math.PI * 220 * i) / sr)); // v1: 220 Hz @ -6 dB
    let gone = false;
    window.electronAPI = { ...(window.electronAPI || {}), projectSetTitle: async () => {}, packsDirPath: '/probe/packs',
      sampleReadFile: async (p) => { if (p !== PATH || gone) throw new Error('ENOENT ' + p); return current.slice(0); } };
    const e = window.__microDaw.arrangementEngine;
    const sm = (await import('/src/services/SampleManager.ts')).sampleManager;
    const bc = await import('/src/audio/sampler/bufferCache.ts');
    const exp = await import('/src/services/ExportService.ts');
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    e.stop(); e.resetToStarter(); e.setBpm(120);
    const sample = await sm.loadFromPath(PATH);
    const at = e.addTrack('audio');
    const region = e.addAudioRegion(at.id, sample.id, 0);
    const onlyAudio = () => ({ ...e.getArrangement(), tracks: [e.getTracks().find((t) => t.id === at.id)] });
    const wavPeak = async () => {
      const w = new DataView(await (await exp.exportWav(onlyAudio())).arrayBuffer());
      let pk = 0; for (let i = 44; i < w.byteLength; i += 2) pk = Math.max(pk, Math.abs(w.getInt16(i, true)));
      return pk;
    };
    const res = { sampleId: sample.id };
    res.exportPeakV1 = await wavPeak();
    // Pack update while the Song is playing: new file content = 0.9 amplitude.
    e.setLoopEnabled?.(true);
    e.play(); await wait(200);
    const player = e.trackAudio.get(at.id).players.get(region.id);
    const before = player.buffer.get();
    current = wav(1, (i, sr) => 0.9 * Math.sin((2 * Math.PI * 440 * i) / sr)); // v2
    bc.invalidatePackBuffers('uzu-drumkit');
    await sm.whenReloaded(PATH);
    const after = player.buffer.get();
    res.livePlayer = { swapped: before !== after, newPeak: Math.max(...after.getChannelData(0).slice(0, 2000).map(Math.abs)).toFixed(2), playerDisposed: player.disposed ?? false };
    e.stop();
    res.sameId = sm.getSample(sample.id)?.id === sample.id && region.audio.sampleId === sample.id;
    res.exportPeakV2 = await wavPeak();
    // Pack removed: audio kept, flagged unavailable.
    gone = true;
    bc.invalidatePackBuffers('uzu-drumkit');
    await sm.whenReloaded(PATH);
    res.unavailable = sm.isUnavailable(sample.id);
    res.exportPeakRemoved = await wavPeak();
    // Live playback peak after removal
    const an = new Tone.Analyser('waveform', 2048); Tone.getDestination().connect(an);
    e.play(); let max = 0; const end = performance.now() + 500;
    while (performance.now() < end) { for (const v of an.getValue()) max = Math.max(max, Math.abs(v)); await wait(8); }
    e.stop(); an.dispose();
    res.livePeakDbAfterRemoval = max > 0 ? Math.round(20 * Math.log10(max)) : -Infinity;
    res.errs = errs;
    return res;
  });
  log('B3 ' + JSON.stringify(out));
};
