//! The effect chains against Chromium: `rune/parity/fixtures/fx_chains.json`, captured by
//! `rune/parity/chromium/capture-fx.steps.mjs` from the app's real code (track strips through
//! `createTrackAudioNodes` + `applyTrackEffect` / `resetTrackEffects` / `scheduleAutomation`,
//! the live bus through a fresh `AudioEngine` and its setters, with real Tone.js).
//!
//! Each case is replayed on the real engine [`Core`]: the input buffer plays through a slot's
//! instrument into its [`TrackStrip`] / [`LiveFx`]; every step becomes the command the engine
//! sends at that time (`StripOp`, pan, `LiveOp`); a `play` step resets the strip, applies the
//! automation pre-pass and starts the transport with the automation in the slot's schedule, so the
//! callbacks fire from the core's tick path. The live cases run through the master bus, as
//! `AudioEngine` does. Compared over the whole render with the block evidence of stats.mjs.

use std::sync::Arc;

use mdaw_dsp::factory::Mulberry32;
use mdaw_model::types::{EffectParams, FilterType, LfoWaveform, Track};
use serde_json::{Value, json};

use super::automation;
use super::live::{LiveFx, LiveOp};
use super::state::FxState;
use super::track::{Effect, StripOp};
use crate::contract::Instrument;
use crate::core::{
    Command, Core, CoreConfig, CoreHandle, SlotFx, SynthInstrument, TrackInstrument, TrackSchedule,
};

const EV_BLOCK: usize = 1024;
const PROJ: usize = 16;
const PROJ_SEED: u32 = 0x9a0f_0001;
/// Full-render error energy vs the capture.
const MAX_ERR_DB: f64 = -60.0;
/// Per 1024-frame block above −90 dBFS: energy within this many dB.
const MAX_BLOCK_ENERGY_DB: f64 = 0.5;
const LEVEL_FLOOR_DB: f64 = -90.0;

fn fixture() -> Value {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../parity/fixtures/fx_chains.json"
    );
    let p = std::env::var("FX_FIXTURE").unwrap_or(p.to_owned());
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap()
}

/// `genSignal` of capture-fx.steps.mjs (= capture-compressor.steps.mjs).
fn gen_signal(parts: &Value, frames: usize, sr: f64) -> [Vec<f32>; 2] {
    let mut out = [vec![0f64; frames], vec![0f64; frames]];
    let two_pi = 2.0 * std::f64::consts::PI;
    for p in parts.as_array().unwrap() {
        for (ch, x) in out.iter_mut().enumerate() {
            let g = f(&p["amp"][ch]);
            let start = (f(&p["at"]) * sr).round() as usize;
            match p["k"].as_str().unwrap() {
                "sine" => {
                    let end = frames.min((f(&p["end"]) * sr).round() as usize);
                    let (freq, phase) = (f(&p["freq"]), f(&p["phase"][ch]));
                    for (i, v) in x.iter_mut().enumerate().take(end).skip(start) {
                        *v += g * (two_pi * freq * ((i - start) as f64 / sr) + phase).sin();
                    }
                }
                kind => {
                    let period = (f(&p["period"]) * sr).round() as usize;
                    let count = p["count"].as_u64().unwrap() as usize;
                    let end = frames.min(start + count * period);
                    let decay = f(&p["decay"]);
                    let mut rng = (kind == "noise").then(|| {
                        Mulberry32::new(
                            (p["seed"].as_u64().unwrap() as u32).wrapping_add(ch as u32),
                        )
                    });
                    let freq = p["freq"].as_f64().unwrap_or(0.0);
                    for (i, v) in x.iter_mut().enumerate().take(end).skip(start) {
                        let tt = ((i - start) % period) as f64 / sr;
                        let env = (-tt / decay).exp();
                        *v += match &mut rng {
                            Some(r) => g * (r.next_f64() * 2.0 - 1.0) * env,
                            None => g * env * (two_pi * freq * tt).sin(),
                        };
                    }
                }
            }
        }
    }
    out.map(|x| x.into_iter().map(|v| v as f32).collect())
}

