//! Phase-8 review fixes (`docs/rune-port/review-phase-8.md`): interrupted drags close their undo
//! group (#1), the recording region grows live (#3), the master readout and CLIP lamp are live
//! (#4), the header volume slider's one geometry (#5), instrument search keys (#7), Ctrl as ⌘
//! (#8), selected regions on top (#9), group rename on blur (#10), the selection ring (#12),
//! no-op splits (#13) and the Song cue (#14).

use rune::{Key, Mods};

use super::harness::*;
use crate::app::DawView;
use crate::song::LoopEdge;

type Rt = rune::Runtime<crate::app::App>;

fn grouping(rt: &Rt) -> bool {
    rt.state.svc.engine.doc().undo_manager().is_grouping()
}

fn none() -> Mods {
    Mods::default()
}

fn ctrl() -> Mods {
    Mods {
        ctrl: true,
        ..Mods::default()
    }
}

/// Press on `key`, move by `dx` in a few frames, keep the button down.
fn hold_drag(rt: &mut Rt, key: &str, dx: f32, dy: f32) -> (f32, f32) {
    let (x, y) = at(rt, key);
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.frame(DT);
    for i in 1..=4 {
        let f = i as f32 / 4.0;
        rt.pointer_move(x + dx * f, y + dy * f);
        rt.frame(DT);
    }
    (x + dx, y + dy)
}

/// One more undoable edit must be its own step (not swallowed into an open group).
fn later_edit_is_its_own_step(rt: &mut Rt) {
    let d = undo_depth(rt);
    rt.state.add_marker_at_playhead();
    assert_eq!(
        undo_depth(rt),
        d + 1,
        "the edit after the gesture is its own step"
    );
}

// --- #1 interrupted drags ----------------------------------------------------------------------

#[test]
fn delete_mid_region_drag_ends_the_drag_first() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let start = region(&rt, &b, &r).start_beat;
    let (x, y) = hold_drag(&mut rt, &format!("region-{r}"), 60.0, 0.0);
    assert_eq!(region(&rt, &b, &r).start_beat, start + 2.0);
    assert!(grouping(&rt));
    press(&mut rt, ev(Key::Delete, none()));
    assert!(!grouping(&rt), "the delete closed the drag's group first");
    assert!(
        rt.state
            .find_track(&b)
            .unwrap()
            .regions
            .iter()
            .all(|x| x.id != r)
    );
    assert_eq!(undo_depth(&rt), 2, "Move Region, then the delete");
    assert_ne!(undo_label(&rt).as_deref(), Some("Move Region"));
    // The rest of the pointer gesture does nothing.
    rt.pointer_move(x + 60.0, y);
    frames(&mut rt, 2);
    rt.pointer_up(x + 60.0, y);
    frames(&mut rt, 2);
    assert_eq!(undo_depth(&rt), 2);
    later_edit_is_its_own_step(&mut rt);
}

#[test]
fn a_region_gone_or_the_view_switched_mid_drag_ends_it_next_frame() {
    // The region removed from elsewhere (an undo, a menu) while the button is held.
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    hold_drag(&mut rt, &format!("region-{r}"), 60.0, 0.0);
    rt.state.svc.engine.edit(|d| d.remove_region(&b, &r));
    frames(&mut rt, 2);
    assert!(!grouping(&rt));
    assert!(rt.state.song.region_drag.is_none());
    later_edit_is_its_own_step(&mut rt);

    // ⌘2 / the view toggle while dragging: the timeline unmounts.
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let r = rid(&rt, &d, 0);
    hold_drag(&mut rt, &format!("region-{r}"), 60.0, 0.0);
    rt.state.view = DawView::Beat;
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(!grouping(&rt));
    assert_eq!(undo_label(&rt).as_deref(), Some("Move Region"));
    later_edit_is_its_own_step(&mut rt);
}

