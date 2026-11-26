//! Track I/O (Rust-only): arming, an audio track's input, hosted plugins in the Track FX drawer
//! and the instrument popover, and recording an armed audio track into a region.

use mdaw_engine::engine::TransportState;
use mdaw_engine::io::input_pair;
use mdaw_model::InstrumentType;
use mdaw_plugin_host::{PluginId, PluginInfo, PluginKind};

use super::harness::*;
use crate::plugins::PluginManager;

fn catalog() -> Vec<PluginInfo> {
    vec![
        PluginInfo {
            id: PluginId::from_codes("aumu", "Alav", "Artu").unwrap(),
            kind: PluginKind::Instrument,
            name: "Analog Lab V".into(),
            vendor: "Arturia".into(),
            version: 1,
        },
        PluginInfo {
            id: PluginId::from_codes("aufx", "Ni$R", "-NI-").unwrap(),
            kind: PluginKind::Effect,
            name: "Guitar Rig 6 FX".into(),
            vendor: "Native Instruments".into(),
            version: 1,
        },
    ]
}

/// The Song scene with an installed-plugin list (nothing is ever loaded in a headless run).
fn io_rt() -> rune::Runtime<crate::app::App> {
    let mut rt = song_rt();
    *rt.state.svc.plugins.0.borrow_mut() = PluginManager::headless(catalog());
    rt.invalidate();
    frames(&mut rt, 2);
    rt
}

fn add_audio(rt: &mut rune::Runtime<crate::app::App>) -> String {
    click(rt, "add-track");
    click(rt, "add-audio");
    run(rt, 0.3);
    let t = rt.state.tracks().last().unwrap();
    assert_eq!(t.instrument.kind, InstrumentType::Audio);
    t.id.clone()
}

#[test]
fn an_audio_track_picks_its_input_from_the_devices_channels() {
    let mut rt = io_rt();
    let a = add_audio(&mut rt);
    // A four-input interface: four mono inputs and two stereo pairs.
    rt.state.p9.audio_devices.inputs = vec![mdaw_audio_host::InputDeviceInfo {
        name: "MiniFuse 4".into(),
        is_default: true,
        default_sample_rate: 44_100,
        channels: 4,
    }];
    click_settle(&mut rt, &format!("input-{a}"));
    let t = texts(&rt);
    for label in ["MiniFuse 4", "In 1", "In 4", "In 1/2", "In 3/4"] {
        assert!(t.iter().any(|x| x == label), "{label} missing in {t:?}");
    }
    assert!(!t.iter().any(|x| x == "In 5"));
    click(&mut rt, "input-opt-2-st");
    let input = rt.state.find_track(&a).unwrap().input.unwrap();
    assert_eq!((input.channel, input.stereo), (2, true));
    assert!(rt.state.menu.is_none());
    frames(&mut rt, 2);
    assert!(texts(&rt).contains(&"In 3/4".to_owned()));
    // The stereo input makes the track's strip stereo; the choice is not an undo step.
    assert!(mdaw_engine::engine::track_is_stereo(
        rt.state.find_track(&a).unwrap()
    ));
    assert_ne!(undo_label(&rt).as_deref(), Some("Set Input"));
}

#[test]
fn the_fx_drawer_lists_adds_bypasses_and_removes_plugins() {
    let mut rt = io_rt();
    let a = add_audio(&mut rt);
    click_settle(&mut rt, &format!("fx-{a}"));
    assert!(has(&rt, &format!("plugin-add-{a}")) && has(&rt, "fx-add"));
    assert!(
        has(&rt, "plugins-hint"),
        "an audio track explains what plugins are for"
    );
    click_settle(&mut rt, &format!("plugin-add-{a}"));
    let t = texts(&rt);
    assert!(t.contains(&"Guitar Rig 6 FX".to_owned()), "{t:?}");
    assert!(
        !t.contains(&"Analog Lab V".to_owned()),
        "instruments are not insert effects"
    );
    click(&mut rt, "plugin-choice-0");
    frames(&mut rt, 2);
    let p = rt.state.find_track(&a).unwrap().plugins[0].clone();
    assert_eq!(
        (
            p.name.as_str(),
            p.kind.as_str(),
            p.subtype.as_str(),
            p.manufacturer.as_str()
        ),
        ("Guitar Rig 6 FX", "aufx", "Ni$R", "-NI-")
    );
    assert!(!p.id.is_empty());
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Plugin"));
    assert!(has(&rt, &format!("plugin-open-{}", p.id)));
    assert!(!has(&rt, "plugins-hint"));
    // The plugin is a slot of the chain: an effect added after it can be moved before it.
    assert!(has(&rt, &format!("fx-plugin-{}", p.id)));
    click_settle(&mut rt, "fx-add");
    click_settle(&mut rt, "fx-add-eq");
    run(&mut rt, 0.5);
    let y = |rt: &rune::Runtime<crate::app::App>, k: &str| rect(rt, k)[1];
    assert!(y(&rt, &format!("fx-plugin-{}", p.id)) < y(&rt, "fx-section-eq"));
    // (The handle's arrow keys, for the keyboard.)
    rt.update(|_, tx| tx.focus("fx-grip-eq", true));
    frames(&mut rt, 2);
    press(&mut rt, ev(rune::Key::Up, Default::default()));
    run(&mut rt, 0.5);
    assert!(y(&rt, "fx-section-eq") < y(&rt, &format!("fx-plugin-{}", p.id)));
    assert_eq!(undo_label(&rt).as_deref(), Some("Reorder Effects"));
    rt.state.undo();
    rt.state.undo();
    rt.invalidate();
    run(&mut rt, 0.6);
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Plugin"));

    click(&mut rt, &format!("plugin-bypass-{}", p.id));
    assert!(rt.state.find_track(&a).unwrap().plugins[0].bypassed);
    assert_eq!(
        undo_label(&rt).as_deref(),
        Some("Add Plugin"),
        "no undo step"
    );
    click(&mut rt, &format!("plugin-bypass-{}", p.id));
    assert!(!rt.state.find_track(&a).unwrap().plugins[0].bypassed);

    click(&mut rt, &format!("plugin-remove-{}", p.id));
    assert!(rt.state.find_track(&a).unwrap().plugins.is_empty());
    rt.state.undo();
    assert_eq!(rt.state.find_track(&a).unwrap().plugins.len(), 1);

    // A project from another computer: a plugin that is not installed here says so.
    rt.state.svc.engine.edit(|d| {
        d.add_track_plugin(
            &a,
            mdaw_model::PluginSlot {
                id: "elsewhere".into(),
                format: "au".into(),
                kind: "aufx".into(),
                subtype: "Xyz1".into(),
                manufacturer: "Acme".into(),
                name: "Space Echo".into(),
                vendor: "Acme".into(),
                bypassed: false,
                state: None,
                extra: Default::default(),
            },
        )
    });
    rt.invalidate();
    frames(&mut rt, 2);
    let t = texts(&rt);
    assert!(t.contains(&"Space Echo".to_owned()));
    assert!(t.contains(&"Not installed".to_owned()));
}

