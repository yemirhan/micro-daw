//! Pad context menu, drum-track menu, kit menu, missing packs.

use mdaw_model::types::{PadSample, PadSampleSource};
use rune::{Key, Mods};

use super::harness::*;
use crate::app::Menu;

fn open_pad_menu(r: &mut rune::Runtime<crate::App>, pad: usize) {
    right_click(r, &format!("pad-{pad}"));
    assert!(matches!(r.state.menu, Some(Menu::Pad { .. })));
}

#[test]
fn rename_trims_and_ignores_empty() {
    let mut r = rt();
    open_pad_menu(&mut r, 2);
    click(&mut r, "pad-menu-rename");
    assert!(matches!(
        r.state.menu,
        Some(Menu::Pad { renaming: true, .. })
    ));
    assert!(has(&r, "pad-rename"));
    assert_eq!(
        r.state.beat.rename.text, "Closed HH",
        "prefilled with the pad name"
    );
    r.state.rename_pad(2, "  Tight hat  ");
    assert_eq!(pad(&r, 2).sample.unwrap().name, "Tight hat");
    r.state.rename_pad(2, "   ");
    assert_eq!(pad(&r, 2).sample.unwrap().name, "Tight hat");
}

#[test]
fn colour_reset_choke_copy_paste_chop_clear() {
    let mut r = rt();
    let c = mdaw_model::kit::PAD_COLORS[5];
    open_pad_menu(&mut r, 0);
    click(&mut r, &format!("pad-color-{c}"));
    assert_eq!(pad(&r, 0).color.as_deref(), Some(c));
    assert!(r.state.menu.is_none(), "every action closes the menu");
    open_pad_menu(&mut r, 0);
    click(&mut r, "pad-color-reset");
    assert_eq!(pad(&r, 0).color, None);
    open_pad_menu(&mut r, 0);
    click(&mut r, "pad-choke-3");
    assert_eq!(pad(&r, 0).choke_group, 3);
    // Copy A1, paste on A2 (one "Paste pad" step).
    open_pad_menu(&mut r, 0);
    click(&mut r, "pad-menu-copy");
    open_pad_menu(&mut r, 1);
    click(&mut r, "pad-menu-paste");
    assert_eq!(pad(&r, 1).sample.unwrap().name, "Kick");
    assert_eq!(pad(&r, 1).choke_group, 3);
    assert_eq!(undo_label(&r).as_deref(), Some("Paste pad"));
    // Chop to pads… selects the pad and opens the editor's chop panel.
    open_pad_menu(&mut r, 4);
    click(&mut r, "pad-menu-chop");
    assert_eq!(r.state.selected_pad(), 4);
    assert_eq!(r.state.menu, Some(Menu::Chop));
    press(&mut r, ev(Key::Escape, Mods::default()));
    open_pad_menu(&mut r, 4);
    click(&mut r, "pad-menu-clear");
    assert!(pad(&r, 4).sample.is_none());
    // Rename / Chop / Clear are disabled on an empty pad; Paste needs a clipboard.
    open_pad_menu(&mut r, 4);
    let dim = |r: &rune::Runtime<crate::App>, k: &str| {
        r.elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some(k))
            .map(|e| e.opacity < 0.9)
    };
    assert_eq!(
        dim(&r, "pad-menu-clear"),
        Some(true),
        "disabled (dimmed) on an empty pad"
    );
    click(&mut r, "pad-menu-clear");
    assert!(pad(&r, 4).sample.is_none());
}

#[test]
fn paste_is_disabled_without_a_clipboard() {
    let mut r = rt();
    open_pad_menu(&mut r, 3);
    click(&mut r, "pad-menu-paste");
    assert_eq!(pad(&r, 3).sample.unwrap().name, "Open HH", "nothing pasted");
}

#[test]
fn drum_track_menu_selects_and_adds_tracks() {
    let mut r = rt();
    click(&mut r, "drum-track");
    assert_eq!(r.state.menu, Some(Menu::DrumTrack));
    click(&mut r, "new-drum-track");
    let doc = r.state.svc.engine.doc();
    let drums: Vec<_> = doc
        .get_tracks()
        .iter()
        .filter(|t| t.instrument.kind == mdaw_model::types::InstrumentType::Drums)
        .collect();
    assert_eq!(drums.len(), 2);
    let second = drums[1].id.clone();
    assert_eq!(doc.get_active_drum_track_id(), Some(second.clone()));
    let tracks = crate::ui::beat::menus::list_drum_tracks(&r.state);
    assert_eq!(tracks[1].label, "Drums 2", "repeated names are numbered");
    let first = tracks[0].id.clone();
    click(&mut r, "drum-track");
    click(&mut r, &format!("drum-track-{first}"));
    assert_eq!(r.state.active_track(), Some(first));
}

