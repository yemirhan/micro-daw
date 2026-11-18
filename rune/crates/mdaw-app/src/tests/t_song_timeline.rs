//! `inventory/views.md` §5 — TimelineRuler, MarkerFlag, MarkerContextMenu, MarkerJumpDropdown,
//! the loop region, AutomationLaneView, the playhead and the Song keys (⇧M, ] / [).

use rune::Key;

use super::harness::*;
use super::t_song_arr::texts_of;
use mdaw_model::AutomationParameter;

/// The playhead reads `b` (the offline transport keeps rolling a few frames after a seek).
fn near(rt: &rune::Runtime<crate::app::App>, b: f64) -> bool {
    let p = rt.state.svc.engine.get_position();
    p >= b - 0.05 && p < b + 1.5
}

fn markers(rt: &rune::Runtime<crate::app::App>) -> Vec<mdaw_model::Marker> {
    rt.state.svc.engine.doc().get_markers().to_vec()
}

#[test]
fn ruler_click_seeks_snapped_and_clears_the_selection() {
    let mut rt = song_rt();
    rt.state.select_all_regions();
    rt.state.song_play();
    let r = rect(&rt, "song-ruler");
    // Beat 6.4 → snapped to 6 (snap 1/4 = 1 beat).
    click_at(&mut rt, r[0] + 6.4 * 30.0, r[1] + 20.0);
    assert!(near(&rt, 6.0), "{}", rt.state.svc.engine.get_position());
    assert!(rt.state.song.selection.is_empty());
    rt.state.song_stop();
    // Tooltip.
    let e = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("song-ruler")).unwrap();
    assert_eq!(e.help.as_deref(), Some("Click to move the playhead"));
}

#[test]
fn ruler_labels_every_n_bars_and_beat_ticks_from_14_px() {
    let mut rt = song_rt();
    let t = texts(&rt);
    for b in ["1", "2", "3", "9"] {
        assert!(t.contains(&b.to_owned()), "bar {b} labelled at 30 px/beat");
    }
    // 10 px/beat: a bar is 40 px, labels every bar still (≥ 32 px); 1, 2, 3...
    rt.state.song_zoom(10.0, None);
    rt.invalidate();
    frames(&mut rt, 3);
    assert!(texts(&rt).contains(&"2".to_owned()));
    // The rule: N in {1, 2, 4, 8, 16, 32} with N × bar ≥ 32, else 64.
    let every = |ppb: f64| {
        [1i64, 2, 4, 8, 16, 32]
            .into_iter()
            .find(|n| *n as f64 * ppb * 4.0 >= 32.0)
            .unwrap_or(64)
    };
    assert_eq!((every(10.0), every(5.0), every(1.0), every(0.1)), (1, 2, 8, 64));
}

#[test]
fn loop_region_always_drawn_handles_only_when_on() {
    let mut rt = song_rt();
    rt.state.set_loop_markers(8.0, 24.0);
    rt.invalidate();
    frames(&mut rt, 3);
    assert!(has(&rt, "loop-region"));
    assert!(!has(&rt, "loop-start") && !has(&rt, "loop-end"));
    press(&mut rt, text_key("l"));
    assert!(rt.state.svc.engine.doc().is_loop_enabled());
    run(&mut rt, 0.5);
    assert!(has(&rt, "loop-start") && has(&rt, "loop-end"));
}

#[test]
fn loop_handles_drag_snapped_one_undo_step_and_at_least_one_beat() {
    let mut rt = song_rt();
    rt.state.set_loop_markers(8.0, 24.0);
    rt.state.toggle_song_loop();
    rt.invalidate();
    run(&mut rt, 0.5);
    let depth = undo_depth(&rt);
    let (x, y) = at(&rt, "loop-end");
    drag(&mut rt, x, y, 4.0 * 30.0 + 5.0, 0.0);
    let m = rt.state.svc.engine.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (8.0, 28.0));
    assert_eq!(undo_depth(&rt), depth + 1, "the drag is one undo step");
    assert_eq!(undo_label(&rt).as_deref(), Some("Set Loop Markers"));
    // The start can't pass the end − 1.
    run(&mut rt, 0.5);
    let (x, y) = at(&rt, "loop-start");
    drag(&mut rt, x, y, 40.0 * 30.0, 0.0);
    let m = rt.state.svc.engine.doc().get_loop_markers().unwrap();
    assert_eq!((m.start_beat, m.end_beat), (27.0, 28.0));
}

