//! Phase-8 review #2: frame cost of a big song (48 tracks) and a big piano-roll selection
//! (32k notes). Bench-style: prints the timings (`--nocapture`) and asserts generous bounds that
//! the culled / set-based view meets in a debug build. Release figures for the review record:
//! `cargo test --release -p mdaw-app t_perf -- --nocapture`.

use std::time::Instant;

use super::harness::*;

type Rt = rune::Runtime<crate::app::App>;

/// The template plus tracks up to 48, each with 8 two-bar regions of 16 notes.
fn big_song() -> Rt {
    let mut rt = song_rt();
    while rt.state.tracks().len() < 48 {
        rt.state.add_song_track(mdaw_model::InstrumentType::Synth);
    }
    let ids: Vec<String> = rt.state.tracks().iter().map(|t| t.id.clone()).collect();
    rt.state.svc.engine.edit(|d| {
        for id in &ids {
            for k in 0..8 {
                let notes = (0..16)
                    .map(|n| mdaw_model::RegionNote {
                        note: 48.0 + (n % 12) as f64,
                        start_beat: n as f64 * 0.5,
                        duration_beats: 0.5,
                        velocity: 0.8,
                        is_drum: false,
                    })
                    .collect();
                d.add_region(
                    id,
                    mdaw_model::Region {
                        id: String::new(),
                        start_beat: k as f64 * 8.0,
                        length_beats: 8.0,
                        notes,
                        color: None,
                        name: Some(format!("R{k}")),
                        audio: None,
                        extra: Default::default(),
                    },
                );
            }
        }
    });
    rt.invalidate();
    run(&mut rt, 0.5);
    rt
}

/// Mean milliseconds of `n` runs of `f` (each ends with a frame).
fn time(rt: &mut Rt, n: usize, mut f: impl FnMut(&mut Rt, usize)) -> f64 {
    let t = Instant::now();
    for i in 0..n {
        f(rt, i);
    }
    t.elapsed().as_secs_f64() * 1000.0 / n as f64
}

/// One UI frame without audio time: headless runs render the engine offline inside a frame,
/// and 48 tracks of FX would dominate what is measured here (the window renders audio on its
/// own thread).
fn ui_frame(rt: &mut Rt) {
    rt.frame(0.0);
}

