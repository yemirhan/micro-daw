//! `inventory/views.md` §5 — ArrangementView, ArrangementToolbar, RegionBlock, rubber band.

use rune::Key;

use super::harness::*;
use crate::song::{ArrTool, DEFAULT_PPB, HEADER_W, RULER_H};

#[test]
fn constants_and_defaults() {
    let rt = song_rt();
    assert_eq!(HEADER_W, 224.0);
    assert_eq!(RULER_H, 28.0);
    let s = &rt.state.song;
    assert_eq!((s.ppb, s.snap, s.tool), (30.0, 1.0, ArrTool::Pointer));
    assert!(s.selection.is_empty());
    assert_eq!(rect(&rt, "song-headers")[2], 224.0);
    assert_eq!(rect(&rt, "song-ruler")[3], 28.0);
    // "Tracks" + the count.
    assert!(texts(&rt).contains(&"Tracks".to_owned()));
    assert_eq!(texts_of(&rt, "song-track-count"), "4");
}

pub fn texts_of(rt: &rune::Runtime<crate::app::App>, key: &str) -> String {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .and_then(|e| e.text)
        .unwrap_or_default()
}

#[test]
fn toolbar_adds_tracks_markers_groups_and_picks_tools() {
    let mut rt = song_rt();
    click(&mut rt, "add-synth");
    click(&mut rt, "add-drums");
    let tracks = rt.state.tracks();
    assert_eq!(tracks.len(), 6);
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Track"));
    assert_eq!(tracks[4].instrument.kind, mdaw_model::InstrumentType::Synth);
    assert_eq!(tracks[5].instrument.kind, mdaw_model::InstrumentType::Drums);
    // Marker at the playhead, named by the count (4 markers → "Drop"). Stopped, the
    // playhead reads 0 (`getPosition()` is 0 while stopped, as in the TS).
    click(&mut rt, "add-marker");
    let m = rt.state.svc.engine.doc().get_markers().to_vec();
    assert_eq!(m.len(), 5);
    assert!(m.iter().any(|m| m.name == "Drop" && m.beat == 0.0), "{m:?}");
    rt.state.song_play();
    rt.state.svc.engine.set_position(6.0);
    click(&mut rt, "add-marker");
    let m = rt.state.svc.engine.doc().get_markers().to_vec();
    assert!(m.iter().any(|m| m.name == "Breakdown" && (m.beat - 6.0).abs() < 0.2), "{m:?}");
    rt.state.song_stop();
    // Group: no selection → every track.
    click(&mut rt, "add-group");
    let g = rt.state.svc.engine.doc().get_groups().to_vec();
    assert_eq!(g.len(), 1);
    assert_eq!((g[0].name.as_str(), g[0].track_ids.len()), ("Group", 6));
    // Tools.
    click(&mut rt, "tool-cut");
    assert_eq!(rt.state.song.tool, ArrTool::Cut);
    click(&mut rt, "tool-pointer");
    assert_eq!(rt.state.song.tool, ArrTool::Pointer);
}

#[test]
fn group_button_uses_the_selected_regions_tracks() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    click(&mut rt, &format!("region-{r}"));
    click(&mut rt, "add-group");
    let g = rt.state.svc.engine.doc().get_groups()[0].clone();
    assert_eq!(g.track_ids, vec![d]);
}

#[test]
fn snap_select_and_zoom_buttons() {
    let mut rt = song_rt();
    click(&mut rt, "snap-trigger");
    assert!(has(&rt, "snap-menu"));
    for l in ["1/16", "1/8", "1/4", "1/2", "Bar"] {
        assert!(has(&rt, &format!("snap-{l}")), "{l}");
    }
    click(&mut rt, "snap-Bar");
    assert_eq!(rt.state.song.snap, 4.0);
    click(&mut rt, "zoom-in");
    assert_eq!(rt.state.song.ppb, 40.0);
    assert_eq!(texts_of(&rt, "zoom-pct"), "133%");
    for _ in 0..6 {
        click(&mut rt, "zoom-in");
    }
    assert_eq!(rt.state.song.ppb, 80.0, "clamped at the max");
    click(&mut rt, "zoom-in");
    assert_eq!(rt.state.song.ppb, 80.0, "disabled at the limit");
    click(&mut rt, "zoom-reset");
    assert_eq!(rt.state.song.ppb, DEFAULT_PPB);
    for _ in 0..4 {
        click(&mut rt, "zoom-out");
    }
    assert_eq!(rt.state.song.ppb, 10.0);
}

