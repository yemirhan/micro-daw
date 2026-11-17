// N1 (review round 5): Stop during a Record count-in must cancel the pending transport start.
// Cases: stop mid count-in then wait (transport must stay stopped), then Record again (pattern
// and hits start at the NEW count-in end), then Play (playhead must not jump back when the
// first count-in would have ended). Real Tone; assertions live in audio-regressions.mjs.
import { setup } from './setup.mjs';

export default async (page, { log }) => {
  await setup(page);
  const run = async (name, fn) => {
    try {
      log(name + ' ' + JSON.stringify(await page.evaluate(fn)));
    } catch (err) {
      log(name + ' THREW ' + String(err).slice(0, 600));
    }
  };
  const pre = `
    const d = window.__microDaw, e = d.arrangementEngine, Tone = window.__Tone, wait = window.__wait;
    const id = window.__fresh();
    const tr = Tone.getTransport();
    const f = (x) => +x.toFixed(3);
    e.setBpm(120);
    e.setLoopSettings({ bars: 1, countInBars: 1, metronome: false, quantize: '1/16', overdub: true });
    for (const s of [0, 4, 8, 12]) e.toggleStep(id, 0, s, 0.25);
    const smp = window.__hook(); await smp.ready; smp.__log.length = 0;
    const notes = (n) => e.getPatternRegion(id).notes.filter((x) => x.note === n).map((x) => ({ s: f(x.startBeat), d: f(x.durationBeats) }));
  `;
  const runSrc = (name, body) => run(name, new Function('return (async () => {' + pre + body + '})()'));

  await runSrc('COUNTIN-ABORT wait', `
    e.startLoopRecording();
    const cie = e.loopRecorder.take.countInEnd;
    await wait(500);
    e.stop();
    while (Tone.immediate() < cie + 0.6) await wait(20);
    const out = {
      engineState: e.getState(),
      toneTransportState: tr.state,
      transportSeconds: f(tr.seconds),
      triggersWhileStopped: smp.__log.length,
    };
    e.stop();
    return out;
  `);

  await runSrc('COUNTIN-ABORT record', `
    e.clearPattern(id);
    for (const s of [0, 4, 8, 12]) e.toggleStep(id, 0, s, 0.25);
    e.startLoopRecording();
    await wait(600);
    e.stop();
    await wait(150);
    e.startLoopRecording();
    const hooked = window.__hook(); hooked.__log.length = 0;
    const cie2 = e.loopRecorder.take.countInEnd;
    while (Tone.immediate() < cie2 + 0.02) await wait(4);
    d.simulateNoteOn(37, 0.9, 10);
    await wait(40);
    d.simulateNoteOff(37, 10);
    await wait(300);
    const trig = hooked.__log.filter((x) => x.pad === 0).map((x) => ({ relCountInEnd: f(x.time - cie2), beat: f(x.beat) }));
    const r = notes(37);
    e.stop();
    return { patternTriggers: trig.slice(0, 3), recorded: r };
  `);

  await runSrc('COUNTIN-ABORT play', `
    e.startLoopRecording();
    const cie1 = e.loopRecorder.take.countInEnd;
    await wait(400);
    e.stop();
    await wait(100);
    e.play();
    const pos = [];
    while (Tone.immediate() < cie1 + 0.4) {
      pos.push({ t: f(Tone.immediate() - cie1), p: f(e.getPosition()) });
      await wait(30);
    }
    const around = pos.filter((x) => x.t > -0.3 && x.t < 0.4);
    let maxBackJump = 0;
    for (let i = 1; i < around.length; i++) maxBackJump = Math.max(maxBackJump, around[i - 1].p - around[i].p);
    const st = e.getState();
    e.stop();
    return { state: st, maxBackJump: f(maxBackJump), around };
  `);
  await run('ERRORS', async () => window.__errs);
};
