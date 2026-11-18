//! Ported `instruments.test.ts` cases (SampledPiano, piano samples) plus the Rust-side rules
//! (fallback routing, nearest sample, allocation-free audio path).

use std::sync::{Arc, Mutex};

use mdaw_dsp::buffer::AudioBuffer;

use super::sampler::{find_closest, pick_sample};
use super::samples::{PIANO_SAMPLE_COUNT, PianoSamples, sample_file_to_midi, trim_leading_silence};
use super::*;

#[derive(Debug, Clone, PartialEq)]
enum Ev {
    On(i32, f64),
    Off(i32, f64),
    Ar(i32, f64, f64),
    All(f64),
}

/// A fallback stub that logs its calls and keeps PolyInstrument-like held / active state
/// (release 0.4 s, like `FALLBACK_PIANO.amp.release`).
#[derive(Clone, Default)]
struct StubFallback {
    log: Arc<Mutex<Vec<Ev>>>,
    held: Arc<Mutex<Vec<(i32, f64)>>>,
}

const STUB_RELEASE: f64 = 0.4;

impl Instrument for StubFallback {
    fn process(&mut self, _: u64, l: &mut [f32], _: &mut [f32]) {
        // A constant per held note, so tests can see the fallback in the bus.
        let n = self
            .held
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.1.is_infinite())
            .count();
        for x in l {
            *x += 0.01 * n as f32;
        }
    }
}

impl PianoFallback for StubFallback {
    fn note_on(&mut self, midi: i32, v: f64, t: f64) {
        self.log.lock().unwrap().push(Ev::On(midi, t));
        let _ = v;
        self.held.lock().unwrap().push((midi, f64::INFINITY));
    }
    fn note_off(&mut self, midi: i32, t: f64) {
        self.log.lock().unwrap().push(Ev::Off(midi, t));
        for h in self.held.lock().unwrap().iter_mut() {
            if h.0 == midi && h.1.is_infinite() {
                h.1 = t + STUB_RELEASE;
            }
        }
    }
    fn attack_release(&mut self, midi: i32, d: f64, t: f64, _: f64) {
        self.log.lock().unwrap().push(Ev::Ar(midi, d, t));
    }
    fn release_all(&mut self, t: f64) {
        self.log.lock().unwrap().push(Ev::All(t));
        self.held.lock().unwrap().clear();
    }
    fn active_voice_count(&self, t: f64) -> usize {
        self.held.lock().unwrap().iter().filter(|h| h.1 > t).count()
    }
    fn is_held(&self, midi: i32) -> bool {
        self.held
            .lock()
            .unwrap()
            .iter()
            .any(|h| h.0 == midi && h.1.is_infinite())
    }
}

fn plain() -> PianoParams {
    PianoParams {
        gain: 0.0,
        character: CharacterParams::default(),
        ..PianoParams::GRAND
    }
}

/// A tiny sample set: a 0.5 s 440 Hz-ish ramp on root 60 (and optionally more roots).
fn tiny(roots: &[u8]) -> Arc<PianoSamples> {
    let data: Vec<f32> = (0..24000)
        .map(|i| ((i % 109) as f32 / 109.0) - 0.5)
        .collect();
    let b = Arc::new(AudioBuffer {
        sample_rate: 48000,
        channels: vec![data.clone(), data],
    });
    Arc::new(PianoSamples::from_buffers(
        48000,
        roots.iter().map(|&r| (r, b.clone())),
    ))
}

fn render(p: &mut SampledPiano, from: u64, quanta: u64) -> (Vec<f32>, Vec<f32>) {
    let mut l = vec![0f32; (quanta * 128) as usize];
    let mut r = l.clone();
    for q in 0..quanta {
        let s = (q * 128) as usize;
        p.process(from + q * 128, &mut l[s..s + 128], &mut r[s..s + 128]);
    }
    (l, r)
}

#[test]
fn plays_the_fallback_until_samples_load_and_routes_note_offs_back() {
    let fb = StubFallback::default();
    let slot = PianoSlot::empty();
    let mut p = SampledPiano::new(
        48000,
        PianoParams::GRAND,
        Box::new(fb.clone()),
        slot.clone(),
        0.0,
    );
    assert!(!p.is_sampled());
    p.note_on(60, 0.8, 0.0);
    p.note_on(64, 0.8, 0.0);
    assert_eq!(p.held_notes(), vec![60, 64]);
    // The samples arrive while 60 and 64 are held on the fallback.
    slot.fill(tiny(&[60]));
    assert!(p.is_sampled());
    p.note_on(67, 0.8, 0.1); // sampled now
    p.note_off(60, 0.5);
    p.note_off(64, 0.5);
    p.note_off(67, 0.5);
    p.note_off(60, 0.6); // not on the fallback any more: not routed again
    assert_eq!(
        *fb.log.lock().unwrap(),
        vec![
            Ev::On(60, 0.0),
            Ev::On(64, 0.0),
            Ev::Off(60, 0.5),
            Ev::Off(64, 0.5)
        ]
    );
    assert_eq!(p.held_notes(), Vec::<i32>::new());
    assert_eq!(p.active_voice_count(0.5 + STUB_RELEASE + 0.05 + 0.5), 0);
}

