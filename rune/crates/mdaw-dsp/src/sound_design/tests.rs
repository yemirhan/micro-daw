use super::*;
use mdaw_model::sound_design::{AmpEnv, OscLayer, Template};

const SR: u32 = 48_000;

fn sine_recipe() -> SoundRecipe {
    let mut r = SoundRecipe {
        trim_tail: false,
        ..SoundRecipe::default()
    };
    r.layers[0] = OscLayer {
        on: true,
        wave: Wave::Sine,
        level: 1.0,
        ..OscLayer::default()
    };
    r
}

fn peak(s: &[f32]) -> f32 {
    s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
}

/// Zero crossings (rising) in `s` → frequency.
fn freq(s: &[f32], sr: f64) -> f64 {
    let n = s.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
    n as f64 / (s.len() as f64 / sr)
}

#[test]
fn envelope_timing() {
    let mut r = sine_recipe();
    r.length = 1.0;
    r.amp = AmpEnv {
        attack: 0.1,
        hold: 0.1,
        decay: 0.2,
        sustain: 0.5,
        release: 0.2,
        curve: 0.0,
        retrig: 0,
    };
    let e = Env::new(&r.sanitized());
    assert!((e.at(0.05) - 0.5).abs() < 1e-9, "linear attack midpoint");
    assert!((e.at(0.1) - 1.0).abs() < 1e-9);
    assert!((e.at(0.15) - 1.0).abs() < 1e-9, "hold");
    assert!((e.at(0.3) - 0.75).abs() < 1e-9, "decay midpoint");
    assert!((e.at(0.6) - 0.5).abs() < 1e-9, "sustain");
    assert!(
        (e.at(0.8) - 0.5).abs() < 1e-9,
        "release starts length − release"
    );
    assert!((e.at(0.9) - 0.25).abs() < 1e-9);
    assert!(e.at(1.0).abs() < 1e-9);
    // Rendered: the length is exact and the attack is silent at t=0.
    let s = render_raw(&r, SR, 0.0, 1.0);
    assert_eq!(s.len(), SR as usize);
    let at = |t: f64| peak(&s[(t * SR as f64) as usize..((t + 0.01) * SR as f64) as usize]);
    assert!((at(0.6) - 0.5).abs() < 0.02);
    assert!(at(0.985) < 0.05);
}

#[test]
fn curve_bends_the_decay() {
    let mut r = sine_recipe();
    r.amp.sustain = 0.0;
    r.amp.decay = 1.0;
    r.amp.attack = 0.0;
    r.amp.curve = 1.0;
    let fast = Env::new(&r).at(0.25);
    r.amp.curve = -1.0;
    let slow = Env::new(&r).at(0.25);
    assert!(fast < 0.75 && slow > 0.75, "{fast} {slow}");
}

#[test]
fn retrig_replays_the_attack() {
    let mut r = sine_recipe();
    r.amp = AmpEnv {
        attack: 0.0,
        decay: 0.005,
        retrig: 2,
        ..AmpEnv::default()
    };
    let e = Env::new(&r);
    assert!(e.at(0.0001) > 0.9);
    assert!(e.at(RETRIG_SPACING + 0.0001) > 0.9);
    assert!(e.at(2.0 * RETRIG_SPACING + 0.0001) > 0.9);
    assert!(e.at(RETRIG_SPACING - 0.0005) < 0.2);
}

#[test]
fn pitch_envelope_sweeps_down_to_the_note() {
    let mut r = sine_recipe();
    r.note = 45.0; // 110 Hz
    r.length = 1.0;
    r.amp = AmpEnv {
        attack: 0.0,
        decay: 1.0,
        sustain: 1.0,
        release: 0.001,
        ..AmpEnv::default()
    };
    r.layers[0].pitch_amount = 24.0;
    r.layers[0].pitch_decay = 0.2;
    let s = render_raw(&r, SR, 0.0, 1.0);
    let sr = SR as f64;
    let early = freq(&s[0..(0.03 * sr) as usize], sr);
    let late = freq(&s[(0.5 * sr) as usize..(0.9 * sr) as usize], sr);
    assert!(early > 250.0, "starts near +2 octaves: {early}");
    assert!((late - 110.0).abs() < 4.0, "settles on the note: {late}");
    // Transpose moves everything.
    let up = render_raw(&r, SR, 12.0, 1.0);
    let late_up = freq(&up[(0.5 * sr) as usize..(0.9 * sr) as usize], sr);
    assert!((late_up - 220.0).abs() < 6.0, "{late_up}");
}

