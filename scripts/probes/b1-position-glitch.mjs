// B1 (review round 4): no stale playhead frames after play -> stop -> idle -> play (real Tone).
export default async (page, { log }) => {
  await page.evaluate(async () => {
    const src = await (await fetch('/src/audio/sampler/SamplerKit.ts')).text();
    const m = src.match(/from\s+"([^"]*deps\/tone\.js[^"]*)"/);
    const Tone = (window.__Tone = await import(m[1]));
    window.__errs = [];
    window.addEventListener('error', (ev) => window.__errs.push('E:' + String(ev.message).slice(0, 160)));
    window.addEventListener('unhandledrejection', (ev) => window.__errs.push('R:' + String(ev.reason).slice(0, 160)));
    const wav = (seconds, fn, sr = 44100) => {
      const n = Math.round(seconds * sr);
      const buf = new ArrayBuffer(44 + n * 2);
      const v = new DataView(buf);
      const w = (o, s) => { for (let i = 0; i < s.length; i++) v.setUint8(o + i, s.charCodeAt(i)); };
      w(0, 'RIFF'); v.setUint32(4, 36 + n * 2, true); w(8, 'WAVE'); w(12, 'fmt ');
      v.setUint32(16, 16, true); v.setUint16(20, 1, true); v.setUint16(22, 1, true);
      v.setUint32(24, sr, true); v.setUint32(28, sr * 2, true); v.setUint16(32, 2, true); v.setUint16(34, 16, true);
      w(36, 'data'); v.setUint32(40, n * 2, true);
      for (let i = 0; i < n; i++) v.setInt16(44 + i * 2, Math.max(-1, Math.min(1, fn(i, sr))) * 32767, true);
      return buf;
    };
    const files = {
      '/probe/sine10.wav': wav(10, (i, sr) => 0.5 * Math.sin((2 * Math.PI * 220 * i) / sr)),
      '/probe/sine1.wav': wav(1, (i, sr) => 0.5 * Math.sin((2 * Math.PI * 220 * i) / sr)),
      '/probe/dc2.wav': wav(2, () => 0.5),
      '/probe/click.wav': wav(0.5, (i) => (i < 20 ? 0.9 : 0)),
    };
    window.electronAPI = { ...(window.electronAPI || {}), projectSetTitle: async () => {}, sampleReadFile: async (p) => files[p].slice(0) };
    window.__mods = {
      eng: await import('/src/services/ArrangementEngine.ts'),
      nr: await import('/src/services/NoteRepeat.ts'),
      undo: await import('/src/services/UndoManager.ts'),
      exp: await import('/src/services/ExportService.ts'),
      taf: await import('/src/utils/trackAudioFactory.ts'),
      mb: await import('/src/audio/masterBus.ts'),
      sk: await import('/src/audio/sampler/SamplerKit.ts'),
      bc: await import('/src/audio/sampler/bufferCache.ts'),
      fk: await import('/src/audio/sampler/factoryKit.ts'),
      tpl: await import("/src/data/templates/index.ts"), ck: await import("/src/audio/sampler/classic/index.ts"), kp: await import("/src/components/packs/kitPreview.ts"), ah: await import("/src/utils/arrangementHelpers.ts"),
    };
    const an = new Tone.Analyser('waveform', 2048);
    Tone.getDestination().connect(an);
    window.__peak = async (ms) => {
      let max = 0;
      const end = performance.now() + ms;
      while (performance.now() < end) {
        for (const v of an.getValue()) max = Math.max(max, Math.abs(v));
        await new Promise((r) => setTimeout(r, 8));
      }
      return max > 0 ? Math.round(20 * Math.log10(max)) : -Infinity;
    };
    window.__wait = (ms) => new Promise((r) => setTimeout(r, ms));
    window.__hook = () => {
      const e = window.__microDaw.arrangementEngine;
      const s = e.trackAudio.get(e.getActiveDrumTrackId()).sampler;
      if (!s.__hooked) {
        const orig = s.trigger.bind(s);
        const origRel = s.release.bind(s);
        s.__log = [];
        s.__rel = [];
        s.trigger = (pad, vel, time) => { s.__log.push({ pad, time, now: Tone.immediate(), beat: window.__beatAt(time) }); return orig(pad, vel, time); };
        s.release = (pad, time) => { s.__rel.push({ pad, time, now: Tone.immediate() }); return origRel(pad, time); };
        s.__hooked = true;
      }
      return s;
    };
    window.__fresh = () => {
      const e = window.__microDaw.arrangementEngine;
      e.stop();
      e.resetToStarter();
      e.setBpm(120);
      return e.ensureDrumTrack();
    };
    window.__beatAt = (t) => { const tr = Tone.getTransport(); return tr.getTicksAtTime(t) / tr.PPQ; };
  });


  const run = async (name, fn) => {
    try {
      log(name + ' ' + JSON.stringify(await page.evaluate(fn)));
    } catch (err) {
      log(name + ' THREW ' + String(err).slice(0, 400));
    }
  };

  await run('SANITY', async () => ({
    lookAhead: window.__Tone.getContext().lookAhead,
    sr: window.__Tone.getContext().sampleRate,
    mods: Object.keys(window.__mods).filter((k) => !window.__mods[k]),
  }));

  await run('POS-GLITCH play -> stop -> idle 2s -> play', async () => {
    const d = window.__microDaw, e = d.arrangementEngine, wait = window.__wait;
    window.__fresh();
    e.setLoopEnabled(false);
    const out = {};
    for (const how of ['play', 'playPattern']) {
      e[how](); await wait(1000); e.stop(); await wait(2000);
      const seen = []; const cb = e.onPositionChange((b) => seen.push(+b.toFixed(2)));
      e[how]();
      const polled = [];
      const t0 = performance.now();
      while (performance.now() - t0 < 260) { polled.push(+e.getPosition().toFixed(2)); await wait(16); }
      cb(); e.stop();
      out[how] = { polled, fromPositionCallback: seen.slice(0, 14) };
    }
    return out;
  });
  await run('ERRORS', async () => window.__errs);
};