#[test]
fn fallback_gets_attack_release_and_release_all_before_load() {
    let fb = StubFallback::default();
    let mut p = SampledPiano::new(
        48000,
        PianoParams::GRAND,
        Box::new(fb.clone()),
        PianoSlot::empty(),
        0.0,
    );
    p.trigger_attack_release(60, 0.001, 0.2, 100.0);
    p.note_on(62, 0.5, 0.3);
    p.release_all(0.4);
    p.note_off(62, 0.5); // cleared by releaseAll: not routed
    assert_eq!(
        *fb.log.lock().unwrap(),
        vec![Ev::Ar(60, 0.024, 0.2), Ev::On(62, 0.3), Ev::All(0.4)]
    );
    // The fallback renders into the bus (×2 makeup, through the chain).
    let mut p2 = SampledPiano::new(
        48000,
        plain(),
        Box::new(StubFallback::default()),
        PianoSlot::empty(),
        0.0,
    );
    p2.note_on(60, 1.0, 0.0);
    let (l, _) = render(&mut p2, 0, 1);
    assert!((l[0] - 0.02).abs() < 1e-7);
}

#[test]
fn with_samples_restrike_releases_the_previous_source() {
    let mut p = SampledPiano::new(
        48000,
        PianoParams {
            polyphony: 8.0,
            ..plain()
        },
        Box::new(StubFallback::default()),
        PianoSlot::ready(tiny(&[60])),
        0.0,
    );
    assert!(p.is_sampled());
    p.note_on(60, 0.8, 0.0);
    p.note_on(60, 0.8, 0.1);
    assert_eq!(p.held_notes(), vec![60]);
    assert_eq!(p.sampler.source_count(), 2);
    p.note_off(60, 0.2);
    assert_eq!(p.held_notes(), Vec::<i32>::new());
    p.note_on(62, 0.8, 0.3);
    p.release_all(0.4);
    assert_eq!(p.held_notes(), Vec::<i32>::new());
    assert_eq!(p.active_voice_count(5.0), 0);
}

#[test]
fn polyphony_is_at_least_8_and_a_steal_releases_the_victim() {
    let mut p = SampledPiano::new(
        48000,
        PianoParams {
            polyphony: 2.0,
            ..plain()
        },
        Box::new(StubFallback::default()),
        PianoSlot::ready(tiny(&[60])),
        0.0,
    );
    for (i, n) in (40..49).enumerate() {
        p.note_on(n, 0.5, i as f64 * 0.01);
    }
    // 9 notes, 8 voices: the oldest non-lowest (41) was stolen.
    assert_eq!(p.held_notes(), vec![40, 42, 43, 44, 45, 46, 47, 48]);
    assert_eq!(p.active_voice_count(0.1), 8);
}

#[test]
fn velocity_curve_and_release_are_clamped() {
    let mut p = SampledPiano::new(
        48000,
        PianoParams {
            velocity_curve: 9.0,
            release: 99.0,
            ..plain()
        },
        Box::new(StubFallback::default()),
        PianoSlot::empty(),
        0.0,
    );
    assert_eq!(p.velocity_to_gain(0.5), 0.06 + 0.94 * 0.5f64.powf(3.0));
    assert_eq!(p.release_seconds(), 5.0);
    p.set_params(
        PianoParams {
            velocity_curve: 0.1,
            release: 0.0,
            ..plain()
        },
        0.0,
    );
    assert_eq!(p.velocity_to_gain(0.25), 0.06 + 0.94 * 0.25f64.powf(0.5));
    assert_eq!(p.release_seconds(), 0.05);
    assert_eq!(p.velocity_to_gain(0.0), 0.06);
    assert_eq!(norm_velocity(f64::NAN), 0.8);
    assert_eq!(norm_velocity(127.0), 1.0);
    assert_eq!(norm_velocity(63.5), 0.5);
    assert_eq!(norm_velocity(-1.0), 0.0);
}