#[test]
fn l_mid_loop_handle_drag_ends_it() {
    let mut rt = song_rt();
    rt.state.set_loop_markers(8.0, 16.0);
    rt.state.toggle_song_loop();
    rt.invalidate();
    run(&mut rt, 0.3);
    let depth = undo_depth(&rt);
    let (x, y) = hold_drag(&mut rt, "loop-end", 4.0 * 30.0, 0.0);
    assert_eq!(rt.state.song.loop_drag, Some(LoopEdge::End));
    assert!(grouping(&rt));
    press(&mut rt, ev(Key::Text("l".into()), none()));
    assert!(!rt.state.svc.engine.doc().is_loop_enabled());
    assert!(!grouping(&rt));
    assert_eq!(rt.state.song.loop_drag, None);
    let m = rt.state.svc.engine.doc().get_loop_markers().unwrap();
    rt.pointer_move(x + 120.0, y);
    frames(&mut rt, 2);
    rt.pointer_up(x + 120.0, y);
    frames(&mut rt, 2);
    assert_eq!(
        rt.state.svc.engine.doc().get_loop_markers(),
        Some(m),
        "no more moves"
    );
    assert!(undo_depth(&rt) > depth);
    later_edit_is_its_own_step(&mut rt);
}

#[test]
fn closing_the_fx_drawer_mid_knob_drag_ends_it() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.toggle_fx(&b);
    rt.state.toggle_fx_section(crate::song::FxSection::Eq);
    rt.invalidate();
    run(&mut rt, 0.5);
    let depth = undo_depth(&rt);
    hold_drag(&mut rt, "fx-knob-eq-low", 0.0, -50.0);
    assert!(grouping(&rt));
    rt.state.toggle_fx(&b); // the drawer closes (FX button, Escape, another track)
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(!grouping(&rt));
    assert!(rt.state.beat.knob_drag.is_none());
    assert_eq!(undo_depth(&rt), depth + 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Change Track Effect"));
    later_edit_is_its_own_step(&mut rt);
}

#[test]
fn cmd_z_mid_drag_undoes_the_drag_so_far_like_the_ts_flush_group() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let start = region(&rt, &b, &r).start_beat;
    let (x, y) = hold_drag(&mut rt, &format!("region-{r}"), 90.0, 0.0);
    assert_eq!(region(&rt, &b, &r).start_beat, start + 3.0);
    // ⌘Z: the drag so far is committed as its own step and undone; the group stays open.
    press(&mut rt, ev(Key::Command('z'), cmd()));
    assert_eq!(region(&rt, &b, &r).start_beat, start);
    assert!(grouping(&rt), "the gesture's own release still balances");
    // The drag goes on from the restored state and ends as one more step.
    rt.pointer_move(x + 60.0, y);
    frames(&mut rt, 2);
    rt.pointer_up(x + 60.0, y);
    frames(&mut rt, 2);
    assert!(!grouping(&rt));
    assert_eq!(region(&rt, &b, &r).start_beat, start + 5.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Move Region"));
    rt.state.undo();
    assert_eq!(region(&rt, &b, &r).start_beat, start);
}

// --- #3 live recording region ------------------------------------------------------------------

#[test]
fn the_recording_region_grows_every_frame() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.toggle_arm(&b);
    rt.state.song_record();
    rt.invalidate();
    run(&mut rt, 0.4);
    assert!(has(&rt, "live-region"));
    let w0 = rect(&rt, "live-region")[2];
    run(&mut rt, 2.0);
    let live = rt.state.svc.engine.get_live_region().unwrap();
    let w1 = rect(&rt, "live-region")[2];
    let want = (live.length_beats * rt.state.song.ppb) as f32;
    assert!(w1 > w0 + 60.0, "{w0} → {w1}");
    assert!((w1 - want).abs() < 8.0, "drawn {w1}, engine {want}");
    rt.state.song_stop();
    run(&mut rt, 0.3);
    assert!(!has(&rt, "live-region"));
}

// --- #4 master readout -------------------------------------------------------------------------