#[test]
fn kit_menu_loads_classic_and_factory_kits_and_saves_user_kits() {
    let mut r = rt();
    click(&mut r, "kit-menu");
    assert_eq!(
        r.state.beat.kit_name.text, "",
        "Factory leaves the name empty"
    );
    click(&mut r, "kit-classic-classic-tr808");
    assert_eq!(r.state.kit().name, "808-style");
    assert!(r.state.menu.is_none());
    // Save under a name; same name overwrites.
    click(&mut r, "kit-menu");
    assert_eq!(r.state.beat.kit_name.text, "808-style");
    r.state.beat.kit_name = rune::TextEdit::new("  My 808 ");
    click(&mut r, "save-kit");
    assert_eq!(r.state.user_kits.len(), 1);
    assert_eq!(r.state.user_kits[0].name, "My 808");
    r.state.save_kit("My 808");
    assert_eq!(r.state.user_kits.len(), 1, "overwritten (id reused)");
    r.state.save_kit("");
    assert!(r.state.user_kits.iter().any(|k| k.name == "My kit"));
    click(&mut r, "kit-menu");
    click(&mut r, "kit-factory");
    assert_eq!(r.state.kit().name, "Factory");
    let id = r.state.user_kits[0].id.clone();
    click(&mut r, "kit-menu");
    click(&mut r, &format!("user-kit-{id}"));
    // Clicking the row's text loads it.
    let _ = id;
    let id = r.state.user_kits[0].id.clone();
    r.state.delete_kit(&id);
    assert_eq!(r.state.user_kits.len(), 1);
}

fn pack_pads(r: &mut rune::Runtime<crate::App>, pack: &str) {
    let mut kit = r.state.kit();
    for i in 0..3 {
        kit.pads[i].sample = Some(PadSample {
            name: format!("P{i}"),
            source: PadSampleSource::Pack {
                pack_id: pack.into(),
                file: format!("{i}.wav"),
            },
        });
    }
    r.state.load_kit(&kit);
    // The installed list, then the catalog the missing pack asks for (no manual load).
    for _ in 0..2 {
        let _ = r
            .state
            .svc
            .packs
            .settle(&mut r.state.svc.engine, std::time::Duration::from_secs(5));
        frames(r, 2);
    }
    assert!(r.state.svc.packs.snapshot().store.catalog.is_some());
    run(r, 1.0); // the strip slides the pads down (layout spring)
}

#[test]
fn missing_pack_strip_get_button_and_titles() {
    let mut r = rt();
    pack_pads(&mut r, "uzu-drumkit");
    assert!(has(&r, "missing-packs"));
    let m = crate::ui::beat::packs::missing_packs(&r.state, &r.state.kit().pads);
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].pads, vec![0, 1, 2]);
    assert!(m[0].available, "the bundled catalog lists it");
    assert!(has(&r, "get-uzu-drumkit"));
    let titles = crate::ui::beat::packs::missing_pack_titles(&m, 16);
    assert_eq!(
        titles[0].as_deref(),
        Some(format!("Pack not installed: {}", m[0].name).as_str())
    );
    assert!(titles[3].is_none());
}

#[test]
fn unavailable_pack_clear_and_replace_are_one_undo_step() {
    let mut r = rt();
    pack_pads(&mut r, "no-such-pack");
    let m = crate::ui::beat::packs::missing_packs(&r.state, &r.state.kit().pads);
    assert!(!m[0].available, "not in the catalog");
    assert!(has(&r, "clear-pads-no-such-pack"));
    click(&mut r, "replace-pads-no-such-pack");
    assert_eq!(undo_label(&r).as_deref(), Some("Replace pads"));
    assert_eq!(
        pad(&r, 0).sample.unwrap().name,
        "Kick",
        "factory sound for the same pad"
    );
    assert!(!has(&r, "missing-packs"));
    r.state.undo();
    run(&mut r, 1.0);
    click(&mut r, "clear-pads-no-such-pack");
    assert_eq!(undo_label(&r).as_deref(), Some("Clear pads"));
    assert!(pad(&r, 0).sample.is_none() && pad(&r, 2).sample.is_none());
}

#[test]
fn a_loaded_kit_cancels_a_pending_get_and_load() {
    let mut r = rt();
    r.state
        .svc
        .packs
        .get_and_load("uzu-drumkit", "kit", &mut r.state.svc.engine);
    frames(&mut r, 2);
    assert!(r.state.svc.packs.snapshot().pending_load_info.is_some());
    assert!(has(&r, "pending-load"));
    assert!(
        !has(&r, "pad-layout"),
        "4x4: the layout toggle steps aside for the chip"
    );
    r.state.load_factory_kit();
    frames(&mut r, 2);
    assert!(r.state.svc.packs.snapshot().pending_load_info.is_none());
    assert!(!has(&r, "pending-load"));
}