#[test]
fn cmd_wheel_zooms_keeping_the_beat_under_the_pointer() {
    let mut rt = song_rt();
    let l = rect(&rt, "song-lanes");
    let x = l[0] + 300.0;
    let y = l[1] + 300.0;
    // Wheel up (DOM deltaY < 0) zooms in: −deltaY × 0.5.
    wheel_mods(&mut rt, x, y, 0.0, 20.0, cmd());
    let ppb = rt.state.song.ppb;
    assert!(ppb > 30.0, "{ppb}");
    let beat_before = 300.0 / 30.0;
    let beat_now = (300.0 + rt.state.song.scroll[0]) as f64 / ppb;
    assert!((beat_now - beat_before).abs() < 0.05, "{beat_now}");
    // Plain wheel scrolls.
    let sy0 = rt.state.song.scroll[0];
    wheel_mods(&mut rt, x, y, -40.0, 0.0, rune::Mods::default());
    assert!(rt.state.song.scroll[0] > sy0);
}

#[test]
fn auto_follow_scrolls_when_the_playhead_leaves_the_view() {
    let mut rt = song_rt();
    let vw = rt.state.song.viewport.get()[0];
    rt.state.song_play();
    // Jump the playhead past the right edge (50-point margin).
    let beat = (vw as f64 + 10.0) / 30.0;
    rt.state.svc.engine.set_position(beat);
    run(&mut rt, 0.1);
    let sl = rt.state.song.scroll[0];
    let x = (rt.state.svc.engine.get_position() * 30.0) as f32;
    // `scrollLeft = playheadX − 100`, clamped to the content like the browser does.
    let max = rt.state.content_width() - vw;
    assert!(sl > 0.0 && (sl - (x - 100.0).min(max)).abs() < 20.0, "scroll {sl} playhead {x}");
    rt.state.song_stop();
}

#[test]
fn window_keys_select_all_delete_escape_and_tools() {
    let mut rt = song_rt();
    let n: usize = rt.state.tracks().iter().map(|t| t.regions.len()).sum();
    press(&mut rt, ev(Key::Command('a'), cmd()));
    assert_eq!(rt.state.song.selection.len(), n);
    press(&mut rt, ev(Key::Escape, rune::Mods::default()));
    assert!(rt.state.song.selection.is_empty());
    press(&mut rt, ev(Key::Command('a'), cmd()));
    press(&mut rt, ev(Key::Delete, rune::Mods::default()));
    assert_eq!(rt.state.tracks().iter().map(|t| t.regions.len()).sum::<usize>(), 0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Region"));
    press(&mut rt, text_key("c"));
    assert_eq!(rt.state.song.tool, ArrTool::Cut);
    press(&mut rt, text_key("v"));
    assert_eq!(rt.state.song.tool, ArrTool::Pointer);
}

#[test]
fn delete_removes_the_region_under_the_context_menu() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    let (x, y) = at(&rt, &format!("region-{r}"));
    right_click_at(&mut rt, x, y);
    assert!(has(&rt, "region-menu"));
    press(&mut rt, ev(Key::Backspace, rune::Mods::default()));
    assert!(rt.state.find_track(&d).unwrap().regions.is_empty());
}