#[test]
fn the_master_readout_and_clip_lamp_follow_the_level() {
    let mut rt = song_rt();
    rt.state.song.show_mixer = true;
    rt.state.song_play();
    rt.invalidate();
    run(&mut rt, 1.5);
    let level = *rt.state.song.live.levels.borrow().get("").unwrap();
    assert!(level.is_finite(), "the template plays: {level}");
    assert_eq!(
        rt.presented("clip-lamp-lit").unwrap().1,
        0.0,
        "below −0.5 dBFS: dark"
    );
    let shown = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("master-db"))
        .and_then(|e| e.text)
        .unwrap();
    assert_eq!(shown, crate::song::master_db_text(level));
    // +12 dB on every band of every track: the master goes over −0.5 dBFS and the lamp lights
    // without the view re-running.
    rt.state.svc.engine.set_master_volume_db(0.0);
    let ids: Vec<String> = rt.state.tracks().iter().map(|t| t.id.clone()).collect();
    for id in &ids {
        let mut m = serde_json::Map::new();
        for (k, v) in [
            ("enabled", serde_json::json!(true)),
            ("low", 12.into()),
            ("mid", 12.into()),
            ("high", 12.into()),
        ] {
            m.insert(k.into(), v);
        }
        rt.state
            .set_track_effect(id, mdaw_model::doc::EffectName::Eq, m);
    }
    let mut lit = 0.0;
    for _ in 0..180 {
        frames(&mut rt, 1);
        lit = rt.presented("clip-lamp-lit").unwrap().1;
        if lit > 0.0 {
            break;
        }
    }
    let level = *rt.state.song.live.levels.borrow().get("").unwrap();
    assert_eq!(lit, 1.0, "lit at {level} dBFS");
    assert!(level > -0.5);
    rt.state.song_stop();
    run(&mut rt, 2.0);
    let shown = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("master-db"))
        .and_then(|e| e.text)
        .unwrap();
    let level = *rt.state.song.live.levels.borrow().get("").unwrap();
    assert_eq!(shown, crate::song::master_db_text(level));
}

// --- #5 volume slider geometry -----------------------------------------------------------------

#[test]
fn the_volume_thumb_is_drawn_where_a_press_reads_its_value() {
    use crate::ui::song::headers::{vol_from_x, vol_thumb_center};
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.set_track_volume(&b, -6.0);
    rt.invalidate();
    frames(&mut rt, 2);
    let s = rect(&rt, &format!("vol-{b}"));
    let th = rect(&rt, &format!("vol-{b}-thumb"));
    let cx = th[0] + th[2] / 2.0 - s[0];
    let f = ((-6.0 - crate::song::MIN_VOLUME) / 40.0) as f32;
    assert!(
        (cx - vol_thumb_center(f, s[2])).abs() < 0.5,
        "{cx} vs {}",
        vol_thumb_center(f, s[2])
    );
    assert_eq!(vol_from_x(cx, s[2]), -6.0);
    // Pressing the thumb keeps −6 dB (the old paint put it where a press read −3 dB).
    click_at(&mut rt, s[0] + cx, s[1] + s[3] / 2.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -6.0);
    // The ends.
    click_at(&mut rt, s[0] + 1.0, s[1] + s[3] / 2.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -40.0);
    click_at(&mut rt, s[0] + s[2] - 1.0, s[1] + s[3] / 2.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, 0.0);
    // The thumb is painted last (on top of the rails).
    let order: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with(&format!("vol-{b}")))
        .collect();
    assert_eq!(
        order.last().map(String::as_str),
        Some(format!("vol-{b}-thumb").as_str())
    );
}

// --- #7 instrument search keys -----------------------------------------------------------------

#[test]
fn instrument_search_down_and_enter_choose_and_focus_returns() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    click_settle(&mut rt, &format!("instrument-{b}"));
    let results = crate::ui::song::headers::instrument_results(&rt.state, &b);
    assert!(results.len() > 3);
    // cmdk highlights the first result.
    assert_eq!(rt.state.song.instrument_active, 0);
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, ev(Key::Down, none()));
    press(&mut rt, ev(Key::Up, none()));
    assert_eq!(rt.state.song.instrument_active, 1);
    press(&mut rt, ev(Key::Enter, none()));
    run(&mut rt, 0.4);
    assert!(rt.state.menu.is_none());
    let inst = &rt.state.find_track(&b).unwrap().instrument;
    assert_eq!(
        (inst.kind, inst.preset_index),
        (results[1].kind, results[1].preset)
    );
    assert!(
        rt.state
            .focus_seen
            .as_deref()
            .is_some_and(|k| k.ends_with(&format!("instrument-{b}"))),
        "focus back on the trigger: {:?}",
        rt.state.focus_seen
    );
    // Typing resets the active result to the first match; Enter takes it.
    click_settle(&mut rt, &format!("instrument-{b}"));
    press(&mut rt, ev(Key::Down, none()));
    rt.paste("drum");
    press(&mut rt, ev(Key::Text("s".into()), none()));
    assert_eq!(rt.state.song.instrument_active, 0);
    press(&mut rt, ev(Key::Enter, none()));
    assert_eq!(
        rt.state.find_track(&b).unwrap().instrument.kind,
        mdaw_model::InstrumentType::Drums
    );
}