#[test]
fn deterministic_and_block_size_independent() {
    for t in Template::ALL {
        let r = t.recipe();
        let a = render_raw(&r, SR, 0.0, 1.0);
        let b = render_raw(&r, SR, 0.0, 1.0);
        assert_eq!(a, b, "{}", t.name());
        // The live path: the same voice, in 128-frame quanta.
        let mut v = Voice::new(SR as f64);
        v.start(&r, 0.0, 1.0);
        let mut live = Vec::new();
        let mut q = [0f32; 128];
        while v.is_active() {
            v.process(&mut q);
            live.extend_from_slice(&q);
        }
        assert_eq!(&live[..a.len()], &a[..], "{}", t.name());
        assert!(live[a.len()..].iter().all(|x| *x == 0.0));
    }
}

#[test]
fn no_nan_or_denormal_output() {
    let mut extreme = sine_recipe();
    extreme.filter.resonance = 1.0;
    extreme.filter.cutoff = 20.0;
    extreme.filter.env_amount = 8.0;
    extreme.drive = 1.0;
    extreme.crush = 2.0;
    extreme.fm.amount = 1.0;
    extreme.layers[1].on = true;
    extreme.layers[1].wave = Wave::Pink;
    extreme.layers[2].on = true;
    extreme.layers[2].wave = Wave::Pulse;
    extreme.layers[2].coarse = 48.0;
    extreme.note = 108.0;
    extreme.length = 4.0;
    let mut nan = sine_recipe();
    nan.gain_db = f64::NAN;
    nan.filter.cutoff = f64::INFINITY;
    nan.amp.decay = f64::NAN;
    let mut recipes: Vec<SoundRecipe> = Template::ALL.iter().map(|t| t.recipe()).collect();
    recipes.push(extreme);
    recipes.push(nan);
    for r in recipes {
        let s = render_raw(&r, SR, 0.0, 1.0);
        for x in &s {
            assert!(x.is_finite());
            assert!(*x == 0.0 || x.abs() >= f32::MIN_POSITIVE, "denormal {x}");
        }
    }
}

#[test]
fn templates_are_audible_and_bounded() {
    for t in Template::ALL {
        let b = render(&t.recipe(), SR);
        let p = peak(&b.channels[0]);
        assert!(p > 0.1 && p < 4.0, "{}: peak {p}", t.name());
    }
}

#[test]
fn tail_trim_cuts_silence_with_a_fade() {
    let mut r = sine_recipe();
    r.length = 2.0;
    r.amp = AmpEnv {
        attack: 0.0,
        decay: 0.1,
        sustain: 0.0,
        curve: 0.0,
        ..AmpEnv::default()
    };
    r.trim_tail = true;
    let b = render(&r, SR);
    let n = b.frames();
    assert!(n < (0.12 * SR as f64) as usize, "trimmed to ~decay: {n}");
    assert!(n >= (0.09 * SR as f64) as usize, "{n}");
    assert_eq!(b.channels[0][n - 1], 0.0, "faded to zero");
    r.trim_tail = false;
    assert_eq!(render(&r, SR).frames(), 2 * SR as usize);
    // A loud sound up to its end is left alone.
    let mut loud = vec![0.5f32; 1000];
    trim_tail(&mut loud, SR);
    assert_eq!(loud.len(), 1000);
    // Silence keeps the minimum length.
    let mut quiet = vec![0.0f32; SR as usize];
    trim_tail(&mut quiet, SR);
    assert_eq!(quiet.len(), (MIN_TRIMMED * SR as f64) as usize);
}