#[test]
fn the_instrument_popover_offers_plugin_instruments() {
    let mut rt = io_rt();
    let l = tid(&rt, "Lead");
    let before = rt.state.find_track(&l).unwrap().instrument.preset_index;
    click_settle(&mut rt, &format!("instrument-{l}"));
    let t = texts(&rt);
    assert!(t.iter().any(|x| x.eq_ignore_ascii_case("plugins")), "{t:?}");
    assert!(t.contains(&"Analog Lab V".to_owned()));
    assert!(!t.contains(&"Guitar Rig 6 FX".to_owned()));
    click(&mut rt, "instrument-plugin-0");
    frames(&mut rt, 2);
    let track = rt.state.find_track(&l).unwrap().clone();
    let p = track
        .instrument
        .plugin
        .clone()
        .expect("the plugin instrument");
    assert_eq!((p.name.as_str(), p.kind.as_str()), ("Analog Lab V", "aumu"));
    assert_eq!(track.name, "Analog Lab V");
    assert_eq!(
        track.instrument.preset_index, before,
        "the preset is kept underneath"
    );
    assert!(has(&rt, &format!("plugin-window-{l}")));
    assert!(texts(&rt).contains(&"Analog Lab V".to_owned()));
    // Choosing a built-in sound removes the plugin again.
    let chill = mdaw_engine::synth::bank::synth_presets()
        .iter()
        .position(|p| p.meta().vibe == mdaw_engine::synth::presets::PresetVibe::Chill)
        .unwrap();
    click_settle(&mut rt, &format!("instrument-{l}"));
    run(&mut rt, 0.3);
    click(&mut rt, &format!("instrument-preset-{chill}"));
    frames(&mut rt, 2);
    let track = rt.state.find_track(&l).unwrap();
    assert!(track.instrument.plugin.is_none());
    assert_eq!(track.instrument.preset_index, chill as i64);
    assert!(!has(&rt, &format!("plugin-window-{l}")));
}

#[test]
fn recording_an_armed_audio_track_adds_its_take_as_a_region() {
    let mut rt = io_rt();
    let a = add_audio(&mut rt);
    click(&mut rt, &format!("arm-{a}"));
    assert!(rt.state.svc.engine.doc().is_track_armed(&a));
    // A device input: one channel of a constant level (two blocks, so the queue is primed).
    let rate = rt.state.svc.engine.sample_rate();
    let (mut feed, source) = input_pair(1, 256, rate);
    rt.state.svc.engine.set_input_source(Some(source));
    let block = vec![0.25f32; rate as usize];
    feed.push_interleaved(&block);
    feed.push_interleaved(&block);
    let regions = rt.state.find_track(&a).unwrap().regions.len();
    // The transport's Record button (it was disabled with only an audio track armed).
    click(&mut rt, "song-record");
    assert_eq!(rt.state.song_state(), TransportState::Recording);
    run(&mut rt, 0.5);
    assert!(
        has(&rt, &format!("live-take-{a}")),
        "the take shows while it records"
    );
    click(&mut rt, "song-record");
    run(&mut rt, 0.1);
    assert_eq!(rt.state.song_state(), TransportState::Stopped);
    let track = rt.state.find_track(&a).unwrap().clone();
    assert_eq!(track.regions.len(), regions + 1);
    let r = track.regions.last().unwrap();
    let audio = r.audio.as_ref().expect("an audio region");
    assert_eq!(r.start_beat, 0.0);
    let secs = r.length_beats * 60.0 / rt.state.svc.engine.doc().get_bpm();
    assert!(
        (secs - 0.5).abs() < 0.1,
        "about the recorded half second ({secs})"
    );
    let buf = rt
        .state
        .p9
        .samples
        .buffer(&audio.sample_id)
        .expect("the take's audio");
    assert_eq!(buf.channels.len(), 1);
    assert!(buf.channels[0].iter().all(|v| (*v - 0.25).abs() < 1e-6));
    assert!(r.name.as_deref().is_some_and(|n| n.contains("take 1")));
    assert!(!has(&rt, &format!("live-take-{a}")));
    // The take is one undo step.
    assert_eq!(undo_label(&rt).as_deref(), Some("Add Audio Region"));
}

#[test]
fn the_topbar_has_no_midi_device_select() {
    let rt = io_rt();
    assert!(!has(&rt, "midi-select"));
    assert!(has(&rt, "controller-setup"));
}