// --- #8 Ctrl as ⌘ ------------------------------------------------------------------------------

#[test]
fn ctrl_wheel_zooms_like_cmd_wheel_in_the_piano_roll_and_the_timeline() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    // Timeline: Ctrl alone zooms.
    let (lx, ly) = at(&rt, "song-lanes");
    let ppb = rt.state.song.ppb;
    wheel_mods(&mut rt, lx, ly, 0.0, 40.0, ctrl());
    assert!(rt.state.song.ppb > ppb, "Ctrl + wheel zooms the timeline");
    let sy = rt.state.song.scroll[1];
    run(&mut rt, 1.0);
    let ppb = rt.state.song.ppb;
    wheel_mods(&mut rt, lx, ly, 0.0, 40.0, cmd());
    assert!(rt.state.song.ppb > ppb, "⌘ + wheel zooms the timeline");
    assert_eq!(rt.state.song.scroll[1], sy, "and does not scroll");

    rt.state.open_piano_roll(&b, &r);
    rt.invalidate();
    run(&mut rt, 0.6);
    let g = rect(&rt, "pr-grid");
    let (x, y) = (g[0] + 200.0, g[1] + 100.0);
    for m in [ctrl(), cmd()] {
        run(&mut rt, 1.0);
        let vp = rt.state.song.piano.vp;
        wheel_mods(&mut rt, x, y, 0.0, 100.0, m);
        let now = rt.state.song.piano.vp;
        assert!(now.ppb > vp.ppb, "{m:?} + wheel zooms X");
        assert_eq!(now.top, vp.top, "{m:?} + wheel does not scroll");
    }
    // Ctrl + Shift zooms Y like ⌘ + Shift.
    run(&mut rt, 1.0);
    let rh = rt.state.song.piano.vp.row_h;
    wheel_mods(
        &mut rt,
        x,
        y,
        0.0,
        200.0,
        Mods {
            ctrl: true,
            shift: true,
            ..Mods::default()
        },
    );
    assert!(rt.state.song.piano.vp.row_h > rh);
    // Ctrl + A selects every note (`e.metaKey || e.ctrlKey`).
    press(&mut rt, ev(Key::Text("a".into()), ctrl()));
    assert_eq!(
        rt.state.song.piano.selection.len(),
        region(&rt, &b, &r).notes.len()
    );
}

// --- #9 / #12 selected regions on top, ring overlay --------------------------------------------