#[test]
fn marker_flag_click_jumps_drag_moves_right_click_edits() {
    let mut rt = song_rt();
    let ms = markers(&rt);
    let verse = ms.iter().find(|m| m.name == "Verse").unwrap().clone();
    let key = format!("marker-{}", verse.id);
    let e = rt.elements().into_iter().find(|e| e.key.as_deref() == Some(&key)).unwrap();
    assert_eq!(
        e.help.as_deref(),
        Some("Verse: click to jump, drag to move, right-click to edit")
    );
    // Click while playing seeks there.
    rt.state.song_play();
    let (x, y) = at(&rt, &format!("marker-label-{}", verse.id));
    click_at(&mut rt, x, y);
    assert!(near(&rt, verse.beat), "{}", rt.state.svc.engine.get_position());
    rt.state.song_stop();
    run(&mut rt, 0.8);
    // Drag: snapped, committed once on release ("Update Marker").
    // The pointer's beat is what snaps (as the TS reads clientX): grab the line.
    let fr = rect(&rt, &format!("marker-{}", verse.id));
    drag(&mut rt, fr[0] + 0.5, fr[1] + 20.0, 2.0 * 30.0 + 4.0, 0.0);
    let m = markers(&rt).into_iter().find(|m| m.id == verse.id).unwrap();
    assert_eq!(m.beat, verse.beat + 2.0);
    assert_eq!(undo_label(&rt).as_deref(), Some("Update Marker"));
    assert_eq!(undo_depth(&rt), 1);
    // Right-click: the menu.
    let (x, y) = at(&rt, &format!("marker-label-{}", verse.id));
    right_click_at(&mut rt, x, y);
    assert!(has(&rt, "marker-menu"));
    assert!(has(&rt, "marker-rename-item"));
    for i in 0..mdaw_model::doc::MARKER_COLORS.len() {
        assert!(has(&rt, &format!("marker-color-{i}")), "{i}");
    }
    assert!(has(&rt, "marker-delete"));
}

#[test]
fn marker_menu_rename_enter_escape_colour_delete() {
    let mut rt = song_rt();
    let id = markers(&rt)[0].id.clone();
    let open = |rt: &mut rune::Runtime<crate::app::App>| {
        let (x, y) = at(rt, &format!("marker-label-{id}"));
        right_click_at(rt, x, y);
    };
    open(&mut rt);
    click_settle(&mut rt, "marker-rename-item");
    assert!(has(&rt, "marker-rename"));
    rt.paste("  Hook  ");
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Enter, rune::Mods::default()));
    assert_eq!(markers(&rt)[0].name, "Hook", "trimmed");
    // Empty names are ignored; Escape cancels.
    click_settle(&mut rt, "marker-rename-item");
    rt.paste("   ");
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Enter, rune::Mods::default()));
    assert_eq!(markers(&rt)[0].name, "Hook");
    click_settle(&mut rt, "marker-rename-item");
    rt.paste("Nope");
    frames(&mut rt, 2);
    press(&mut rt, ev(Key::Escape, rune::Mods::default()));
    assert_eq!(markers(&rt)[0].name, "Hook");
    assert!(
        matches!(rt.state.menu, Some(crate::app::Menu::Song(crate::song::SongMenu::Marker { renaming: false, .. }))),
        "Escape leaves the rename, not the menu"
    );
}