#[test]
fn region_click_selects_and_shift_toggles() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let b = tid(&rt, "Bass");
    let rd = rid(&rt, &d, 0);
    let rb = rid(&rt, &b, 0);
    click(&mut rt, &format!("region-{rd}"));
    assert_eq!(rt.state.song.selection, vec![(rd.clone(), d.clone())]);
    assert_eq!(undo_depth(&rt), 0, "a click without moving is no undo step");
    let (x, y) = at(&rt, &format!("region-{rb}"));
    click_mods(&mut rt, x, y, shift());
    assert_eq!(rt.state.song.selection.len(), 2);
    run(&mut rt, 0.8); // not a double click
    click_mods(&mut rt, x, y, shift());
    assert_eq!(rt.state.song.selection, vec![(rd.clone(), d.clone())]);
    run(&mut rt, 0.8);
    click(&mut rt, &format!("region-{rb}"));
    assert_eq!(rt.state.song.selection, vec![(rb, b)]);
}

#[test]
fn region_move_is_snapped_clamped_and_one_undo_step() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let (x, y) = at(&rt, &format!("region-{r}"));
    drag(&mut rt, x, y, 3.0 * 30.0 + 7.0, 0.0);
    assert_eq!(region(&rt, &b, &r).start_beat, 3.0, "snapped to the beat");
    assert_eq!(undo_depth(&rt), 1, "one undo step for the whole drag");
    assert_eq!(undo_label(&rt).as_deref(), Some("Move Region"));
    let (x, y) = at(&rt, &format!("region-{r}"));
    drag(&mut rt, x, y, -400.0, 0.0);
    assert_eq!(region(&rt, &b, &r).start_beat, 0.0, "minimum 0");
    rt.state.undo();
    rt.state.undo();
    assert_eq!(region(&rt, &b, &r).start_beat, 0.0);
}

#[test]
fn region_edges_resize_with_a_minimum_length() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let len = region(&rt, &b, &r).length_beats;
    let rr = rect(&rt, &format!("region-{r}"));
    // Right edge: +2 beats.
    drag(&mut rt, rr[0] + rr[2] - 2.0, rr[1] + 30.0, 60.0, 0.0);
    assert_eq!(region(&rt, &b, &r).length_beats, len + 2.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Trim Region"));
    assert_eq!(undo_depth(&rt), 1);
    // Left edge: start moves, the end stays.
    let rr = rect(&rt, &format!("region-{r}"));
    drag(&mut rt, rr[0] + 2.0, rr[1] + 30.0, 4.0 * 30.0, 0.0);
    let reg = region(&rt, &b, &r);
    assert_eq!((reg.start_beat, reg.length_beats), (4.0, len + 2.0 - 4.0));
    // Right edge shrunk past the start keeps the snap (1 beat).
    let rr = rect(&rt, &format!("region-{r}"));
    drag(&mut rt, rr[0] + rr[2] - 2.0, rr[1] + 30.0, -2000.0, 0.0);
    assert_eq!(region(&rt, &b, &r).length_beats, 1.0);
}

#[test]
fn cut_tool_splits_at_the_snapped_beat() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    rt.state.song.tool = ArrTool::Cut;
    rt.invalidate();
    frames(&mut rt, 2);
    let rr = rect(&rt, &format!("region-{r}"));
    click_at(&mut rt, rr[0] + 4.0 * 30.0 + 9.0, rr[1] + 30.0);
    let regs = &rt.state.find_track(&d).unwrap().regions;
    assert_eq!(regs.len(), 2);
    assert_eq!(regs[0].length_beats, 4.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Split Region"));
}

