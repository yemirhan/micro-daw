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
    assert!(r.state.browser.settle(std::time::Duration::from_secs(5)));
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

/// Lets the listing worker finish and the view show it.
fn settle(r: &mut rune::Runtime<crate::App>) {
    assert!(r.state.browser.settle(std::time::Duration::from_secs(5)));
    r.invalidate();
    frames(r, 2);
}

#[test]
fn keys_move_enter_assigns_to_the_selected_pad_and_left_goes_up() {
    let mut r = rt();
    let d = open_browser(&mut r);
    r.state.select_pad(5);
    r.invalidate();
    // A listing selects its first row (`selectAfter ?? entries[0]`).
    assert!(
        r.state
            .browser
            .selected
            .as_deref()
            .unwrap()
            .ends_with("Kicks")
    );
    click(&mut r, "browser-list");
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
    // ↑ to the folder, → opens it, ← goes back up and re-selects the folder just left.
    press(&mut r, ev(Key::Up, Mods::default()));
    press(&mut r, ev(Key::Right, Mods::default()));
    settle(&mut r);
    let kicks = d.join("Kicks").to_string_lossy().into_owned();
    assert_eq!(r.state.browser.current.as_deref(), Some(kicks.as_str()));
    press(&mut r, ev(Key::Left, Mods::default()));
    settle(&mut r);
    assert_eq!(
        r.state.browser.current.as_deref(),
        Some(d.to_string_lossy().as_ref())
    );
    assert_eq!(r.state.browser.selected.as_deref(), Some(kicks.as_str()));
    press(&mut r, ev(Key::Left, Mods::default()));
    settle(&mut r);
    assert_eq!(
        r.state.browser.current, None,
        "at the root: back to the folder list"
    );
    assert_eq!(
        r.state.browser.selected.as_deref(),
        Some(d.to_string_lossy().as_ref()),
        "the root row is selected"
    );
}

#[test]
fn each_filter_term_matches_on_its_own_and_enter_takes_the_first_match() {
    let mut r = rt();
    let d = open_browser(&mut r);
    std::fs::write(d.join("Kick 1.wav"), b"RIFF").unwrap();
    std::fs::write(d.join("Snare 1.wav"), b"RIFF").unwrap();
    r.state.browser.load(None);
    settle(&mut r);
    let names = |r: &rune::Runtime<crate::App>| -> Vec<String> {
        r.state
            .browser
            .entries()
            .into_iter()
            .map(|e| e.name)
            .collect()
    };
    r.state.select_pad(7);
    click(&mut r, "browser-filter");
    let type_text = |r: &mut rune::Runtime<crate::App>, t: &str| {
        for c in t.chars() {
            r.key(text_key(&c.to_string()));
        }
        frames(r, 2);
    };
    type_text(&mut r, "1 kick");
    assert_eq!(r.state.browser.filter.text, "1 kick");
    assert_eq!(names(&r), vec!["Kick 1.wav"], "terms in any order");
    press(&mut r, ev(Key::Escape, Mods::default())); // clears the filter, keeps focus
    assert_eq!(r.state.browser.filter.text, "");
    type_text(&mut r, "  CLAP   1 ");
    assert_eq!(names(&r), vec!["Clap 1.wav", "clap 10.wav"]);
    // The selected row (the Kicks folder) is filtered out: the first match acts for Enter.
    let entries = r.state.browser.entries();
    assert_eq!(r.state.browser.selected_index(&entries), Some(0));
    assert!(texts(&r).iter().any(|t| t == "2/6 items"));
    assert_eq!(r.state.focus_seen.as_deref(), Some("browser-filter"));
    press(&mut r, ev(Key::Enter, Mods::default()));
    assert_eq!(pad(&r, 7).sample.unwrap().name, "Clap 1");
    // No filter and nothing selected: no row acts.
    r.state.browser.filter = rune::TextEdit::new("");
    r.state.browser.selected = None;
    let entries = r.state.browser.entries();
    assert_eq!(r.state.browser.selected_index(&entries), None);
}

#[test]
fn listings_run_off_the_ui_thread_and_stale_ones_are_dropped() {
    let mut r = rt();
    let d = open_browser(&mut r);
    let kicks = d.join("Kicks");
    std::fs::write(kicks.join("BD.wav"), b"RIFF").unwrap();
    // Two navigations before either listing arrived: only the second one shows.
    r.state
        .browser_navigate(Some(kicks.to_string_lossy().into_owned()));
    assert!(
        r.state.browser.loading,
        "the folder lists in the background"
    );
    r.state
        .browser_navigate(Some(d.to_string_lossy().into_owned()));
    let g = r.state.browser.generation;
    std::thread::sleep(std::time::Duration::from_millis(50));
    settle(&mut r);
    assert_eq!(r.state.browser.generation, g);
    assert_eq!(
        r.state.browser.total(),
        4,
        "the parent's listing, not Kicks'"
    );
    // The restored folder of a new session lists in the background as well.
    let mut st = mdaw_model::settings::SettingsStore::in_memory();
    let root = d.to_string_lossy().into_owned();
    let _ = mdaw_model::settings::write_browser_roots(&mut st, std::slice::from_ref(&root));
    let _ = mdaw_model::settings::write_browser_last_path(&mut st, Some(&kicks.to_string_lossy()));
    let mut b = crate::ui::browser::BrowserState::new(&st);
    assert!(b.loading && b.listing.is_empty());
    assert!(b.settle(std::time::Duration::from_secs(5)));
    assert_eq!(b.listing.len(), 1);
    assert!(b.selected.as_deref().unwrap().ends_with("BD.wav"));
    // An unreadable folder reports its error.
    r.state
        .browser_navigate(Some(d.join("gone").to_string_lossy().into_owned()));
    settle(&mut r);
    assert!(r.state.browser.error.is_some());
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
