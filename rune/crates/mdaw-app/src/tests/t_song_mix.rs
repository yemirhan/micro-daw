//! `inventory/views.md` §5 TrackEffectsPanel, §8 Mixer, and `beat-ui.md` §14 TransportBar.

use rune::Key;

use super::harness::*;
use super::t_song_arr::texts_of;
use mdaw_engine::engine::TransportState;

fn fx(rt: &rune::Runtime<crate::app::App>, id: &str) -> mdaw_model::TrackEffectState {
    rt.state
        .find_track(id)
        .unwrap()
        .effects
        .clone()
        .unwrap_or_default()
}

#[test]
fn fx_drawer_header_sections_and_close() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.set_track_effect(
        &b,
        mdaw_model::doc::EffectName::Reverb,
        serde_json::json!({"enabled": true})
            .as_object()
            .unwrap()
            .clone(),
    );
    click_settle(&mut rt, &format!("fx-{b}"));
    let t = texts(&rt);
    assert!(t.contains(&"Track FX".to_owned()));
    assert_eq!(texts_of(&rt, "fx-track-name"), "Bass");
    // The chain shows the effects that are on (expanded), not the ones that are off.
    assert!(has(&rt, "fx-body-reverb"));
    assert!(!has(&rt, "fx-section-filter"));
    assert!(has(&rt, "fx-chain-in") && has(&rt, "fx-chain-out"));
    // "Add effect" lists the ones not in the chain; a pick turns it on with its knobs showing.
    click_settle(&mut rt, "fx-add");
    assert!(has(&rt, "fx-add-delay") && !has(&rt, "fx-add-reverb"));
    click_settle(&mut rt, "fx-add-delay");
    assert!(fx(&rt, &b).delay.enabled);
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Effect"));
    assert!(has(&rt, "fx-body-delay"));
    // The chevron collapses; an added effect goes to the end of the chain.
    click(&mut rt, "fx-expand-delay");
    assert!(!has(&rt, "fx-body-delay"));
    click_settle(&mut rt, "fx-add");
    click_settle(&mut rt, "fx-add-eq");
    click_settle(&mut rt, "fx-add");
    click_settle(&mut rt, "fx-add-distortion");
    run(&mut rt, 0.5);
    let order = |rt: &rune::Runtime<crate::app::App>| -> Vec<&'static str> {
        let mut v: Vec<(f32, &'static str)> = ["reverb", "delay", "eq", "distortion"]
            .into_iter()
            .filter(|k| has(rt, &format!("fx-section-{k}")))
            .map(|k| (rect(rt, &format!("fx-section-{k}"))[1], k))
            .collect();
        v.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        v.into_iter().map(|x| x.1).collect()
    };
    assert_eq!(order(&rt), ["reverb", "delay", "eq", "distortion"]);
    // A slot's handle drags it past its neighbour (one undo step, taken on release); the
    // project keeps the order.
    let depth = undo_depth(&rt);
    let (x, y) = at(&rt, "fx-grip-distortion");
    let travel = rect(&rt, "fx-section-eq")[3] * 0.7;
    drag(&mut rt, x, y, 0.0, -travel);
    run(&mut rt, 0.5);
    assert_eq!(order(&rt), ["reverb", "delay", "distortion", "eq"]);
    assert_eq!(undo_label(&rt).as_deref(), Some("Reorder Effects"));
    assert_eq!(undo_depth(&rt), depth + 1);
    assert!(rt.state.song.fx_drag.is_none());
    // A short drag moves nothing.
    let (x, y) = at(&rt, "fx-grip-reverb");
    drag(&mut rt, x, y, 0.0, 6.0);
    assert_eq!(undo_depth(&rt), depth + 1);
    let travel = (rect(&rt, "fx-section-delay")[3] + 12.0) * 0.7;
    drag(&mut rt, x, y, 0.0, travel);
    run(&mut rt, 0.5);
    assert_eq!(order(&rt), ["delay", "reverb", "distortion", "eq"]);
    let saved = rt.state.find_track(&b).unwrap().fx_order.clone();
    assert_eq!(saved[..4], ["delay", "reverb", "distortion", "eq"]);
    rt.state.undo();
    rt.invalidate();
    run(&mut rt, 0.5);
    assert_eq!(order(&rt), ["reverb", "delay", "distortion", "eq"]);
    // The power button takes an effect out of the chain (its settings stay in the project).
    click_settle(&mut rt, "fx-switch-delay");
    assert!(!fx(&rt, &b).delay.enabled);
    assert!(!has(&rt, "fx-section-delay"));
    // Reverb: the starting wet is 0.25 though the knob's double-click default is 0.3 (TS quirk).
    assert_eq!(fx(&rt, &b).reverb.wet, 0.25);
    run(&mut rt, 0.5);
    double_click(&mut rt, "fx-knob-reverb-wet");
    assert_eq!(fx(&rt, &b).reverb.wet, 0.3);
    click(&mut rt, "fx-close");
    assert_eq!(rt.state.song.fx_track, None);
}

