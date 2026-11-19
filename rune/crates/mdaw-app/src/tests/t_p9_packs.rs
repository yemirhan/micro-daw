//! `packs-ui-and-sample-dsp.md` §A: the Packs page (header, search, tag chips, sections, cards,
//! install buttons, the detail panel, kit preview, sound previews, Load on pads, uninstall
//! confirmation, the "Cancelling…" state) on the bundled catalog (headless runs are offline).

use rune::Key;

use super::harness::*;
use crate::app::{AppMode, DawView};

fn packs_rt() -> rune::Runtime<crate::App> {
    let mut rt = rt();
    rt.state.switch_mode(AppMode::Packs);
    rt.state.svc.packs.load_catalog(false);
    let _ = rt
        .state
        .svc
        .packs
        .settle(&mut rt.state.svc.engine, std::time::Duration::from_secs(5));
    rt.invalidate();
    frames(&mut rt, 3);
    rt
}

fn card_ids(rt: &rune::Runtime<crate::App>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.key)
        .filter_map(|k| {
            k.rsplit('/')
                .next()
                .and_then(|l| l.strip_prefix("pack-card-"))
                .map(str::to_owned)
        })
        .collect()
}

#[test]
fn header_sections_and_cards() {
    let rt = packs_rt();
    let t = texts(&rt);
    for s in [
        "Packs",
        "Drum kits for your pads. Built-in machines load instantly; free sample packs download once.",
        "Built-in kits",
        "Synthesized in the app, no download",
        "Downloadable packs",
        "Free recordings with open licences",
        "All",
        "Ready",
        "Get",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    let ids = card_ids(&rt);
    assert_eq!(ids.len(), 6 + 4, "{ids:?}");
    assert!(ids.contains(&"builtin:tr808".to_owned()));
    // Remote counts are shown once the catalog is ready; card aria-label is `name: tagline`.
    assert!(
        rt.accessibility_json()
            .to_string()
            .contains("808-style: Booming analog kick")
    );
}

#[test]
fn search_terms_tags_and_clearing() {
    let mut rt = packs_rt();
    rt.state.p9.packs.query = rune::TextEdit::new("909");
    rt.invalidate();
    frames(&mut rt, 2);
    assert_eq!(card_ids(&rt), vec!["builtin:tr909".to_owned()]);
    // Every word must match.
    rt.state.p9.packs.query = rune::TextEdit::new("snare zzzz");
    rt.invalidate();
    frames(&mut rt, 2);
    assert!(card_ids(&rt).is_empty());
    assert!(
        texts(&rt)
            .iter()
            .any(|x| x == "No packs match \"snare zzzz\"")
    );
    click(&mut rt, "packs-show-all");
    assert!(rt.state.p9.packs.query.text.is_empty() && rt.state.p9.packs.tag.is_none());
    // The clear button empties the query.
    rt.state.p9.packs.query = rune::TextEdit::new("808");
    rt.invalidate();
    frames(&mut rt, 2);
    click(&mut rt, "packs-search-clear");
    assert!(rt.state.p9.packs.query.text.is_empty());
    // A tag chip filters exactly; clicking the active chip keeps it.
    click(&mut rt, "pack-tag-808");
    assert_eq!(rt.state.p9.packs.tag.as_deref(), Some("808"));
    let ids = card_ids(&rt);
    assert_eq!(ids.len(), 2, "{ids:?}");
    click(&mut rt, "pack-tag-808");
    assert_eq!(rt.state.p9.packs.tag.as_deref(), Some("808"));
    click(&mut rt, "pack-tag-all");
    assert!(rt.state.p9.packs.tag.is_none());
    // No "Installed" chip without installed packs.
    assert!(!has(&rt, "pack-tag-__installed"));
}

#[test]
fn selecting_a_builtin_opens_its_detail_and_escape_closes_it() {
    let mut rt = packs_rt();
    click(&mut rt, "pack-card-builtin:tr808");
    assert_eq!(rt.state.p9.packs.selected.as_deref(), Some("builtin:tr808"));
    assert!(has(&rt, "pack-detail"));
    let t = texts(&rt);
    for s in [
        "808-style kit",
        "Load on pads",
        "Kits",
        "Sounds",
        "About",
        "Inspired by the machine's sound; not affiliated with or endorsed by its maker.",
        "Kick",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    // Clicking the selected card again deselects; Escape closes.
    click(&mut rt, "pack-card-builtin:tr808");
    assert!(rt.state.p9.packs.selected.is_none());
    click(&mut rt, "pack-card-builtin:tr808");
    press(&mut rt, ev(Key::Escape, Default::default()));
    assert!(rt.state.p9.packs.selected.is_none());
    assert!(!has(&rt, "pack-detail"));
}

#[test]
fn load_on_pads_replaces_the_kit_and_goes_to_the_pads() {
    let mut rt = packs_rt();
    click(&mut rt, "pack-card-builtin:tr909");
    click(&mut rt, "kit-load-tr909");
    assert_eq!(rt.state.mode, AppMode::Daw);
    assert_eq!(rt.state.view, DawView::Beat);
    assert_eq!(rt.state.kit().id, "classic-tr909");
    assert!(rt.state.svc.packs.snapshot().pending_load.is_none());
}

#[test]
fn kit_preview_plays_the_groove_flashes_and_stops() {
    let mut rt = packs_rt();
    click(&mut rt, "pack-card-builtin:tr808");
    click(&mut rt, "kit-preview-tr808");
    frames(&mut rt, 3);
    let kp = &rt.state.p9.kit_preview;
    assert_eq!(kp.playing.as_deref(), Some("builtin:tr808/tr808"));
    let s = kp.session.clone().expect("groove scheduled");
    assert!((s.step_dur - 60.0 / 96.0 / 4.0).abs() < 1e-12);
    assert!(rt.state.svc.engine.is_preview_playing());
    // The faceplate lights the hit pads in time (step 0: kick + hat).
    let lit = rt.state.p9.kit_preview.flash("builtin:tr808/", s.t0 + 0.01);
    assert!(!lit.is_empty(), "{lit:?}");
    assert!(
        rt.state
            .p9
            .kit_preview
            .flash("builtin:tr909/", s.t0 + 0.01)
            .is_empty()
    );
    // The button is a toggle (aria-pressed) and stops it.
    click(&mut rt, "kit-preview-tr808");
    assert!(rt.state.p9.kit_preview.playing.is_none());
    // Played to the end it stops by itself after 32 steps + 1.2 s.
    click(&mut rt, "kit-preview-tr808");
    run(&mut rt, 32.0 * 60.0 / 96.0 / 4.0 + 1.4);
    assert!(rt.state.p9.kit_preview.playing.is_none());
    // A sound preview stops the groove.
    click(&mut rt, "kit-preview-tr808");
    frames(&mut rt, 2);
    rt.state.play_pack_sound("builtin:tr808", "kick");
    assert!(rt.state.p9.kit_preview.playing.is_none());
    frames(&mut rt, 3);
    assert_eq!(
        rt.state.svc.packs.snapshot().previewing.as_deref(),
        Some("builtin:tr808::kick")
    );
}

#[test]
fn a_remote_pack_streams_previews_and_offers_get_and_load() {
    let mut rt = packs_rt();
    let (_, remotes) = crate::views::pack_items(&rt.state);
    let item = remotes[0].clone();
    let id = item.id().to_owned();
    click(&mut rt, &format!("pack-card-{id}"));
    let t = texts(&rt);
    for s in [
        "Downloads once, then works offline.",
        "Previews stream from the source before you download.",
        "Get & load",
        "Sounds by",
        "Licence",
    ] {
        assert!(t.iter().any(|x| x == s), "{s} missing");
    }
    // Offline (headless): the remote kit's sounds can't load → the error and Retry show.
    let mdaw_model::packs::model::PackItem::Remote { manifest, .. } = &item else {
        unreachable!()
    };
    let kit = manifest.kits[0].id.clone();
    rt.state.preview_kit(&id, &kit);
    frames(&mut rt, 3);
    let key = format!("{id}/{kit}");
    let err = rt.state.p9.kit_preview.errors.get(&key).cloned().unwrap();
    assert!(err.starts_with("Couldn't load this kit's sounds"), "{err}");
    assert!(rt.state.p9.kit_preview.playing.is_none());
    rt.invalidate();
    frames(&mut rt, 2);
    let t = texts(&rt);
    assert!(
        t.iter()
            .any(|x| x.starts_with("Preview failed: Couldn't load")),
        "{:?}",
        t.iter().filter(|x| x.contains("load")).collect::<Vec<_>>()
    );
    assert!(has(&rt, &format!("kit-retry-{kit}")));
}

#[test]
fn uninstall_asks_once_more_and_disarms() {
    let mut rt = packs_rt();
    // Not used by the project: 4 s.
    rt.state.uninstall_click("some-pack", false);
    let until = rt.state.p9.packs.confirm_uninstall.unwrap();
    assert!((until - rt.state.now - 4.0).abs() < 1e-9);
    run(&mut rt, 4.1);
    assert!(rt.state.p9.packs.confirm_uninstall.is_none());
    // Used: 9 s; the second click uninstalls (and disarms).
    rt.state.uninstall_click("some-pack", true);
    let until = rt.state.p9.packs.confirm_uninstall.unwrap();
    assert!((until - rt.state.now - 9.0).abs() < 1e-9);
    rt.state.uninstall_click("some-pack", true);
    assert!(rt.state.p9.packs.confirm_uninstall.is_none());
}

#[test]
fn cancelling_shows_until_the_install_settles() {
    let mut rt = packs_rt();
    rt.state.pack_cancel("uzu-drumkit");
    assert!(rt.state.p9.packs.cancelling.contains("uzu-drumkit"));
    // Not downloading (nothing ran): settled at the next frame.
    frames(&mut rt, 2);
    assert!(rt.state.p9.packs.cancelling.is_empty());
}
