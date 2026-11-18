use super::*;

#[test]
fn default_chain_is_a_gain() {
    let mut c = CharacterChain::new(48000, CharacterParams::default(), -6.0, 0.0);
    assert_eq!(c.stages(), [false; 5]);
    let mut l = [0.5f32; 128];
    let mut r = [-0.25f32; 128];
    c.process(0, &mut l, &mut r);
    let g = db_to_gain(-6.0) as f32;
    assert!(l.iter().all(|&x| x == 0.5 * g));
    assert!(r.iter().all(|&x| x == -0.25 * g));
}

#[test]
fn stages_follow_the_thresholds() {
    let mut p = CharacterParams {
        drive: 0.002,
        presence: 0.06,
        tone: 18999.0,
        tremolo: 0.002,
        chorus: 0.002,
        ..CharacterParams::default()
    };
    let mut c = CharacterChain::new(48000, p, 0.0, 0.0);
    assert_eq!(c.stages(), [true; 5]);
    p = CharacterParams {
        drive: 0.001,
        presence: -0.05,
        tone: 19000.0,
        tremolo: 0.001,
        chorus: 0.001,
        ..p
    };
    c.update(p, 0.0, 0.1);
    assert_eq!(c.stages(), [false; 5]);
}

#[test]
fn drive_curve_is_level_matched_at_the_reference() {
    for d in [0.0, 0.35, 1.0] {
        let f = drive_curve(d);
        assert!((f(0.35) - 0.35).abs() < 1e-12);
        assert_eq!(f(0.0), 0.0);
        assert!(f(10.0) <= 1.0 && f(-10.0) >= -1.0);
    }
    let mut buf = [0f32; DRIVE_CURVE_LEN];
    fill_drive_curve(0.5, &mut buf);
    let tone = mdaw_dsp::shaping::tone_curve(DRIVE_CURVE_LEN, drive_curve(0.5));
    assert_eq!(&buf[..], &tone[..]);
}

#[test]
fn volume_update_ramps_over_50_ms() {
    let mut c = CharacterChain::new(48000, CharacterParams::default(), 0.0, 0.0);
    c.update(CharacterParams::default(), -20.0, 0.0);
    let mut l = vec![1f32; 4800];
    let mut r = vec![1f32; 4800];
    for (i, (a, b)) in l.chunks_mut(128).zip(r.chunks_mut(128)).enumerate() {
        c.process(i as u64 * 128, a, b);
    }
    assert_eq!(l[0], 1.0);
    assert!(l[1200] < 1.0 && l[1200] > 0.1);
    assert!((l[2400] - 0.1).abs() < 1e-6);
    assert!((l[4799] - 0.1).abs() < 1e-6);
}

#[test]
fn process_and_update_do_not_allocate() {
    let p = CharacterParams {
        drive: 0.3,
        presence: 4.0,
        tone: 3000.0,
        tremolo: 0.5,
        chorus: 0.5,
        ..CharacterParams::default()
    };
    let mut c = CharacterChain::new(48000, p, 0.0, 0.0);
    let mut l = [0.1f32; 128];
    let mut r = [0.1f32; 128];
    let n = crate::test_alloc::allocations_during(|| {
        for q in 0..20u64 {
            if q == 10 {
                c.update(
                    CharacterParams {
                        drive: 0.6,
                        chorus: 0.0,
                        ..p
                    },
                    -3.0,
                    10.0 * 128.0 / 48000.0,
                );
            }
            c.process(q * 128, &mut l, &mut r);
        }
    });
    assert_eq!(n, 0);
}