#[test]
fn fx_knob_drag_is_one_undo_step() {
    // Electron bug fixed in both apps (user decision 2026-11-18): the TS pushed one undo
    // entry per knob move.
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.toggle_fx(&b);
    rt.state.add_fx(crate::song::FxSection::Eq);
    rt.invalidate();
    run(&mut rt, 0.5);
    let depth = undo_depth(&rt);
    let (x, y) = at(&rt, "fx-knob-eq-low");
    // 200 points = the full range (24 dB): 50 points up ≈ +6 dB.
    drag(&mut rt, x, y, 0.0, -50.0);
    let low = fx(&rt, &b).eq.low;
    assert!((low - 6.0).abs() < 0.6, "{low}");
    assert_eq!(undo_depth(&rt), depth + 1);
    assert_eq!(undo_label(&rt).as_deref(), Some("Change Track Effect"));
    rt.state.undo();
    assert_eq!(fx(&rt, &b).eq.low, 0.0);
}

#[test]
fn fx_selects_filter_type_and_lfo_wave() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.toggle_fx(&b);
    rt.state.add_fx(crate::song::FxSection::Filter);
    rt.invalidate();
    run(&mut rt, 0.5);
    click_settle(&mut rt, "fx-filter-type");
    for k in ["lowpass", "highpass", "bandpass", "notch", "allpass"] {
        assert!(has(&rt, &format!("filter-type-menu-{k}")));
    }
    click(&mut rt, "filter-type-menu-notch");
    assert_eq!(fx(&rt, &b).filter.kind, mdaw_model::FilterType::Notch);
    run(&mut rt, 0.5);
    click_settle(&mut rt, "fx-lfo-wave");
    click(&mut rt, "lfo-wave-menu-sawtooth");
    assert_eq!(
        fx(&rt, &b).filter.lfo_wave,
        mdaw_model::LfoWaveform::Sawtooth
    );
    // Using a select keeps the filter in the chain, drawn at full strength.
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some("fx-body-filter"))
        .unwrap();
    assert!((e.opacity - 1.0).abs() < 0.02, "{}", e.opacity);
}

#[test]
fn fx_param_table_matches_the_ts() {
    use crate::song::FX_PARAMS;
    let get = |sec: &str, key: &str| {
        FX_PARAMS
            .iter()
            .find(|p| p.section.key() == sec && p.key == key)
            .map(|p| (p.min, p.max, p.step, p.default))
            .unwrap()
    };
    assert_eq!(get("filter", "cutoff"), (60.0, 18000.0, 1.0, 18000.0));
    assert_eq!(get("filter", "resonance"), (0.0, 20.0, 0.1, 1.0));
    assert_eq!(get("filter", "lfoRate"), (0.1, 20.0, 0.1, 1.0));
    assert_eq!(get("filter", "lfoDepth"), (0.0, 1.0, 0.01, 0.0));
    assert_eq!(get("eq", "mid"), (-12.0, 12.0, 0.5, 0.0));
    assert_eq!(get("compressor", "threshold"), (-60.0, 0.0, 1.0, -24.0));
    assert_eq!(get("compressor", "ratio"), (1.0, 20.0, 0.5, 4.0));
    assert_eq!(get("compressor", "attack"), (0.001, 1.0, 0.001, 0.003));
    assert_eq!(get("compressor", "release"), (0.01, 1.0, 0.01, 0.25));
    assert_eq!(get("chorus", "depth"), (0.0, 1.0, 0.01, 0.5));
    assert_eq!(get("delay", "time"), (0.01, 1.0, 0.01, 0.25));
    assert_eq!(get("delay", "feedback"), (0.0, 0.95, 0.01, 0.3));
    assert_eq!(get("delay", "wet"), (0.0, 1.0, 0.01, 0.3));
    assert_eq!(get("distortion", "amount"), (0.0, 1.0, 0.01, 0.3));
    assert_eq!(get("distortion", "wet"), (0.0, 1.0, 0.01, 0.5));
    assert_eq!(get("reverb", "wet"), (0.0, 1.0, 0.01, 0.3));
}

#[test]
fn mixer_toggles_with_x_and_shows_strips() {
    let mut rt = song_rt();
    press(&mut rt, text_key("x"));
    assert!(rt.state.song.show_mixer);
    let t = texts(&rt);
    assert!(t.contains(&"Mixer".to_owned()) && t.contains(&"Press X to hide".to_owned()));
    let b = tid(&rt, "Bass");
    assert_eq!(rect(&rt, &format!("strip-{b}"))[2], 92.0);
    assert_eq!(texts_of(&rt, &format!("strip-db-{b}")), "-6 dB");
    assert_eq!(texts_of(&rt, "master-db"), "-∞ dB");
    press(&mut rt, text_key("x"));
    assert!(!has(&rt, "mixer"));
}