#[test]
fn t_perf_song_with_48_tracks_and_a_32k_note_selection() {
    let mut rt = big_song();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 1);
    // A region drag: one frame per pointer move (each move edits the doc → a full rebuild).
    let (x, y) = at(&rt, &format!("region-{r}"));
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    ui_frame(&mut rt);
    let drag = time(&mut rt, 20, |rt, i| {
        rt.pointer_move(x + 30.0 * ((i % 4) as f32 + 1.0), y);
        ui_frame(rt);
    });
    rt.pointer_up(x, y);
    frames(&mut rt, 2);
    // Wheel scrolling the lanes.
    let (lx, ly) = at(&rt, "song-lanes");
    let wheel = time(&mut rt, 20, |rt, i| {
        let dy = if i % 2 == 0 { -120.0 } else { 120.0 };
        rt.pointer_move(lx, ly);
        rt.wheel_event(rune::WheelEvent::new(0.0, dy).at(lx, ly));
        ui_frame(rt);
    });
    // ⌘A: every region selected, every one on screen checked for selection.
    let mut select_all = 0.0;
    for _ in 0..5 {
        rt.state.clear_selection();
        rt.invalidate();
        ui_frame(&mut rt);
        select_all += time(&mut rt, 1, |rt, _| {
            rt.key(ev(rune::Key::Command('a'), cmd()));
            ui_frame(rt);
        }) / 5.0;
    }
    assert_eq!(
        rt.state.song.selection.len(),
        rt.state
            .tracks()
            .iter()
            .map(|t| t.regions.len())
            .sum::<usize>()
    );

    // The piano roll: a 32k-note line (a sixteenth each, 8192 beats), at the default zoom: a few
    // dozen on screen. The cost measured is the selection's, all selected vs none.
    let notes: Vec<mdaw_model::RegionNote> = (0..32_768)
        .map(|n| mdaw_model::RegionNote {
            note: 48.0 + (n % 12) as f64,
            start_beat: n as f64 * 0.25,
            duration_beats: 0.25,
            velocity: 0.8,
            is_drum: false,
        })
        .collect();
    let len = 32_768.0 * 0.25;
    rt.state
        .svc
        .engine
        .edit(|d| d.edit_region_notes(&b, &r, notes, Some("Big"), Some(len)));
    rt.state.open_piano_roll(&b, &r);
    rt.invalidate();
    run(&mut rt, 0.5);
    rt.state.song.piano.vp.ppb = mdaw_model::piano_roll::PR_DEFAULT_PX_PER_BEAT;
    rt.state.song.piano.vp.left = 0.0;
    let piano_none = time(&mut rt, 5, |rt, _| {
        rt.invalidate();
        ui_frame(rt);
    });
    rt.state.song.piano.selection = (0..32_768).collect();
    let piano = time(&mut rt, 5, |rt, _| {
        rt.invalidate();
        ui_frame(rt);
    });
    eprintln!(
        "[perf] 48 tracks: drag frame {drag:.2} ms, wheel frame {wheel:.2} ms, ⌘A {select_all:.2} ms; \
         piano roll 32k notes: none selected {piano_none:.2} ms, all selected {piano:.2} ms"
    );
    // Generous bounds (a loaded machine: the whole workspace's tests run in parallel; debug
    // ×12). Shared CI runners (`CI` set) get another ×4. Release before the fix, this test on
    // this Mac: see docs/rune-port/review-phase-8.md.
    let ci = if std::env::var_os("CI").is_some() {
        4.0
    } else {
        1.0
    };
    let bound = |release: f64| {
        ci * if cfg!(debug_assertions) {
            release * 12.0
        } else {
            release
        }
    };
    assert!(drag < bound(20.0), "drag {drag:.1} ms");
    assert!(wheel < bound(20.0), "wheel {wheel:.1} ms");
    assert!(select_all < bound(20.0), "⌘A {select_all:.1} ms");
    assert!(piano < bound(20.0), "piano {piano:.1} ms");
    assert!(
        piano < piano_none * 2.0 + 2.0 * ci,
        "selection cost: {piano_none:.1} → {piano:.1} ms"
    );
}

#[test]
#[ignore]
fn t_perf_profile_loop() {
    let mut rt = big_song();
    let which = std::env::var("PERF_WHICH").unwrap_or_default();
    if which == "piano" {
        let b = tid(&rt, "Bass");
        let r = rid(&rt, &b, 1);
        let notes: Vec<mdaw_model::RegionNote> = (0..32_768)
            .map(|n| mdaw_model::RegionNote {
                note: 48.0 + (n % 12) as f64,
                start_beat: n as f64 * 0.25,
                duration_beats: 0.25,
                velocity: 0.8,
                is_drum: false,
            })
            .collect();
        rt.state
            .svc
            .engine
            .edit(|d| d.edit_region_notes(&b, &r, notes, Some("Big"), Some(8192.0)));
        rt.state.open_piano_roll(&b, &r);
        rt.invalidate();
        run(&mut rt, 0.5);
        rt.state.song.piano.vp.ppb = mdaw_model::piano_roll::PR_DEFAULT_PX_PER_BEAT;
        rt.state.song.piano.vp.left = 0.0;
        rt.state.song.piano.selection = (0..32_768).collect();
    }
    let t = Instant::now();
    while t.elapsed().as_secs_f64() < 12.0 {
        if which == "drag" {
            rt.state.svc.engine.edit(|d| d.set_bpm(121.0));
        }
        rt.invalidate();
        rt.frame(0.0);
    }
}