#[test]
fn marker_menu_colour_and_delete() {
    let mut rt = song_rt();
    let id = markers(&rt)[0].id.clone();
    let open = |rt: &mut rune::Runtime<crate::app::App>| {
        let (x, y) = at(rt, &format!("marker-label-{id}"));
        right_click_at(rt, x, y);
    };
    // Colour closes the menu.
    open(&mut rt);
    let c = mdaw_model::doc::MARKER_COLORS[3];
    click(&mut rt, "marker-color-3");
    assert_eq!(markers(&rt)[0].color, c);
    assert!(!has(&rt, "marker-menu"));
    // Delete.
    run(&mut rt, 0.8);
    open(&mut rt);
    let n = markers(&rt).len();
    click(&mut rt, "marker-delete");
    assert_eq!(markers(&rt).len(), n - 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Remove Marker"));
}

#[test]
fn marker_keys_and_jump_dropdown() {
    let mut rt = song_rt();
    // ⇧M adds "Drop" (4 markers) at the playhead.
    press(&mut rt, rune::KeyEvent { key: Key::Text("M".into()), mods: shift() });
    assert_eq!(markers(&rt).len(), 5);
    assert!(markers(&rt).iter().any(|m| m.name == "Drop"));
    rt.state.svc.engine.edit(|d| {
        let ids: Vec<String> = d.get_markers().iter().filter(|m| m.name == "Drop").map(|m| m.id.clone()).collect();
        for id in ids {
            d.remove_marker(&id);
        }
    });
    // ] / [ while playing.
    let beats: Vec<f64> = markers(&rt).iter().map(|m| m.beat).collect();
    rt.state.song_play();
    rt.state.svc.engine.set_position(beats[1] + 1.0);
    press(&mut rt, text_key("]"));
    assert!(near(&rt, beats[2]));
    // [ goes to the last marker more than 0.01 beat before the playhead: rolling just past
    // a marker, that's the marker itself (as in the TS).
    press(&mut rt, text_key("["));
    assert!(near(&rt, beats[2]), "{}", rt.state.svc.engine.get_position());
    rt.state.svc.engine.set_position(beats[2] - 1.0);
    press(&mut rt, text_key("["));
    assert!(near(&rt, beats[1]), "{}", rt.state.svc.engine.get_position());
    // The jump dropdown: count, items with bar.beat, picking seeks.
    assert_eq!(texts_of(&rt, "marker-jump").len(), 0);
    let e = rt.elements().into_iter().find(|e| e.key.as_deref() == Some("marker-jump")).unwrap();
    assert_eq!(e.help.as_deref(), Some("Jump to marker ( [ / ] )"));
    click_settle(&mut rt, "marker-jump");
    let m3 = markers(&rt)[3].clone();
    assert!(texts(&rt).contains(&format!(
        "{}.{}",
        (m3.beat / 4.0).floor() as i64 + 1,
        (m3.beat % 4.0).floor() as i64 + 1
    )));
    click(&mut rt, &format!("jump-{}", m3.id));
    assert!(near(&rt, m3.beat));
    rt.state.song_stop();
    // Hidden without markers.
    let ids: Vec<String> = markers(&rt).iter().map(|m| m.id.clone()).collect();
    for id in ids {
        rt.state.remove_marker(&id);
    }
    rt.invalidate();
    frames(&mut rt, 3);
    assert!(!has(&rt, "marker-jump"));
}

#[test]
fn automation_lane_click_adds_right_click_deletes() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.toggle_automation_param(&b, AutomationParameter::Volume);
    rt.invalidate();
    run(&mut rt, 0.4);
    let key = format!("auto-{b}-volume");
    let r = rect(&rt, &key);
    assert_eq!(r[3], 48.0);
    // Lane 64 + 48: the header cell too.
    assert!(texts(&rt).contains(&"↳".to_owned()));
    // Click at beat 4.3 (snapped to 4), 12 points from the top (value 0.75).
    click_at(&mut rt, r[0] + 4.3 * 30.0, r[1] + 12.0);
    let lane = rt.state.find_track(&b).unwrap().automation.clone().unwrap()[0].clone();
    assert_eq!(lane.points.len(), 1);
    assert_eq!(lane.points[0].beat, 4.0);
    assert!((lane.points[0].value - 0.75).abs() < 0.01);
    assert_eq!(undo_label(&rt).as_deref(), Some("Set Automation Point"));
    // The click did not start a rubber band.
    assert!(rt.state.song.band.is_none());
    run(&mut rt, 0.8);
    click_at(&mut rt, r[0] + 10.0 * 30.0, r[1] + 40.0);
    run(&mut rt, 0.5);
    let (x, y) = at(&rt, &format!("{key}-pt-0"));
    right_click_at(&mut rt, x, y);
    let lane = rt.state.find_track(&b).unwrap().automation.clone().unwrap()[0].clone();
    assert_eq!(lane.points.len(), 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Delete Automation Point"));
}

#[test]
fn playhead_is_dimmed_while_stopped_and_red_while_recording() {
    let mut rt = song_rt();
    let op = |rt: &rune::Runtime<crate::app::App>| {
        rt.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("song-playhead"))
            .unwrap()
    };
    assert!((op(&rt).opacity - 0.55).abs() < 0.01);
    rt.state.song_play();
    run(&mut rt, 0.5);
    assert!((op(&rt).opacity - 1.0).abs() < 0.01);
    assert_eq!(op(&rt).fill, crate::theme::t().primary);
    rt.state.song_stop();
    let b = tid(&rt, "Bass");
    rt.state.toggle_arm(&b);
    rt.state.song_record();
    run(&mut rt, 0.5);
    assert_eq!(op(&rt).fill, crate::theme::t().rec);
    rt.state.song_stop();
}

