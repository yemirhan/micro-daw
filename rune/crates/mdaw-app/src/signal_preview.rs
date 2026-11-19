//! `hooks/useSignalPreview.ts`: while the Samples view's Generate tab shows, a test-signal synth
//! (a `Tone.PolySynth(Tone.Synth{oscillator: type})`, or a `NoiseSynth` for noise, envelope
//! A .01 / D .1 / S .8 / R .3, into `Tone.Volume(gainToDb(level))`) plays on the engine's preview
//! slot; the live synth and the drums are muted (`audioEngine.mute()`, `drumEngine.mute()`).
//! "Hold to preview" plays the frequency quantised to the nearest note at velocity 0.8; MIDI
//! note-ons play at their pitch (velocity / 127), set the frequency to `round(mtof(note))` and
//! show the note name; note-offs (or velocity 0) release.
//!
//! The voice is Chromium's band-limited oscillator (the engine's `fx::osc::Osc`) with Tone's
//! envelope shapes: linear attack, `exponentialApproach` decay to the sustain and release to 0
//! (`τ = ln(t + 1)/ln 200`, linear over the last 10 %). The level is read per quantum from a
//! shared atomic, so moving the Level slider changes a sounding note like `volume.value = …`.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use mdaw_dsp::factory::osc::OscType;
use mdaw_engine::contract::Instrument;
use mdaw_engine::core::{SynthInstrument, SynthNoteOp};
use mdaw_engine::fx::osc::{Osc, Tables};
use mdaw_engine::sample_processor::SignalType;

use crate::app::App;

const ATTACK: f64 = 0.01;
const DECAY: f64 = 0.1;
const SUSTAIN: f64 = 0.8;
const RELEASE: f64 = 0.3;
const VOICES: usize = 8;

fn approach_tc(t: f64) -> f64 {
    (t + 1.0).ln() / 200f64.ln()
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Idle,
    On { start: f64, vel: f64 },
    Off { start: f64, from: f64 },
}

struct Voice {
    note: f64,
    stage: Stage,
    osc: Option<Osc>,
    noise: u64,
}

/// The preview synth (audio side).
pub struct PreviewSynth {
    kind: SignalType,
    sr: f64,
    voices: Vec<Voice>,
    level: Arc<AtomicU32>,
}

impl PreviewSynth {
    pub fn new(kind: SignalType, sr: u32, level: Arc<AtomicU32>) -> Self {
        let tables = Tables::new(sr);
        let shape = match kind {
            SignalType::Square => Some(OscType::Square),
            SignalType::Sawtooth => Some(OscType::Sawtooth),
            SignalType::Triangle => Some(OscType::Triangle),
            SignalType::Sine => Some(OscType::Sine),
            SignalType::WhiteNoise => None,
        };
        Self {
            kind,
            sr: sr as f64,
            voices: (0..VOICES)
                .map(|i| Voice {
                    note: -1.0,
                    stage: Stage::Idle,
                    osc: shape.map(|sh| Osc::new(tables.clone(), sh, false, sr as f64)),
                    noise: 0x9e37_79b9_7f4a_7c15 ^ (i as u64 + 1),
                })
                .collect(),
            level,
        }
    }

    /// The envelope at time `t` (seconds since context start).
    fn env(stage: Stage, t: f64) -> f64 {
        match stage {
            Stage::Idle => 0.0,
            Stage::On { start, vel } => {
                let dt = t - start;
                let v = if dt < ATTACK {
                    dt / ATTACK
                } else {
                    let d = dt - ATTACK;
                    if d >= DECAY {
                        SUSTAIN
                    } else if d >= DECAY * 0.9 {
                        let at =
                            SUSTAIN + (1.0 - SUSTAIN) * (-(DECAY * 0.9) / approach_tc(DECAY)).exp();
                        at + (SUSTAIN - at) * ((d - DECAY * 0.9) / (DECAY * 0.1))
                    } else {
                        SUSTAIN + (1.0 - SUSTAIN) * (-d / approach_tc(DECAY)).exp()
                    }
                };
                v * vel
            }
            Stage::Off { start, from } => {
                let d = t - start;
                if d >= RELEASE {
                    0.0
                } else if d >= RELEASE * 0.9 {
                    let at = from * (-(RELEASE * 0.9) / approach_tc(RELEASE)).exp();
                    at * (1.0 - (d - RELEASE * 0.9) / (RELEASE * 0.1))
                } else {
                    from * (-d / approach_tc(RELEASE)).exp()
                }
            }
        }
    }
}