/// `blockEvidence` of stats.mjs.
fn block_evidence(x: &[f32]) -> Vec<[f64; PROJ + 1]> {
    let mut rng = Mulberry32::new(PROJ_SEED);
    x.chunks(EV_BLOCK)
        .map(|b| {
            let mut out = [0f64; PROJ + 1];
            out[0] = b.iter().map(|&v| v as f64 * v as f64).sum();
            for p in &mut out[1..] {
                for w in b.chunks(32) {
                    let word = (rng.next_f64() * 4_294_967_296.0) as u32;
                    for (t, &v) in w.iter().enumerate() {
                        let v = v as f64;
                        *p += if (word >> t) & 1 == 1 { -v } else { v };
                    }
                }
            }
            out
        })
        .collect()
}

/// Per block: (error energy estimate, reference energy, |energy difference| dB or 0 below the
/// floor). Blocks from `from` on (`until` excluded).
fn block_errors(x: &[f32], reference: &[f64]) -> Vec<(f64, f64, f64)> {
    let ours = block_evidence(x);
    assert_eq!(ours.len() * (PROJ + 1), reference.len(), "evidence size");
    let floor = 10f64.powf(LEVEL_FLOOR_DB / 10.0);
    ours.iter()
        .zip(reference.chunks(PROJ + 1))
        .enumerate()
        .map(|(i, (o, r))| {
            let len = EV_BLOCK.min(x.len() - i * EV_BLOCK) as f64;
            let err = (1..=PROJ).map(|k| (o[k] - r[k]).powi(2)).sum::<f64>() / PROJ as f64;
            let d = if r[0] / len >= floor {
                (10.0 * (o[0].max(1e-300) / r[0]).log10()).abs()
            } else {
                0.0
            };
            (err, r[0], d)
        })
        .collect()
}

/// The capture's input, played into the slot from frame 0 (a stand-in instrument).
struct Feed {
    l: Arc<Vec<f32>>,
    r: Arc<Vec<f32>>,
}

impl Instrument for Feed {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let f0 = frame as usize;
        for k in 0..left.len() {
            left[k] += self.l.get(f0 + k).copied().unwrap_or(0.0);
            right[k] += self.r.get(f0 + k).copied().unwrap_or(0.0);
        }
    }
}

impl SynthInstrument for Feed {
    fn note_on(&mut self, _: f64, _: f64, _: f64) {}
    fn note_off(&mut self, _: f64, _: f64) {}
    fn release_all(&mut self, _: f64) {}
}

/// The case's track (the page's audio-track defaults + `case.track`).
fn case_track(case: &Value) -> Value {
    let mut t = json!({
        "id": "t", "name": "t", "instrument": {"type": "audio", "presetIndex": 0}, "regions": [],
        "volume": 0, "pan": 0, "muted": false, "solo": false, "color": ""
    });
    for (k, v) in case["track"].as_object().unwrap() {
        t[k] = v.clone();
    }
    t
}

fn track_of(t: &Value) -> Track {
    serde_json::from_value(t.clone()).unwrap()
}

fn effect_of(name: &str) -> Effect {
    serde_json::from_value(Value::String(name.into())).unwrap()
}

fn live_op(name: &str, v: &Value) -> LiveOp {
    let n = || f(v);
    match name {
        "setReverbWet" => LiveOp::ReverbWet(n()),
        "setChorusDepth" => LiveOp::ChorusDepth(n()),
        "setFilterCutoff" => LiveOp::FilterCutoff(n()),
        "setFilterResonance" => LiveOp::FilterResonance(n()),
        "setFilterType" => {
            LiveOp::FilterType(serde_json::from_value::<FilterType>(v.clone()).unwrap())
        }
        "setFilterLfoRate" => LiveOp::FilterLfoRate(n()),
        "setFilterLfoDepth" => LiveOp::FilterLfoDepth(n()),
        "setFilterLfoWave" => {
            LiveOp::FilterLfoWave(serde_json::from_value::<LfoWaveform>(v.clone()).unwrap())
        }
        "setDelayTime" => LiveOp::DelayTime(n()),
        "setDelayFeedback" => LiveOp::DelayFeedback(n()),
        "setDelayWet" => LiveOp::DelayWet(n()),
        "setDistortionAmount" => LiveOp::DistortionAmount(n()),
        "setDistortionWet" => LiveOp::DistortionWet(n()),
        "setEqLow" => LiveOp::EqLow(n()),
        "setEqMid" => LiveOp::EqMid(n()),
        "setEqHigh" => LiveOp::EqHigh(n()),
        "setCompThreshold" => LiveOp::CompThreshold(n()),
        "setCompRatio" => LiveOp::CompRatio(n()),
        "setCompAttack" => LiveOp::CompAttack(n()),
        "setCompRelease" => LiveOp::CompRelease(n()),
        "setVolume" => LiveOp::Volume(n()),
        "mute" => LiveOp::Mute(true),
        "unmute" => LiveOp::Mute(false),
        other => panic!("live setter {other}"),
    }
}