#[test]
fn fm_changes_the_carrier() {
    let mut r = sine_recipe();
    r.layers[1] = OscLayer {
        on: true,
        level: 0.0,
        coarse: 7.0,
        ..OscLayer::default()
    };
    let plain = render_raw(&r, SR, 0.0, 1.0);
    r.fm.amount = 0.5;
    let fm = render_raw(&r, SR, 0.0, 1.0);
    assert_ne!(plain, fm);
    assert!(fm.iter().all(|x| x.is_finite()));
}

#[test]
fn sample_edits_trim_reverse_normalize_and_fade() {
    let src = AudioBuffer {
        sample_rate: 100,
        channels: vec![(0..100).map(|i| i as f32 / 200.0).collect()],
    };
    let mut e = SampleEdit::new("x");
    assert_eq!(apply_sample_edits(&src, &e).channels, src.channels);
    e.start = 0.1;
    e.end = Some(0.5);
    let b = apply_sample_edits(&src, &e);
    assert_eq!(b.frames(), 40);
    assert_eq!(b.channels[0][0], 10.0 / 200.0);
    e.reverse = true;
    e.normalize = true;
    let b = apply_sample_edits(&src, &e);
    assert_eq!(
        b.channels[0][0], 1.0,
        "peak first after reverse, normalized"
    );
    e.fade_out = 0.1;
    e.fade_in = 0.1;
    let b = apply_sample_edits(&src, &e);
    assert_eq!(b.channels[0][0], 0.0);
    assert_eq!(b.channels[0][39], 0.0);
    assert!(b.channels[0][20] > 0.5);
    e.end = Some(0.1);
    assert_eq!(apply_sample_edits(&src, &e).frames(), 1);
}

/// Review finding 9: a release as long as the sound (or longer), with any attack, still plays
/// the attack and fades to 0 at the end; never all silent while the level is up.
#[test]
fn overlapping_attack_and_release_are_never_silent() {
    let mut clap = Template::Clap.recipe();
    clap.amp.release = clap.length;
    let s = render_raw(&clap, SR, 0.0, 1.0);
    assert!(peak(&s) > 0.05, "Clap with release = length: {}", peak(&s));
    for &length in &[0.02, 0.1, 0.45, 1.0, 3.0] {
        for &attack in &[0.0, 0.0005, 0.01, 0.2, 1.0, 5.0] {
            for &release in &[0.0, 0.01, 0.1, 0.45, 1.0, 3.0, 10.0] {
                for &retrig in &[0u8, 3] {
                    let mut r = sine_recipe();
                    r.length = length;
                    r.amp = AmpEnv {
                        attack,
                        hold: 0.0,
                        decay: 0.3,
                        sustain: 0.5,
                        release,
                        curve: 0.3,
                        retrig,
                    };
                    let r = r.sanitized();
                    let s = render_raw(&r, SR, 0.0, 1.0);
                    let p = peak(&s);
                    assert!(
                        p > 0.01,
                        "silent: length {length} attack {attack} release {release} retrig {retrig}"
                    );
                    let e = Env::new(&r);
                    assert!(
                        e.at(r.length).abs() < 1e-9,
                        "the release ends at 0: length {length} attack {attack} release {release}"
                    );
                    let curve = envelope_curve(&r, 64);
                    assert!(curve.iter().all(|x| x.is_finite() && (0.0..=1.0).contains(x)));
                }
            }
        }
    }
}

/// Review finding 12: every template leaves headroom (peak ≤ −0.5 dBFS, aimed at −1..−3).
#[test]
fn templates_leave_headroom() {
    let peaks: Vec<(&str, f64)> = Template::ALL
        .iter()
        .map(|t| {
            let b = render(&t.recipe(), SR);
            (t.name(), 20.0 * (peak(&b.channels[0]) as f64).log10())
        })
        .collect();
    for (name, db) in &peaks {
        assert!(*db <= -0.5, "{name}: peak {db:.2} dBFS ({peaks:?})");
        assert!(*db >= -6.0, "{name}: too quiet, {db:.2} dBFS ({peaks:?})");
    }
}
