use super::*;

fn rnd() -> impl FnMut() -> f64 {
    let mut r = MathRandom::new();
    move || r.next_f64()
}

#[test]
fn tail_is_reverb_plus_delay_echoes_capped_at_three_seconds() {
    let mut p = DEFAULT_PROCESSING;
    assert_eq!(p.tail_seconds(), 0.0);
    p.reverb.enabled = true;
    assert_eq!(p.tail_seconds(), 2.5);
    p.delay.enabled = true;
    assert!((p.tail_seconds() - (2.5 + 0.25 / 0.7)).abs() < 1e-12);
    p.delay.feedback = 0.9;
    assert_eq!(p.tail_seconds(), 3.0);
    p.delay.feedback = 0.3;
    p.reverb.enabled = false;
    assert!((p.tail_seconds() - 0.25 / 0.7).abs() < 1e-12);
    assert!(!DEFAULT_PROCESSING.any_enabled());
}

#[test]
fn reverse_and_normalize_keep_length_and_rate() {
    let src = AudioBuffer {
        sample_rate: 22050,
        channels: vec![vec![0.1, -0.5, 0.25], vec![0.0, 0.2, 0.0]],
    };
    let p = ProcessParams {
        reverse: true,
        normalize: true,
        ..DEFAULT_PROCESSING
    };
    let out = process_audio(&src, &p, &mut rnd(), &mut NoiseBank::new());
    assert_eq!(out.sample_rate, 22050);
    assert_eq!(out.channels[0], vec![0.5, -1.0, 0.2]);
    assert_eq!(out.channels[1], vec![0.0, 0.4, 0.0]);
    // Peak exactly 1 or 0: untouched.
    let mut b = AudioBuffer::mono(48000, vec![1.0, -0.5]);
    normalize_buffer(&mut b);
    assert_eq!(b.channels[0], vec![1.0, -0.5]);
}

#[test]
fn rendering_adds_the_tail() {
    let src = AudioBuffer::mono(48000, vec![0.5; 4800]);
    let p = ProcessParams {
        delay: DelayStage {
            enabled: true,
            ..DEFAULT_PROCESSING.delay
        },
        ..DEFAULT_PROCESSING
    };
    let out = process_audio(&src, &p, &mut rnd(), &mut NoiseBank::new());
    assert_eq!(out.channel_count(), 1);
    assert_eq!(out.frames(), ((0.1 + 0.25 / 0.7) * 48000.0) as usize);
}

#[test]
fn reverb_envelope_rises_at_the_predelay_and_ends_at_zero() {
    let sr = 48000.0;
    let len = ((2.5 + 0.01) * sr) as usize;
    let e = reverb_envelope(2.5, sr, len);
    assert!(e[..480].iter().all(|&v| v == 0.0));
    assert_eq!(e[480], 1.0);
    assert!(e[481] < 1.0 && e[481] > 0.99);
    assert!(e.windows(2).skip(481).all(|w| w[1] <= w[0]));
    assert!(e[len - 1] < 1e-4);
}

#[test]
fn generator_names_and_lengths() {
    let p = DEFAULT_SIGNAL;
    assert_eq!(signal_name(&p), "sine 440Hz 1s");
    let n = SignalParams {
        kind: SignalType::WhiteNoise,
        duration: 0.25,
        ..p
    };
    assert_eq!(signal_name(&n), "white-noise 0.25s");
    let b = generate_signal(&n, 44100, &mut rnd());
    assert_eq!(b.frames(), (44100.0f64 * 0.25).ceil() as usize);
    assert!(b.channels[0].iter().all(|v| v.abs() <= 0.800_001));
    let t = generate_signal(&p, 48000, &mut rnd());
    assert_eq!(t.frames(), 48000);
    let peak = t.channels[0].iter().fold(0f32, |m, v| m.max(v.abs()));
    assert!((peak - 0.8).abs() < 1e-3, "{peak}");
}
