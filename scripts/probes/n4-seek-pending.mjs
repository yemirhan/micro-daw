// N4 (review round 5): seeking right after Play (before the scheduled start takes effect) must
// start from the new position: no playhead frame near 0 and no beat-0 step. Real Tone.
import { setup } from './setup.mjs';

export default async (page, { log }) => {
  await setup(page);
  for (const delay of [0, 30, 70]) {
    try {
      const out = await page.evaluate(async (delay) => {
        const e = window.__microDaw.arrangementEngine, wait = window.__wait;
        const id = window.__fresh();
        const f = (x) => +x.toFixed(3);
        e.setBpm(120);
        // Pad 0 only at beat 0 (before the seek target), pad 1 at beat 3 (the target).
        e.toggleStep(id, 0, 0, 0.25);
        e.toggleStep(id, 1, 12, 0.25);
        const smp = window.__hook(); await smp.ready; smp.__log.length = 0;
        e.play();
        if (delay) await wait(delay);
        const callsBeforeSeek = smp.__log.length;
        e.seek(3);
        // Pad-0 voices still alive right after the seek: the beat-0 step will be heard.
        const staleVoices = smp.voices[0].length;
        const pos = [];
        const end = performance.now() + 400;
        while (performance.now() < end) { pos.push(f(e.getPosition())); await wait(25); }
        // Trigger CALLS made before the seek were scheduled from the old position; the restart
        // cancels them (staleVoices). Calls after the seek must all be at/after the target.
        const triggers = window.__hook().__log.slice(callsBeforeSeek).map((x) => ({ beat: f(x.beat), late: f(x.now - x.time) }));
        const state = e.getState();
        e.stop();
        return { state, positions: pos, triggers, callsBeforeSeek, staleVoices };
      }, delay);
      log(`SEEK-PENDING ${delay}ms ` + JSON.stringify(out));
    } catch (err) {
      log(`SEEK-PENDING ${delay}ms THREW ` + String(err).slice(0, 600));
    }
  }
};
