//! Sample browser: roots, listing, keys, assign (Enter / double-click), row drag onto a pad.

use rune::{Key, Mods};

use super::harness::*;

fn sample_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mdaw-browser-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(d.join("Kicks")).unwrap();
    std::fs::write(d.join("Clap 1.wav"), b"RIFF").unwrap();
    std::fs::write(d.join("clap 10.wav"), b"RIFF").unwrap();
    std::fs::write(d.join("clap 2.WAV"), b"RIFF").unwrap();
    std::fs::write(d.join("notes.txt"), b"x").unwrap();
    std::fs::write(d.join(".hidden.wav"), b"x").unwrap();
    d
}

fn open_browser(r: &mut rune::Runtime<crate::App>) -> std::path::PathBuf {
    let d = sample_dir();
    press(r, text_key("b"));
    r.state.browser_add_root(&d.to_string_lossy());
    r.invalidate();
    run(r, 0.5);
    d
}

#[test]
fn empty_state_without_roots() {
    let mut r = rt();
    press(&mut r, text_key("b"));
    assert!(has(&r, "browser-choose"));
    assert!(texts(&r).iter().any(|t| t == "Add a sample folder"));
}

#[test]
fn lists_folders_first_then_audio_in_natural_order() {
    let mut r = rt();
    open_browser(&mut r);
    let names: Vec<String> = r
        .state
        .browser
        .entries()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(
        names,
        vec!["Kicks", "Clap 1.wav", "clap 2.WAV", "clap 10.wav"]
    );
    assert!(texts(&r).iter().any(|t| t == "4 items"));
}

#[test]
fn keys_move_enter_assigns_to_the_selected_pad_and_left_goes_up() {
    let mut r = rt();
    let d = open_browser(&mut r);
    r.state.select_pad(5);
    r.invalidate();
    click(&mut r, "browser-list");
    press(&mut r, ev(Key::Down, Mods::default()));
    press(&mut r, ev(Key::Down, Mods::default()));
    assert!(
        r.state
            .browser
            .selected
            .as_deref()
            .unwrap()
            .ends_with("Clap 1.wav")
    );
    press(&mut r, ev(Key::Enter, Mods::default()));
    let s = pad(&r, 5).sample.unwrap();
    assert_eq!(s.name, "Clap 1");
    // ↑ to the folder, → opens it, ← goes back up.
    press(&mut r, ev(Key::Up, Mods::default()));
    press(&mut r, ev(Key::Right, Mods::default()));
    assert_eq!(
        r.state.browser.current.as_deref(),
        Some(d.join("Kicks").to_string_lossy().as_ref())
    );
    press(&mut r, ev(Key::Left, Mods::default()));
    assert_eq!(
        r.state.browser.current.as_deref(),
        Some(d.to_string_lossy().as_ref())
    );
    press(&mut r, ev(Key::Left, Mods::default()));
    assert_eq!(
        r.state.browser.current, None,
        "at the root: back to the folder list"
    );
}

#[test]
fn double_click_assigns_and_pad_keys_stay_in_the_browser() {
    let mut r = rt();
    open_browser(&mut r);
    double_click(&mut r, "browser-row-2");
    assert_eq!(pad(&r, 0).sample.unwrap().name, "clap 2");
    // Focus in the browser list: Z is the browser's (no pad hit).
    press(&mut r, text_key("z"));
    assert!(r.state.held_keys.is_empty());
}

#[test]
fn a_row_dragged_onto_a_pad_assigns_it_there() {
    let mut r = rt();
    open_browser(&mut r);
    let (x, y) = at(&r, "browser-row-3");
    let (px, py) = at(&r, "pad-10");
    drag(&mut r, x, y, px - x, py - y);
    assert_eq!(pad(&r, 10).sample.unwrap().name, "clap 10");
    assert_eq!(r.state.selected_pad(), 10);
    assert!(r.state.sample_drag.is_none());
}

#[test]
fn filter_and_width_persist() {
    let mut r = rt();
    open_browser(&mut r);
    r.state.browser.filter = rune::TextEdit::new("10");
    r.invalidate();
    frames(&mut r, 2);
    assert_eq!(r.state.browser.entries().len(), 1);
    let (x, y) = at(&r, "browser-resize");
    drag(&mut r, x, y, 100.0, 0.0);
    let w = mdaw_model::settings::read_browser_width(&r.state.svc.settings.borrow());
    assert!((w - 348.0).abs() < 2.0, "{w}");
}
