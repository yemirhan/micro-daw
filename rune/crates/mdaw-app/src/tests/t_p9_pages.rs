//! `inventory/views.md` §12 Settings, §15 the MIDI monitor, §9 Live FX / SynthEditor /
//! InstrumentSelector.

use rune::Key;

use super::harness::*;
use crate::app::AppMode;
use mdaw_midi::monitor::MonitorFilter;

#[test]
fn settings_rows_write_the_settings_key() {
    let mut rt = rt();
    rt.state.switch_mode(AppMode::Settings);
    rt.invalidate();
    frames(&mut rt, 2);
    let t = texts(&rt);
    for s in [
        "Settings",
        "Preferences are saved on this computer.",
        "Check for updates automatically",
        "Looks for a new version when the app starts.",
        "Default master volume",
        "-12 dB",
        "Buffer size",
        "256 samples",
        "Output device",
        "Input device",
        "System default",
        "Opened while an audio track is armed.",
        "MIDI inputs",
        "Remember the pad controller",
        "Yusuf Emirhan Sahin",
        "GitHub",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    click(&mut rt, "auto-check-updates");
    click(&mut rt, "auto-connect-midi");
    let st = rt.state.p9.settings.clone();
    assert!(!st.general.auto_check_updates);
    assert!(!st.midi.auto_connect_last_device);
    let raw = rt
        .state
        .svc
        .settings
        .borrow()
        .get_item(mdaw_model::settings::SETTINGS_STORAGE_KEY)
        .unwrap()
        .to_owned();
    let v: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(v["general"]["autoCheckUpdates"], false);
    assert_eq!(v["midi"]["autoConnectLastDevice"], false);
    assert_eq!(v["audio"]["bufferSizeHint"], 256);
    // The buffer size list and the volume slider (keyboard: ← −1 dB).
    click(&mut rt, "buffer-size");
    click(&mut rt, "buffer-1024");
    assert_eq!(rt.state.p9.settings.audio.buffer_size_hint as i64, 1024);
    click(&mut rt, "default-master-volume");
    press(&mut rt, ev(Key::Left, Default::default()));
    let v = rt.state.p9.settings.audio.default_master_volume;
    assert!((-40.0..=0.0).contains(&v));
    assert!(
        texts(&rt)
            .iter()
            .any(|x| *x == format!("{} dB", mdaw_model::js::number_to_string(v)))
    );
}

fn feed(rt: &mut rune::Runtime<crate::App>, bytes: &[u8], t: f64) {
    rt.state.svc.midi.host.monitor().on_raw(bytes, t);
}

#[test]
fn midi_monitor_lists_filters_pauses_and_clears() {
    let mut rt = rt();
    rt.state.switch_mode(AppMode::Dev);
    frames(&mut rt, 2);
    assert!(texts(&rt).iter().any(|x| x == "No MIDI device connected"));
    feed(&mut rt, &[0x90, 60, 100], 1234.5);
    feed(&mut rt, &[0xF8], 1235.0);
    feed(&mut rt, &[0xB0, 74, 64], 1300.0);
    frames(&mut rt, 2);
    let t = texts(&rt);
    assert!(t.iter().any(|x| x == "1.234"), "{t:?}");
    assert!(t.iter().any(|x| x == "C4 (60) vel=100"));
    assert!(t.iter().any(|x| x == "Cutoff = 64"));
    assert!(t.iter().any(|x| x == "90 3C 64"));
    assert_eq!(rt.state.p9.monitor.events().len(), 2, "clock dropped");
    // Filter.
    click(&mut rt, "dev-filter");
    click(&mut rt, "dev-filter-control-change");
    assert_eq!(rt.state.p9.monitor.filter, MonitorFilter::ControlChange);
    assert_eq!(rt.state.p9.monitor.events().len(), 1);
    assert!(texts(&rt).iter().any(|x| x == " events (filtered)"));
    // Pause drops input; resume; clear.
    click(&mut rt, "dev-pause");
    assert!(rt.state.p9.monitor.paused);
    assert!(texts(&rt).iter().any(|x| x == "Capture paused"));
    feed(&mut rt, &[0xB0, 1, 2], 1400.0);
    frames(&mut rt, 2);
    rt.state.monitor_set_filter(MonitorFilter::All);
    assert_eq!(rt.state.p9.monitor.events().len(), 2);
    click(&mut rt, "dev-pause");
    click(&mut rt, "dev-clear");
    assert!(rt.state.p9.monitor.events().is_empty());
    // Leaving the page stops the feed.
    rt.state.switch_mode(AppMode::Daw);
    frames(&mut rt, 2);
    assert!(!rt.state.p9_busy());
}