#[test]
fn nearest_sample_prefers_the_upper_root_and_repitches() {
    let s = tiny(&[57, 60, 63]);
    assert_eq!(find_closest(&s, 60), Some(0));
    assert_eq!(find_closest(&s, 61), Some(1));
    assert_eq!(find_closest(&s, 62), Some(-1));
    assert_eq!(find_closest(&tiny(&[120]), 24), None); // 96 semitones away: out of range
    assert_eq!(find_closest(&tiny(&[120]), 25), Some(-95));
    // Equidistant (two roots 2 apart): the upper one wins.
    let s2 = tiny(&[58, 60]);
    assert_eq!(pick_sample(&s2, 59).unwrap().0, 60);
    let (root, rate) = pick_sample(&s, 62).unwrap();
    assert_eq!(root, 63);
    assert!((rate as f64 - 2f64.powf(-1.0 / 12.0)).abs() < 1e-7);
    let (root, rate) = pick_sample(&s, 127).unwrap();
    assert_eq!(root, 63);
    assert_eq!(rate, 2f64.powf(64.0 / 12.0) as f32);
}

#[test]
fn bundles_one_sample_per_minor_third_a0_to_c8() {
    assert_eq!(PIANO_SAMPLE_COUNT, 30);
    assert_eq!(sample_file_to_midi("./samples/piano/A0.mp3"), 21);
    assert_eq!(sample_file_to_midi("./samples/piano/C4.mp3"), 60);
    assert_eq!(sample_file_to_midi("./samples/piano/Ds4.mp3"), 63);
    assert_eq!(sample_file_to_midi("./samples/piano/Fs4.mp3"), 66);
    assert_eq!(sample_file_to_midi("./samples/piano/C8.mp3"), 108);
    assert_eq!(sample_file_to_midi("./samples/piano/E4.mp3"), -1);
    assert_eq!(sample_file_to_midi("./samples/piano/readme.txt"), -1);
    let mut midis: Vec<i32> = samples::SAMPLE_FILES
        .iter()
        .map(|(n, _)| sample_file_to_midi(&format!("{n}.mp3")))
        .collect();
    midis.sort_unstable();
    assert_eq!(midis, (0..30).map(|i| 21 + 3 * i).collect::<Vec<_>>());
}

#[test]
fn leading_silence_is_trimmed_keeping_1_ms() {
    let mut data = vec![0.001f32; 1000];
    data.extend([0.5f32; 100]);
    let b = trim_leading_silence(AudioBuffer {
        sample_rate: 48000,
        channels: vec![data.clone(), vec![0.0; 1100]],
    });
    assert_eq!(b.frames(), 1100 - (1000 - 48));
    assert_eq!(b.channels[0][48], 0.5);
    // Fewer than 16 frames to drop: unchanged.
    let short = AudioBuffer::mono(48000, [vec![0.0; 60], vec![1.0; 10]].concat());
    assert_eq!(trim_leading_silence(short.clone()), short);
    // Silent: the onset is the end, so only the last 1 ms is kept (as in the TS).
    let silent = AudioBuffer::mono(48000, vec![0.0; 500]);
    assert_eq!(trim_leading_silence(silent).frames(), 48);
}

#[test]
fn release_fades_the_source_out() {
    let mut p = SampledPiano::new(
        48000,
        PianoParams {
            release: 0.1,
            ..plain()
        },
        Box::new(StubFallback::default()),
        PianoSlot::ready(tiny(&[60])),
        0.0,
    );
    p.note_on(60, 1.0, 0.0);
    p.note_off(60, 0.1);
    let (l, _) = render(&mut p, 0, 75); // 0.2 s
    let peak = |a: usize, b: usize| l[a..b].iter().fold(0f32, |m, x| m.max(x.abs()));
    assert!(peak(0, 4800) > 0.9);
    assert!(peak(4800 + 4000, 4800 + 4300) < 0.02);
    assert_eq!(peak(4800 + 4800, 9600), 0.0);
}

#[test]
fn audio_path_does_not_allocate() {
    // (the logging stub allocates; the parity tests' silent fallback does not)
    let fallback = Box::new(super::parity_tests::NoFallback);
    let mut p = SampledPiano::new(
        48000,
        PianoParams::GRAND,
        fallback,
        PianoSlot::ready(tiny(&[60, 72])),
        0.0,
    );
    let mut l = [0f32; 128];
    let mut r = [0f32; 128];
    let n = crate::test_alloc::allocations_during(|| {
        for q in 0..50u64 {
            let t = q as f64 * 128.0 / 48000.0;
            if q % 5 == 0 {
                SynthInstrument::note_on(&mut p, 60.0 + (q % 12) as f64, 0.7, t);
            }
            if q % 7 == 0 {
                SynthInstrument::note_off(&mut p, 60.0 + (q % 12) as f64, t);
            }
            if q == 30 {
                p.trigger_attack_release(70, 0.1, t, 0.5);
                p.set_params(
                    PianoParams {
                        release: 1.0,
                        ..PianoParams::GRAND
                    },
                    t,
                );
            }
            if q == 40 {
                SynthInstrument::release_all(&mut p, t);
            }
            p.process(q * 128, &mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
}