#[test]
fn double_click_opens_the_piano_roll_except_audio() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    double_click(&mut rt, &format!("region-{r}"));
    assert_eq!(rt.state.song.editing, Some((b.clone(), r.clone())));
    assert!(has(&rt, "piano-roll"));
    assert!(!has(&rt, "song-dock"), "the dock hides while the piano roll is open");
    // Audio regions have no editor.
    rt.state.close_piano_roll();
    let audio = mdaw_model::SampleRef {
        id: "s1".into(),
        name: "Break".into(),
        path: "/b.wav".into(),
        duration_seconds: 2.0,
        sample_rate: 44100.0,
        channels: 2,
    };
    let reg = rt.state.svc.engine.edit(|d| d.import_audio_sample(&audio)).unwrap();
    let at_id = rt.state.tracks().last().unwrap().id.clone();
    rt.invalidate();
    frames(&mut rt, 3);
    double_click(&mut rt, &format!("region-{}", reg.id));
    assert_eq!(rt.state.song.editing, None);
    let _ = at_id;
    // Its sample isn't loaded (no sample manager yet): the missing badge shows.
    assert!(has(&rt, "sample-badge-missing"));
}

#[test]
fn region_menu_items_and_actions() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    let (x, y) = at(&rt, &format!("region-{r}"));
    right_click_at(&mut rt, x, y);
    for k in ["region-split", "region-duplicate", "region-copy", "region-quantize", "region-delete"] {
        assert!(has(&rt, k), "{k}");
    }
    assert!(!has(&rt, "region-paste"), "no clipboard yet");
    click(&mut rt, "region-copy");
    assert!(!has(&rt, "region-menu"));
    right_click_at(&mut rt, x, y);
    assert!(has(&rt, "region-paste"));
    // Paste at the playhead on that track (while playing: stopped, the playhead reads 0).
    rt.state.song_play();
    rt.state.svc.engine.set_position(32.0);
    click(&mut rt, "region-paste");
    rt.state.song_stop();
    frames(&mut rt, 2);
    let regs = rt.state.find_track(&d).unwrap().regions.clone();
    assert_eq!(regs.len(), 2);
    assert!((regs[1].start_beat - 32.0).abs() < 0.2, "{}", regs[1].start_beat);
    assert_eq!(undo_label(&rt).as_deref(), Some("Paste Region"));
    // Split at the playhead (engine position).
    run(&mut rt, 0.8);
    right_click_at(&mut rt, x, y);
    rt.state.song_play();
    rt.state.svc.engine.set_position(4.0);
    click(&mut rt, "region-split");
    rt.state.song_stop();
    frames(&mut rt, 2);
    assert_eq!(undo_label(&rt).as_deref(), Some("Split Region"));
    assert_eq!(rt.state.find_track(&d).unwrap().regions.len(), 3);
    // Duplicate, quantize (with the current snap), delete.
    let r0 = rid(&rt, &d, 0);
    let (x, y) = at(&rt, &format!("region-{r0}"));
    run(&mut rt, 0.8);
    right_click_at(&mut rt, x, y);
    click(&mut rt, "region-duplicate");
    assert_eq!(undo_label(&rt).as_deref(), Some("Duplicate Region"));
    right_click_at(&mut rt, x, y);
    click(&mut rt, "region-quantize");
    assert_eq!(undo_label(&rt).as_deref(), Some("Quantize Notes"));
    right_click_at(&mut rt, x, y);
    click(&mut rt, "region-delete");
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Region"));
    // Escape closes the menu.
    right_click_at(&mut rt, x + 200.0, y);
    if has(&rt, "region-menu") {
        press(&mut rt, ev(Key::Escape, rune::Mods::default()));
        assert!(!has(&rt, "region-menu"));
    }
}

#[test]
fn region_menu_hides_quantize_on_audio_regions() {
    let mut rt = song_rt();
    let audio = mdaw_model::SampleRef {
        id: "s1".into(),
        name: "Break".into(),
        path: "/b.wav".into(),
        duration_seconds: 2.0,
        sample_rate: 44100.0,
        channels: 2,
    };
    let reg = rt.state.svc.engine.edit(|d| d.import_audio_sample(&audio)).unwrap();
    rt.invalidate();
    frames(&mut rt, 3);
    let (x, y) = at(&rt, &format!("region-{}", reg.id));
    right_click_at(&mut rt, x, y);
    assert!(has(&rt, "region-menu"));
    assert!(!has(&rt, "region-quantize"));
}