fn send_strip(h: &mut CoreHandle, t: f64, op: StripOp) {
    h.send(
        Some(t),
        Command::StripOp {
            slot: 0,
            op: Box::new(op),
        },
    );
}

/// Render a case on the core (see the module docs).
fn render_case(case: &Value) -> [Vec<f32>; 2] {
    let sr = case["sr"].as_u64().unwrap() as u32;
    let frames = case["frames"].as_u64().unwrap() as usize;
    let [il, ir] = gen_signal(&case["signal"], frames, sr as f64);
    let mut config = CoreConfig::new(sr);
    config.max_tracks = 1;
    let (mut core, mut h, _live) = Core::new(config);
    let feed = Feed {
        l: Arc::new(il),
        r: Arc::new(ir),
    };
    h.set_instrument(0, Some(TrackInstrument::Synth(Box::new(feed))));
    let steps = case["steps"].as_array().unwrap();
    if case["kind"] == "strip" {
        let bpm = f(&case["bpm"]);
        h.set_bpm(bpm);
        let mut tv = case_track(case);
        let track = track_of(&tv);
        let fx = FxState::from(&track.effects.clone().unwrap_or_default());
        let strip = h.new_track_strip(&fx, track.pan, track.volume);
        h.set_track_strip(0, Some(strip));
        for step in steps {
            let t = f(&step["frame"]) / sr as f64;
            for op in step["ops"].as_array().unwrap() {
                let track = track_of(&tv);
                let fx = FxState::from(&track.effects.clone().unwrap_or_default());
                let reset = StripOp::Reset {
                    fx,
                    pan: track.pan,
                    volume_db: track.volume,
                };
                match op["op"].as_str().unwrap() {
                    "apply" => {
                        let name = op["name"].as_str().unwrap();
                        for (k, v) in op["params"].as_object().unwrap() {
                            tv["effects"][name][k] = v.clone();
                        }
                        let fx = FxState::from(&track_of(&tv).effects.unwrap());
                        send_strip(&mut h, t, StripOp::Apply(effect_of(name), fx));
                    }
                    "track" => {
                        for (k, v) in op["patch"].as_object().unwrap() {
                            tv[k] = v.clone();
                        }
                    }
                    "reset" => send_strip(&mut h, t, reset),
                    "play" => {
                        send_strip(&mut h, t, reset);
                        for o in automation::pre_ops(&track) {
                            send_strip(&mut h, t, o);
                        }
                        let s = TrackSchedule::new(false, Vec::new())
                            .with_automation(automation::events(&track, bpm));
                        h.send(
                            Some(t),
                            Command::SetSchedule {
                                slot: 0,
                                schedule: Some(Arc::new(s)),
                            },
                        );
                        h.send(
                            Some(t),
                            Command::Start {
                                at: Some(t),
                                beat: 0.0,
                                generation: 1,
                            },
                        );
                    }
                    "pan" => h.send(
                        Some(t),
                        Command::SetTrackPan {
                            slot: 0,
                            pan: f(&op["v"]) as f32,
                        },
                    ),
                    "volume" => send_strip(&mut h, t, StripOp::Volume(f(&op["v"]))),
                    // mute is a gate after the strip in both apps (`setTrackMuted`)
                    "mute" => h.send(
                        Some(t),
                        Command::SetTrackMute {
                            slot: 0,
                            muted: op["m"].as_bool().unwrap(),
                        },
                    ),
                    other => panic!("strip op {other}"),
                }
            }
        }
    } else {
        let mut p = EffectParams::default();
        for o in case["init"].as_array().unwrap() {
            live_op(o["op"].as_str().unwrap(), &o["v"]).update(&mut p);
        }
        h.set_master_bus(Some(0.0));
        let fx = LiveFx::new(&p, sr, 0.0);
        h.set_track_strip(0, Some(SlotFx::Live(Box::new(fx))));
        for step in steps {
            let t = f(&step["frame"]) / sr as f64;
            for o in step["ops"].as_array().unwrap() {
                let op = live_op(o["op"].as_str().unwrap(), &o["v"]);
                h.send(Some(t), Command::LiveOp { slot: 0, op });
            }
        }
    }
    let mut l = vec![0f32; frames];
    let mut r = vec![0f32; frames];
    core.process(&mut l, &mut r);
    while h.poll_event().is_some() {}
    h.collect_garbage();
    [l, r]
}