#[test]
fn a_selected_region_is_on_top_of_an_overlapping_one_with_its_ring_outside() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r0 = rid(&rt, &b, 0);
    let first = region(&rt, &b, &r0);
    // A second region over the first one's last 2 beats, after it in the list (drawn above).
    let over = mdaw_model::Region {
        id: String::new(),
        start_beat: first.start_beat + first.length_beats - 2.0,
        length_beats: 4.0,
        notes: Vec::new(),
        color: None,
        name: Some("Over".into()),
        audio: None,
        extra: Default::default(),
    };
    rt.state.svc.engine.edit(|d| {
        d.add_region(&b, over);
    });
    rt.invalidate();
    frames(&mut rt, 2);
    let r1 = rt
        .state
        .find_track(&b)
        .unwrap()
        .regions
        .last()
        .unwrap()
        .id
        .clone();
    // The overlap: one beat into the second region.
    let rr = rect(&rt, &format!("region-{r1}"));
    let (ox, oy) = (rr[0] + 30.0, rr[1] + 30.0);
    click_at(&mut rt, ox, oy);
    assert_eq!(
        rt.state.song.selection[0].0, r1,
        "unselected: the later region is on top"
    );
    run(&mut rt, 0.8);
    click(&mut rt, &format!("region-{r0}")); // its non-overlapping middle
    assert_eq!(rt.state.song.selection[0].0, r0);
    run(&mut rt, 0.8);
    click_at(&mut rt, ox, oy);
    assert_eq!(
        rt.state.song.selection[0].0, r0,
        "the selected region takes the overlap"
    );
    // The ring: 1.5 points outside the box, inert, drawn after every region.
    let a = rect(&rt, &format!("region-{r0}"));
    let ring = rect(&rt, &format!("region-{r0}-ring"));
    assert_eq!(ring, [a[0] - 1.5, a[1] - 1.5, a[2] + 3.0, a[3] + 3.0]);
    let keys: Vec<String> = rt
        .elements()
        .into_iter()
        .filter_map(|e| e.key)
        .filter(|k| k.starts_with("region-") && !k.contains("-edge-"))
        .collect();
    let ring_at = keys.iter().position(|k| k.ends_with("-ring")).unwrap();
    let last_region = keys.iter().rposition(|k| !k.ends_with("-ring")).unwrap();
    assert!(ring_at > last_region, "{keys:?}");
}

// --- #10 group rename on blur ------------------------------------------------------------------

#[test]
fn group_rename_commits_on_blur_and_escape_still_cancels() {
    let mut rt = song_rt();
    let d = tid(&rt, "Drums");
    let g = rt
        .state
        .svc
        .engine
        .edit(|doc| doc.create_group("Rhythm", std::slice::from_ref(&d)));
    rt.invalidate();
    run(&mut rt, 0.8);
    double_click(&mut rt, &format!("group-name-{}", g.id));
    assert!(has(&rt, "group-rename"));
    rt.paste("  Kit  ");
    frames(&mut rt, 2);
    // Focus leaves the field (a press on the ruler).
    click(&mut rt, "song-ruler");
    run(&mut rt, 0.3);
    assert_eq!(rt.state.svc.engine.doc().get_groups()[0].name, "Kit");
    assert!(!has(&rt, "group-rename"));
    // Escape cancels, a later blur commits nothing.
    run(&mut rt, 0.8);
    double_click(&mut rt, &format!("group-name-{}", g.id));
    rt.paste("Nope");
    press(&mut rt, ev(Key::Escape, none()));
    click(&mut rt, "song-ruler");
    run(&mut rt, 0.3);
    assert_eq!(rt.state.svc.engine.doc().get_groups()[0].name, "Kit");
    // Whitespace only: ignored on blur too.
    run(&mut rt, 0.8);
    double_click(&mut rt, &format!("group-name-{}", g.id));
    rt.paste("   ");
    click(&mut rt, "song-ruler");
    run(&mut rt, 0.3);
    assert_eq!(rt.state.svc.engine.doc().get_groups()[0].name, "Kit");
}

// --- #13 no-op split ---------------------------------------------------------------------------

#[test]
fn split_at_playhead_outside_the_region_pushes_nothing() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let reg = region(&rt, &b, &r);
    // The cue at the region start (a split there is outside (0, len)).
    rt.state.svc.engine.set_cue(reg.start_beat);
    let depth = undo_depth(&rt);
    rt.state.split_region_at_playhead(&b, &r);
    assert_eq!(undo_depth(&rt), depth);
    rt.state.svc.engine.set_cue(reg.start_beat + 1.0);
    rt.state.split_region_at_playhead(&b, &r);
    assert_eq!(undo_depth(&rt), depth + 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Split Region"));
}

// --- #14 the Song cue --------------------------------------------------------------------------