impl Instrument for PreviewSynth {
    fn process(&mut self, frame: u64, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let gain = f32::from_bits(self.level.load(Ordering::Relaxed));
        for v in &mut self.voices {
            if v.stage == Stage::Idle {
                continue;
            }
            let freq = (440.0 * 2f64.powf((v.note - 69.0) / 12.0)) as f32;
            for k in 0..n {
                let t = (frame + k as u64) as f64 / self.sr;
                let e = Self::env(v.stage, t) as f32;
                let x = match v.osc.as_mut() {
                    Some(o) => o.tick(freq),
                    None => {
                        v.noise ^= v.noise << 13;
                        v.noise ^= v.noise >> 7;
                        v.noise ^= v.noise << 17;
                        ((v.noise >> 11) as f64 / (1u64 << 53) as f64 * 2.0 - 1.0) as f32
                    }
                };
                let y = x * e * gain;
                left[k] += y;
                right[k] += y;
            }
            if let Stage::Off { start, .. } = v.stage
                && (frame + n as u64) as f64 / self.sr >= start + RELEASE
            {
                v.stage = Stage::Idle;
            }
        }
    }
}

impl SynthInstrument for PreviewSynth {
    fn note_on(&mut self, note: f64, velocity: f64, time: f64) {
        // NoiseSynth is monophonic: every attack restarts the one voice.
        let i = if self.kind == SignalType::WhiteNoise {
            0
        } else {
            self.voices
                .iter()
                .position(|v| v.note == note && v.stage != Stage::Idle)
                .or_else(|| self.voices.iter().position(|v| v.stage == Stage::Idle))
                .unwrap_or(0)
        };
        let v = &mut self.voices[i];
        v.note = note;
        v.stage = Stage::On {
            start: time,
            vel: velocity,
        };
    }
    fn note_off(&mut self, note: f64, time: f64) {
        for v in &mut self.voices {
            if (self.kind == SignalType::WhiteNoise || v.note == note)
                && let Stage::On { .. } = v.stage
            {
                let from = Self::env(v.stage, time);
                v.stage = Stage::Off { start: time, from };
            }
        }
    }
    fn release_all(&mut self, time: f64) {
        for v in &mut self.voices {
            if let Stage::On { .. } = v.stage {
                let from = Self::env(v.stage, time);
                v.stage = Stage::Off { start: time, from };
            }
        }
    }
}

/// The UI side of the preview (lives in `App::p9` while mounted).
pub struct SignalPreview {
    pub kind: SignalType,
    pub level: Arc<AtomicU32>,
    /// MIDI rows from the raw listener: (status, data1, data2).
    pub midi: Arc<Mutex<Vec<[u8; 3]>>>,
    listener: u64,
}

thread_local! {
    static CURRENT: std::cell::RefCell<Option<SignalPreview>> = const { std::cell::RefCell::new(None) };
}

fn level_bits(amp: f64) -> u32 {
    // Tone.Volume(gainToDb(amp)) → dbToGain: the gain (0 for 0).
    let db = 20.0 * (amp.ln() / std::f64::consts::LN_10);
    (10f64.powf(db / 20.0) as f32).to_bits()
}

fn install(s: &mut App, kind: SignalType, level: &Arc<AtomicU32>) {
    let synth = PreviewSynth::new(kind, s.svc.engine.sample_rate(), level.clone());
    s.svc.engine.set_preview_instrument(Some(Box::new(synth)));
}