#[test]
fn mixer_empty_state() {
    let mut rt = song_rt();
    let ids: Vec<String> = rt.state.tracks().iter().map(|t| t.id.clone()).collect();
    for id in ids {
        rt.state.remove_song_track(&id);
    }
    rt.state.song.show_mixer = true;
    rt.invalidate();
    frames(&mut rt, 3);
    assert!(has(&rt, "mixer-empty"));
    assert!(!has(&rt, "master-strip"));
}

#[test]
fn fader_click_jumps_grab_keeps_offset_keys_and_double_click() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.song.show_mixer = true;
    rt.invalidate();
    run(&mut rt, 0.5);
    let k = format!("fader-{b}");
    let r = rect(&rt, &k);
    // Click near the bottom off the cap: jumps there (−40 dB at the bottom).
    click_at(&mut rt, r[0] + 14.0, r[1] + r[3] - 4.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -40.0);
    // Grab the cap and move up 41 points (82 points of travel = 40 dB → +20 dB).
    run(&mut rt, 0.8);
    let cap = rect(&rt, &format!("{k}-cap"));
    drag(&mut rt, cap[0] + 14.0, cap[1] + 11.0, 0.0, -41.0);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -20.0);
    // Shift is 4× finer.
    let cap = rect(&rt, &format!("{k}-cap"));
    drag_mods(&mut rt, cap[0] + 14.0, cap[1] + 11.0, 0.0, -41.0, shift());
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -15.0);
    // Keys: ±1, ±6 with Shift (focused by the click).
    press(&mut rt, ev(Key::Up, rune::Mods::default()));
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -14.0);
    press(&mut rt, ev(Key::Down, shift()));
    assert_eq!(rt.state.find_track(&b).unwrap().volume, -20.0);
    run(&mut rt, 0.8);
    double_click(&mut rt, &k);
    assert_eq!(rt.state.find_track(&b).unwrap().volume, 0.0);
}

#[test]
fn mixer_pan_knob_mute_solo_and_meters() {
    let mut rt = song_rt();
    let b = tid(&rt, "Bass");
    rt.state.song.show_mixer = true;
    rt.invalidate();
    run(&mut rt, 0.5);
    let (x, y) = at(&rt, &format!("mixer-pan-{b}"));
    drag(&mut rt, x, y, 0.0, -50.0);
    assert!((rt.state.find_track(&b).unwrap().pan - 0.5).abs() < 0.011);
    assert!(
        texts(&rt).contains(&"R50".to_owned())
            || texts(&rt).contains(&"R51".to_owned())
            || texts(&rt).contains(&"R49".to_owned())
    );
    click(&mut rt, &format!("strip-mute-{b}"));
    assert!(rt.state.find_track(&b).unwrap().muted);
    run(&mut rt, 0.6);
    let e = rt
        .elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(format!("strip-{b}").as_str()))
        .unwrap();
    assert!((e.opacity - 0.7).abs() < 0.02);
    click(&mut rt, &format!("strip-solo-{b}"));
    assert!(rt.state.find_track(&b).unwrap().solo);
    // Playing: levels are read every frame, the meters move and the master readout shows dB.
    rt.state.toggle_track_mute(&b);
    rt.state.song_play();
    run(&mut rt, 1.5);
    let lv = rt
        .state
        .song
        .live
        .levels
        .borrow()
        .get("")
        .copied()
        .unwrap_or(f64::NEG_INFINITY);
    assert!(lv.is_finite(), "master level {lv}");
    assert!(texts_of(&rt, "master-db").ends_with(" dB"));
    rt.state.song_stop();
    // Meter scale, floor and thresholds.
    use crate::ui::song::mixer::{meter_norm, meter_offset};
    assert_eq!(meter_norm(f64::NEG_INFINITY), 0.0);
    assert_eq!(meter_norm(-60.0), 0.0);
    assert_eq!(meter_norm(0.0), 1.0);
    assert!((meter_offset(-12.0, true) - 0.2 * 104.0).abs() < 1e-3);
}