#[test]
fn the_cue_is_set_while_stopped_drawn_dimmed_and_shown_in_the_lcd() {
    let mut rt = song_rt();
    let r = rect(&rt, "song-ruler");
    // Bar 3 (beat 8) at 30 px per beat.
    click_at(&mut rt, r[0] + 8.0 * 30.0 + 2.0, r[1] + 14.0);
    assert_eq!(rt.state.svc.engine.get_cue(), 8.0);
    assert_eq!(rt.state.svc.engine.get_position(), 8.0);
    frames(&mut rt, 2);
    let ph = rect(&rt, "song-playhead");
    assert!(
        (ph[0] - (r[0] + 240.0)).abs() < 1.0,
        "the dimmed playhead at the cue: {ph:?}"
    );
    assert!(
        rt.presented("song-playhead").unwrap().1 < 0.6,
        "dimmed while stopped"
    );
    assert_eq!(&*rt.state.song.live.lcd_bar.get(), "  3");
    // Add marker / paste use it.
    let n = rt.state.svc.engine.doc().get_markers().len();
    press(&mut rt, ev(Key::Text("M".into()), shift()));
    let beats: Vec<f64> = rt
        .state
        .svc
        .engine
        .doc()
        .get_markers()
        .iter()
        .map(|m| m.beat)
        .collect();
    assert_eq!(beats.len(), n + 1);
    assert!(beats.contains(&8.0), "{beats:?}");
    // Space plays from the cue; Stop returns to it.
    press(&mut rt, ev(Key::Text(" ".into()), none()));
    assert!(rt.state.svc.engine.get_position() >= 8.0);
    run(&mut rt, 1.0);
    assert!(rt.state.svc.engine.get_position() > 9.0);
    press(&mut rt, ev(Key::Text(" ".into()), none()));
    assert_eq!(rt.state.svc.engine.get_position(), 8.0);
    frames(&mut rt, 2);
    assert_eq!(&*rt.state.song.live.lcd_bar.get(), "  3");
    // Home: back to bar 1.
    press(&mut rt, ev(Key::Home, none()));
    assert_eq!(rt.state.svc.engine.get_cue(), 0.0);
}

#[test]
fn markers_and_brackets_move_the_cue_and_the_piano_roll_shares_it() {
    let mut rt = song_rt();
    // The template's markers: 0, 16, 32, 48.
    let beats = |rt: &Rt| -> Vec<f64> {
        rt.state
            .svc
            .engine
            .doc()
            .get_markers()
            .iter()
            .map(|m| m.beat)
            .collect()
    };
    assert_eq!(beats(&rt), vec![0.0, 16.0, 32.0, 48.0]);
    press(&mut rt, ev(Key::Text("]".into()), none()));
    assert_eq!(rt.state.svc.engine.get_cue(), 16.0);
    press(&mut rt, ev(Key::Text("]".into()), none()));
    assert_eq!(rt.state.svc.engine.get_cue(), 32.0);
    press(&mut rt, ev(Key::Text("[".into()), none()));
    assert_eq!(rt.state.svc.engine.get_cue(), 16.0);
    // A marker flag click.
    let id = rt.state.svc.engine.doc().get_markers()[2].id.clone();
    click(&mut rt, &format!("marker-{id}"));
    assert_eq!(rt.state.svc.engine.get_cue(), 32.0);
    // The marker jump list.
    let id = rt.state.svc.engine.doc().get_markers()[3].id.clone();
    rt.state.seek_to_marker(&id);
    assert_eq!(rt.state.svc.engine.get_cue(), 48.0);
    // The piano roll: opening puts the cue at the region start; its ruler sets the same value.
    let b = tid(&rt, "Bass");
    let r = rid(&rt, &b, 0);
    let start = region(&rt, &b, &r).start_beat;
    rt.state.open_piano_roll(&b, &r);
    rt.invalidate();
    run(&mut rt, 0.6);
    assert_eq!(rt.state.svc.engine.get_cue(), start);
    rt.state.pr_set_position(2.0);
    assert_eq!(rt.state.svc.engine.get_cue(), start + 2.0);
    // `]` while the editor is open moves the shared cue (redrawn there).
    press(&mut rt, ev(Key::Text("]".into()), none()));
    assert!(rt.state.svc.engine.get_cue() > start + 2.0);
    rt.state.close_piano_roll();
    assert_eq!(
        rt.state.svc.engine.get_position(),
        rt.state.svc.engine.get_cue()
    );
}