/// Mount: silence the live synth and the drums (the preview mute), put the synth on the preview slot, listen to MIDI.
pub fn mount(s: &mut App) {
    let kind = s.p9.signal.kind;
    let level = Arc::new(AtomicU32::new(level_bits(s.p9.signal.amplitude)));
    install(s, kind, &level);
    // Its own mute, not the user's (the top bar keeps showing the user's state, and leaving
    // the tab never un-mutes a mute the user set).
    s.svc.with_router(|r, out| r.set_preview_muted(true, out));
    let midi: Arc<Mutex<Vec<[u8; 3]>>> = Arc::default();
    let q = midi.clone();
    let listener = s
        .svc
        .midi
        .host
        .add_raw_listener(Box::new(move |b: &[u8], _| {
            if b.len() >= 3 {
                q.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push([b[0], b[1], b[2]]);
            }
        }));
    CURRENT.with(|c| {
        *c.borrow_mut() = Some(SignalPreview {
            kind,
            level,
            midi,
            listener,
        })
    });
    s.p9.signal_preview = None;
}

/// Unmount: release, remove the synth, lift the preview mute (the user's mute is untouched).
pub fn unmount(s: &mut App) {
    let Some(p) = CURRENT.with(|c| c.borrow_mut().take()) else {
        return;
    };
    s.svc.midi.host.remove_raw_listener(p.listener);
    s.svc.engine.set_preview_instrument(None);
    s.svc.with_router(|r, out| r.set_preview_muted(false, out));
    s.p9.signal_preview = None;
}

/// Per frame: follow the type (recreate the synth) and the level; play queued MIDI.
pub fn poll(s: &mut App) -> bool {
    let kind = s.p9.signal.kind;
    let amp = s.p9.signal.amplitude;
    let mut recreate = None;
    let mut rows = Vec::new();
    CURRENT.with(|c| {
        if let Some(p) = c.borrow_mut().as_mut() {
            p.level.store(level_bits(amp), Ordering::Relaxed);
            if p.kind != kind {
                p.kind = kind;
                recreate = Some(p.level.clone());
            }
            rows = std::mem::take(&mut *p.midi.lock().unwrap_or_else(|e| e.into_inner()));
        }
    });
    if let Some(level) = recreate {
        install(s, kind, &level);
        s.p9.signal_preview = None;
    }
    let mut changed = false;
    for [status, note, vel] in rows {
        let cmd = status & 0xF0;
        if cmd == 0x90 && vel > 0 {
            s.svc.engine.preview_synth_note(SynthNoteOp::On {
                note: note as f64,
                velocity: vel as f64 / 127.0,
            });
            if kind != SignalType::WhiteNoise {
                let hz = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
                s.p9.signal.frequency = hz.round();
                s.p9.detected_note = Some(mdaw_model::notes::midi_to_note_name(note as f64));
                changed = true;
            }
        } else if cmd == 0x80 || cmd == 0x90 {
            s.svc
                .engine
                .preview_synth_note(SynthNoteOp::Off { note: note as f64 });
        }
    }
    changed
}

/// "Hold to preview" down: the frequency's nearest note at velocity 0.8.
pub fn start_hold(s: &mut App) {
    let f = s.p9.signal.frequency.max(1.0);
    let note = (69.0 + 12.0 * (f / 440.0).log2()).round();
    s.p9.signal_preview = Some(note as i64);
    s.svc.engine.preview_synth_note(SynthNoteOp::On {
        note,
        velocity: 0.8,
    });
}

/// Pointer up / leave.
pub fn stop_hold(s: &mut App) {
    if let Some(n) = s.p9.signal_preview.take() {
        s.svc
            .engine
            .preview_synth_note(SynthNoteOp::Off { note: n as f64 });
    }
}

/// Queues a raw MIDI row as if the device sent it (the raw listener's path; tests).
pub fn feed(bytes: [u8; 3]) {
    CURRENT.with(|c| {
        if let Some(p) = c.borrow().as_ref() {
            p.midi.lock().unwrap_or_else(|e| e.into_inner()).push(bytes);
        }
    });
}

/// The preview is mounted (tests).
pub fn mounted() -> bool {
    CURRENT.with(|c| c.borrow().is_some())
}