#[test]
fn rubber_band_selects_regions_of_rows_below_the_first_track() {
    // Electron bug fixed in both apps (user decision 2026-11-18): the TS skipped rows above
    // the band without advancing its running top, so this band selected nothing.
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let rb = rid(&rt, &b, 0);
    let l = rect(&rt, "song-lanes");
    // From empty space right of the regions on the Lead row, up-left into the Bass region.
    let (x0, y0) = (l[0] + 600.0, l[1] + 64.0 * 2.0 + 30.0);
    drag(&mut rt, x0, y0, -200.0, -60.0);
    assert_eq!(rt.state.song.selection, vec![(rb.clone(), b.clone())]);
    // Shift adds; a plain click on empty space clears.
    let d = tid(&rt, "Drums");
    let rd = rid(&rt, &d, 0);
    drag_mods(&mut rt, l[0] + 600.0, l[1] + 40.0, -200.0, -20.0, shift());
    assert_eq!(rt.state.song.selection.len(), 2);
    assert!(rt.state.song.selection.contains(&(rd, d)));
    click_at(&mut rt, l[0] + 900.0, l[1] + 400.0);
    assert!(rt.state.song.selection.is_empty());
}

#[test]
fn rubber_band_counts_group_rows_and_skips_collapsed_groups() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let b = tid(&rt, "Bass");
    let l = tid(&rt, "Lead");
    rt.state.svc.engine.edit(|doc| {
        doc.create_group("G", &[b.clone()]);
    });
    rt.invalidate();
    frames(&mut rt, 3);
    // Rows: group (28), Bass (64), Drums (64), Lead, Pad.
    let rows = rt.state.song_rows();
    assert!(matches!(&rows[0], crate::song::Row::Group { top, .. } if *top == 0.0));
    assert!(matches!(&rows[1], crate::song::Row::Track { id, top, .. } if *id == b && *top == 28.0));
    let lanes = rect(&rt, "song-lanes");
    drag(&mut rt, lanes[0] + 600.0, lanes[1] + 28.0 + 64.0 + 40.0, -200.0, -10.0);
    let rd = rid(&rt, &d, 0);
    assert_eq!(rt.state.song.selection, vec![(rd, d)]);
    let _ = l;
}

#[test]
fn header_empty_state() {
    let mut rt = song_rt();
    let ids: Vec<String> = rt.state.tracks().iter().map(|t| t.id.clone()).collect();
    for id in ids {
        rt.state.remove_song_track(&id);
    }
    rt.invalidate();
    frames(&mut rt, 3);
    let t = texts(&rt);
    assert!(t.contains(&"No tracks yet".to_owned()));
    assert!(t.contains(
        &"Add a synth or drum track, arm it, and press R to record from your keyboard.".to_owned()
    ));
    assert!(has(&rt, "empty-add-drums"));
    click(&mut rt, "empty-add-synth");
    assert_eq!(rt.state.tracks().len(), 1);
    assert_eq!(rt.state.tracks()[0].instrument.kind, mdaw_model::InstrumentType::Synth);
    assert!(!has(&rt, "song-empty"));
}

#[test]
fn scene_region_selected_shows_the_selection() {
    let mut app = crate::app::App::headless();
    crate::scenes_song::region_selected(&mut app);
    assert_eq!(app.song.selection.len(), 1);
    let mut rt = runtime(app);
    let r = rt.state.song.selection[0].0.clone();
    let fill = |rt: &rune::Runtime<crate::app::App>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(&format!("region-{r}")))
            .unwrap()
            .fill
    };
    let selected = fill(&rt);
    rt.state.clear_selection();
    rt.invalidate();
    frames(&mut rt, 3);
    assert_ne!(selected, fill(&rt), "the selected region draws its stronger tint");
}


