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
        "Auto-connect last device",
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
    assert!(texts(&rt).iter().any(|x| *x == format!("{} dB", mdaw_model::js::number_to_string(v))));
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

#[test]
fn live_fx_knobs_sections_and_selects_drive_the_live_bus() {
    let mut rt = song_rt();
    let t = texts(&rt);
    for s in ["Live FX", "CUTOFF", "RESO", "REVERB", "CHORUS", "18.0k", "12%"] {
        assert!(
            t.iter().any(|x| x.eq_ignore_ascii_case(s)),
            "{s} missing"
        );
    }
    // Keyboard on the reverb knob: +1 tick (0.01).
    click(&mut rt, "livefx-reverb-2");
    press(&mut rt, ev(Key::Up, Default::default()));
    let p = rt.state.svc.engine.live_effect_params();
    assert!((p.reverb_wet - 0.13).abs() < 1e-9, "{}", p.reverb_wet);
    // Sections: one at a time; the dot lights when active.
    click(&mut rt, "livefx-tab-delay");
    assert_eq!(rt.state.p9.fx_section, Some(crate::views::FxSec::Delay));
    assert!(has(&rt, "livefx-mix-8"));
    click(&mut rt, "livefx-tab-comp");
    assert_eq!(rt.state.p9.fx_section, Some(crate::views::FxSec::Comp));
    assert!(!has(&rt, "livefx-mix-8"));
    click(&mut rt, "livefx-tab-comp");
    assert!(rt.state.p9.fx_section.is_none());
    // Filter type select.
    click(&mut rt, "livefx-filter-type");
    click(&mut rt, "livefx-type-hp");
    assert_eq!(
        rt.state.svc.engine.live_effect_params().filter_type,
        mdaw_model::types::FilterType::Highpass
    );
    // LFO section: wave select.
    click(&mut rt, "livefx-tab-lfo");
    click(&mut rt, "livefx-lfo-wave");
    click(&mut rt, "livefx-wave-saw");
    assert_eq!(
        rt.state.svc.engine.live_effect_params().filter_lfo_wave,
        mdaw_model::types::LfoWaveform::Sawtooth
    );
}

#[test]
fn synth_editor_presets_categories_knobs_and_reset() {
    let mut rt = rt();
    click(&mut rt, "synth-editor-toggle");
    assert!(rt.state.p9.synth_editor);
    let t = texts(&rt);
    assert!(t.iter().any(|x| x == "Sound"));
    assert!(t.iter().any(|x| x.starts_with("Sampled")));
    // A category chip jumps to its first preset.
    click(&mut rt, "synth-cat-bass");
    let idx = rt.state.svc.engine.live().preset_index();
    let bank = mdaw_engine::synth::bank::synth_presets();
    assert_eq!(
        bank[idx].meta().category,
        mdaw_engine::synth::presets::PresetCategory::Bass
    );
    assert!(rt.accessibility_json().to_string().contains("Reset"));
    // A knob edit is a live override; Reset edits clears it.
    let key = "synth-filter.cutoff";
    if has(&rt, key) {
        click(&mut rt, key);
        press(&mut rt, ev(Key::Up, Default::default()));
        assert!(!rt.state.svc.engine.live().synth_overrides().is_empty());
        assert!(texts(&rt).iter().any(|x| x == "Reset edits"));
        click(&mut rt, "synth-reset");
        assert!(rt.state.svc.engine.live().synth_overrides().is_empty());
    }
    // A choice control.
    if has(&rt, "synth-oscA.wave-square") {
        click(&mut rt, "synth-oscA.wave-square");
        let o = rt.state.svc.engine.live().synth_overrides();
        assert_eq!(o["oscA"]["wave"], "square");
    }
    click(&mut rt, "synth-editor-toggle");
    assert!(!rt.state.p9.synth_editor);
}

#[test]
fn instrument_selector_searches_filters_and_picks() {
    let mut rt = rt();
    click(&mut rt, "instrument");
    assert!(has(&rt, "instrument-popover"));
    rt.state.p9.instrument_search = rune::TextEdit::new("zzzzqq");
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(has(&rt, "sound-empty"));
    rt.state.p9.instrument_search = rune::TextEdit::new("rhodes");
    rt.invalidate();
    frames(&mut rt, 2);
    let bank = mdaw_engine::synth::bank::synth_presets();
    let i = bank.iter().position(|p| p.meta().name == "Rhodes EP").unwrap();
    click(&mut rt, &format!("sound-{i}"));
    assert_eq!(rt.state.preset.0, "Rhodes EP");
    assert!(rt.state.menu.is_none());
}
