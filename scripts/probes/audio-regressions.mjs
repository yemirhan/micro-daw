// Real-Tone audio/timing regressions (review round 4: B1 and B3). The vitest
// Tone mock cannot show these; this runs them against real Tone in headless
// Chromium and fails (exit 1) when an expectation is not met.
//
//   npm run test:audio
//   (= node scripts/ui-snap.mjs --mode daw --steps scripts/probes/audio-regressions.mjs)
import recordRestart from './b1-record-restart.mjs';
import positionGlitch from './b1-position-glitch.mjs';
import songAutostop from './b1-song-autostop.mjs';
import packReload from './b3-pack-reload.mjs';

export default async (page, ctx) => {
  const lines = [];
  const log = (line) => {
    lines.push(line);
    ctx.log(line);
  };
  const sub = { ...ctx, log };
  for (const probe of [recordRestart, positionGlitch, songAutostop, packReload]) await probe(page, sub);

  const failures = [];
  const expect = (ok, what) => {
    if (!ok) failures.push(what);
  };
  const parsed = (prefix) =>
    lines
      .filter((l) => l.startsWith(prefix))
      .map((l) => {
        const json = l.slice(l.indexOf(' {'));
        try {
          return JSON.parse(json);
        } catch {
          return null;
        }
      });

  for (const l of lines) expect(!/ THREW /.test(l), `probe threw: ${l.slice(0, 200)}`);
  for (const l of lines.filter((x) => x.startsWith('ERRORS '))) expect(l === 'ERRORS []', `page errors: ${l}`);

  // B1(c): hits within ~100 ms of (re)starting Record land at beat 0.
  const restarts = [...parsed('RESTART '), ...parsed('PLAYING -> record'), ...parsed('GATE held')];
  expect(restarts.length >= 6, `expected 6 record-restart cases, got ${restarts.length}`);
  for (const r of restarts) expect(r?.recorded?.[0]?.s === 0, `record restart hit not at beat 0: ${JSON.stringify(r)}`);

  // B1(b): no stale playhead frames after play -> stop -> idle -> play.
  for (const g of parsed('POS-GLITCH')) {
    for (const [how, v] of Object.entries(g ?? {})) {
      for (const series of [v.polled, v.fromPositionCallback]) {
        expect(Array.isArray(series) && series.length > 0, `${how}: no positions`);
        expect(Math.max(...series.slice(0, 6)) < 0.25, `${how}: stale position frames ${JSON.stringify(series)}`);
      }
    }
  }

  // B1(a): Song Play (loop off) after a long idle keeps playing from 0.
  for (const a of parsed('AUTOSTOP')) {
    for (const [k, v] of Object.entries(a ?? {})) {
      expect(v.finalState === 'playing', `${k}: song auto-stopped (${v.finalState})`);
      expect(v.states.every((s) => parseFloat(s.split('@')[1]) < 4), `${k}: stale start ${JSON.stringify(v.states)}`);
      expect(v.hitsHeard > 0, `${k}: no hits heard`);
    }
  }

  // B3: a Song audio clip from a pack survives a pack update / removal.
  const [b3] = parsed('B3 ');
  expect(!!b3, 'B3 probe did not report');
  if (b3) {
    expect(b3.exportPeakV1 > 0 && b3.exportPeakV2 > 0 && b3.exportPeakRemoved > 0, `B3: silent export ${JSON.stringify(b3)}`);
    expect(b3.livePlayer?.swapped === true, 'B3: running player kept the old audio after the pack update');
    expect(b3.sameId === true, 'B3: sample id changed');
    expect(b3.unavailable === true, 'B3: removed pack sample not flagged unavailable');
    expect(Number.isFinite(b3.livePeakDbAfterRemoval), 'B3: silent playback after the pack was removed');
  }

  if (failures.length) throw new Error(`audio regressions failed:\n  - ${failures.join('\n  - ')}`);
  ctx.log('AUDIO REGRESSIONS OK');
};