#[test]
fn effect_chains_match_chromium() {
    // The references have the Electron app's distortion (see `Distortion`'s docs).
    super::effects::CLASSIC_FOR_REFERENCE.with(|c| c.set(true));
    let fx = fixture();
    let mut failures = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = render_case(case);
        let n = case["frames"].as_u64().unwrap() as usize;
        let blocks = n / EV_BLOCK;
        let (mut err, mut sig, mut worst, mut worst_at) = (0.0, 0.0, 0f64, 0usize);
        for (ch, x) in out.iter().enumerate() {
            let reference: Vec<f64> = case["evidence"][ch]
                .as_array()
                .unwrap()
                .iter()
                .map(f)
                .collect();
            for (i, (e, s, d)) in block_errors(x, &reference).into_iter().enumerate() {
                if i >= blocks {
                    break;
                }
                err += e;
                sig += s;
                if d > worst {
                    worst = d;
                    worst_at = i;
                }
            }
        }
        let db = 10.0 * (err.max(1e-300) / sig).log10();
        eprintln!(
            "{id:>18} @ {}: full-render error {db:.1} dB, worst block energy {worst:.2e} dB (at {:.3} s)",
            case["sr"],
            (worst_at * EV_BLOCK) as f64 / f(&case["sr"])
        );
        if db > MAX_ERR_DB || worst > MAX_BLOCK_ENERGY_DB {
            failures.push(format!("{id}: {db:.1} dB, worst block {worst:.3} dB"));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// RMS (dBFS, both channels) of `x` over `[a, b)`.
fn rms_db(x: &[Vec<f32>; 2], a: usize, b: usize) -> f64 {
    let mut acc = 0f64;
    for ch in x {
        for v in &ch[a..b] {
            acc += (*v as f64).powi(2);
        }
    }
    let rms = (acc / (2 * (b - a)) as f64).sqrt();
    if rms > 1e-10 {
        20.0 * rms.log10()
    } else {
        -200.0
    }
}

/// The capture's `stepRms` entry starting at `frame`.
fn capture_rms(case: &Value, frame: u64) -> f64 {
    case["stepRms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e[0].as_u64() == Some(frame))
        .map(|e| f(&e[1]))
        .unwrap()
}

/// Electron bug fixed in both apps (user decision 2026-11-18): a volume write and a play-start
/// reset on a muted track keep it muted (`strip_mute_volume`: muted at 0.5 s, volume at 1.0 s,
/// reset at 1.2 s, un-muted at 1.5 s); so does `setVolume` on the muted live synth
/// (`live_mute_volume`). The capture and the render agree: silent until the un-mute.
#[test]
fn a_volume_change_keeps_a_track_and_the_live_synth_muted() {
    let fx = fixture();
    for (id, steps) in [
        ("strip_mute_volume", &[24_064u64, 48_000, 57_600][..]),
        ("live_mute_volume", &[24_064, 48_000][..]),
    ] {
        let case = fx["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id)
            .unwrap();
        for &s in steps {
            assert_eq!(capture_rms(case, s), -200.0, "{id} @ {s}: muted in the app");
        }
        assert!(
            capture_rms(case, 72_064) > -30.0,
            "{id}: audible after un-muting"
        );
        let out = render_case(case);
        assert_eq!(rms_db(&out, 24_064, 72_064), -200.0, "{id}: muted here");
        assert!(rms_db(&out, 72_064, 96_000) > -30.0, "{id}: audible here");
    }
}

/// Electron bug fixed in both apps (user decision 2026-11-18): removing the filter LFO clears the
/// override its connection set on `filter.frequency`, so the static cutoff is restored instead
/// of 0 Hz and the lowpass keeps passing (`strip_apply`: LFO off at 3.3 s; before the fix the
/// capture fell from −21.6 to −30.5 dBFS there and the tail to −41.9 dBFS).
#[test]
fn lfo_off_restores_the_static_cutoff() {
    let fx = fixture();
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "strip_apply")
        .unwrap();
    let out = render_case(case);
    let (lfo_off, end) = (158_464, 192_000);
    // the last 100 ms before the LFO went off vs the last 100 ms of the render
    let before = rms_db(&out, lfo_off - 4_800, lfo_off);
    let after = rms_db(&out, end - 4_800, end);
    eprintln!("strip_apply: {before:.1} dBFS before the LFO went off, {after:.1} dBFS at the end");
    assert!(after > before - 6.0, "{before:.1} → {after:.1} dBFS");
    // the capture's steps: [.., 148864, 158464]
    assert!(capture_rms(case, 158_464) > capture_rms(case, 148_864) - 3.0);
}

/// Sample-level comparison against raw captures (`PARITY_DEBUG_DIR`, see the steps script).
#[test]
#[ignore]
fn fx_parity_debug() {
    let Ok(dir) = std::env::var("PARITY_DEBUG_DIR") else {
        return;
    };
    let only = std::env::var("CASE").ok();
    let fx = fixture();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        if only.as_deref().is_some_and(|o| o != id) {
            continue;
        }
        let Ok(bytes) = std::fs::read(format!("{dir}/{id}.f32")) else {
            continue;
        };
        let raw: Vec<f32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        let frames = raw.len() / 2;
        let out = render_case(case);
        let seg: usize = std::env::var("SEG").map_or(4800, |v| v.parse().unwrap());
        let mut s = Vec::new();
        for a in (0..frames).step_by(seg) {
            let (mut e, mut g, mut o) = (0f64, 0f64, 0f64);
            for ch in 0..2 {
                for i in a..(a + seg).min(frames) {
                    e += (out[ch][i] as f64 - raw[ch * frames + i] as f64).powi(2);
                    g += (raw[ch * frames + i] as f64).powi(2);
                    o += (out[ch][i] as f64).powi(2);
                }
            }
            s.push(format!(
                "{:.2}s:{:.0}({:+.1})",
                a as f64 / f(&case["sr"]),
                10.0 * (e.max(1e-300) / g.max(1e-300)).log10(),
                10.0 * (o.max(1e-300) / g.max(1e-300)).log10()
            ));
        }
        if let Ok(idx) = std::env::var("IDX") {
            for i in idx.split(',').map(|v| v.parse::<usize>().unwrap()) {
                eprintln!(
                    "  [{i}] L ours {:e} theirs {:e} | R ours {:e} theirs {:e}",
                    out[0][i],
                    raw[i],
                    out[1][i],
                    raw[frames + i]
                );
            }
        }
        if let Ok(thr) = std::env::var("GLITCH") {
            let thr: f32 = thr.parse().unwrap();
            for ch in 0..2 {
                let g: Vec<usize> = (0..frames)
                    .filter(|&i| (out[ch][i] - raw[ch * frames + i]).abs() > thr)
                    .take(40)
                    .collect();
                eprintln!("  ch{ch} diffs > {thr}: {g:?}");
            }
        }
        let first = (0..frames).find(|&i| out[0][i] != raw[i]);
        eprintln!(
            "{id}: first L diff at {first:?}\n  per 100 ms error dB: {}",
            s.join(" ")
        );
    }
}