#[test]
fn transport_buttons_states_and_tooltips() {
    let mut rt = song_rt();
    // The tooltip text (Rune's help string carries the shortcut keycap after a space).
    let help = |rt: &rune::Runtime<crate::app::App>, k: &str| {
        let h = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(k))
            .unwrap()
            .help;
        h.map(|h| {
            for keycap in [" Space", " R", " L", " M"] {
                if let Some(x) = h.strip_suffix(keycap) {
                    return x.to_owned();
                }
            }
            h
        })
    };
    assert_eq!(help(&rt, "song-play").as_deref(), Some("Play"));
    assert_eq!(
        help(&rt, "song-record").as_deref(),
        Some("Arm a track to record")
    );
    assert_eq!(help(&rt, "song-loop").as_deref(), Some("Loop off"));
    assert_eq!(
        help(&rt, "song-metronome").as_deref(),
        Some("Metronome off")
    );
    // Record is disabled without an armed track; stop is disabled while stopped.
    click(&mut rt, "song-record");
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
    click(&mut rt, "song-play");
    assert_eq!(rt.state.song_state(), TransportState::Playing);
    assert_eq!(help(&rt, "song-play").as_deref(), Some("Playing"));
    click(&mut rt, "song-stop");
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
    // Loop + metronome.
    click(&mut rt, "song-loop");
    assert!(rt.state.svc.engine.doc().is_loop_enabled());
    assert_eq!(help(&rt, "song-loop").as_deref(), Some("Loop on"));
    click(&mut rt, "song-metronome");
    assert!(rt.state.svc.engine.is_metronome_enabled());
    assert_eq!(help(&rt, "song-metronome").as_deref(), Some("Metronome on"));
    // Armed + loop on: "Punch record (loop)"; recording shows the PUNCH badge.
    let b = tid(&rt, "Bass");
    rt.state.toggle_arm(&b);
    rt.invalidate();
    frames(&mut rt, 2);
    assert_eq!(
        help(&rt, "song-record").as_deref(),
        Some("Punch record (loop)")
    );
    click(&mut rt, "song-record");
    run(&mut rt, 0.3);
    assert_eq!(rt.state.song_state(), TransportState::Recording);
    assert!(has(&rt, "song-rec-badge"));
    assert_eq!(help(&rt, "song-record").as_deref(), Some("Stop recording"));
    // Play is disabled while recording; stop stops the recording.
    click(&mut rt, "song-play");
    assert_eq!(rt.state.song_state(), TransportState::Recording);
    click(&mut rt, "song-stop");
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
    // Unlooped armed: "Record".
    click(&mut rt, "song-loop");
    assert_eq!(help(&rt, "song-record").as_deref(), Some("Record"));
}

#[test]
fn song_keys_space_r_m_l() {
    let mut rt = song_rt();
    press(&mut rt, text_key(" "));
    assert_eq!(rt.state.song_state(), TransportState::Playing);
    press(&mut rt, text_key(" "));
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
    press(&mut rt, text_key("m"));
    assert!(rt.state.svc.engine.is_metronome_enabled());
    press(&mut rt, text_key("l"));
    assert!(rt.state.svc.engine.doc().is_loop_enabled());
    // R: nothing armed → nothing; armed → records; R again stops.
    let b = tid(&rt, "Bass");
    rt.state.toggle_arm(&b);
    press(&mut rt, text_key("r"));
    assert_eq!(rt.state.song_state(), TransportState::Recording);
    press(&mut rt, text_key("r"));
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
}

#[test]
fn lcd_formats_and_time_width() {
    use crate::song::{format_bbs, format_time};
    assert_eq!(format_bbs(0.0), ("  1".into(), "1".into(), "1".into()));
    assert_eq!(format_bbs(13.75), ("  4".into(), "2".into(), "4".into()));
    assert_eq!(format_bbs(-3.0), ("  1".into(), "1".into(), "1".into()));
    assert_eq!(format_bbs(4000.0).0, "1001");
    assert_eq!(format_time(0.0, 120.0), "0:00.0");
    assert_eq!(format_time(150.0, 120.0), "1:15.0");
    assert_eq!(format_time(19.0, 120.0), "0:09.5");
    let mut rt = song_rt();
    assert!(has(&rt, "song-lcd-time"), "time shows from 1440 points");
    rt.state.song_play();
    run(&mut rt, 1.0);
    rt.state.song_stop();
    let _ = &mut rt;
}

#[test]
fn bpm_field_wheel_drag_and_type() {
    let mut rt = song_rt();
    let (x, y) = at(&rt, "song-bpm");
    wheel(&mut rt, x, y, 10.0);
    assert_eq!(rt.state.svc.engine.doc().get_bpm(), 121.0);
    drag(&mut rt, x, y, 0.0, -20.0);
    assert_eq!(rt.state.svc.engine.doc().get_bpm(), 131.0, "dy / 2");
    run(&mut rt, 0.8);
    click(&mut rt, "song-bpm");
    assert!(has(&rt, "bpm-input"));
    rt.paste("300");
    press(&mut rt, ev(Key::Enter, rune::Mods::default()));
    assert_eq!(rt.state.svc.engine.doc().get_bpm(), 240.0, "clamped");
}
