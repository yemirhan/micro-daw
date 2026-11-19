//! `components/packs/*`: the Packs page (header with search + tag chips, built-in kits,
//! downloadable packs, the 380-point detail aside).

use mdaw_model::packs::PackLicense;
use mdaw_model::packs::model::{
    InstallButtonState, PackItem, SoundRef, filter_items, format_bytes, group_files_by_folder,
    install_button_state, license_label, license_url, lit_pads, pack_file_label, sound_files,
    sound_key, top_tags,
};
use rune::{Color, Cx, El, Icon, Shadow, Tx, col, el, icon, row, spacer, text, text_input};

use crate::app::App;
use crate::icons;
use crate::icons_views;
use crate::theme::{R_LG, R_MD, R_SM, css_color, mono, t, ui};
use crate::ui::beat::packs::progress_ring;
use crate::ui::p9::kit::{Chip, empty_state, empty_text, filter_chips, view_header};
use crate::ui::widgets::{Button, Size, Variant};
use crate::views::{CatalogView, pack_items};

const INSTALLED: &str = "__installed";
const CARD_MIN: f32 = 320.0;
const GAP: f32 = 10.0;

/// `catalogStatus(snapshot)`.
pub fn catalog_view(s: &App) -> CatalogView {
    use mdaw_packs::service::CatalogStatus;
    let snap = s.svc.packs.snapshot();
    if snap.catalog_status == CatalogStatus::Error
        || (!snap.available && snap.store.catalog.is_none())
    {
        CatalogView::Error
    } else if snap.store.catalog.is_some() {
        CatalogView::Ready
    } else {
        CatalogView::Loading
    }
}

/// Faceplate pad order: the MPC layout (`padRows('4x4')`: pad 13–16 on top, 1–4 at the bottom).
fn faceplate_rows() -> [[usize; 4]; 4] {
    [[12, 13, 14, 15], [8, 9, 10, 11], [4, 5, 6, 7], [0, 1, 2, 3]]
}

/// `color-mix(in oklch, a t, b)`: L, C and alpha linearly, the hue along the shorter arc.
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let lch = |c: Color| {
        let [l, x, y, al] = c.to_oklab();
        let ch = (x * x + y * y).sqrt();
        let h = y.atan2(x).to_degrees();
        [l, ch, if h < 0.0 { h + 360.0 } else { h }, al]
    };
    let (mut p, mut q) = (lch(a), lch(b));
    // An achromatic colour's hue is powerless: it takes the other one's.
    if p[1] < 1e-4 {
        p[2] = q[2];
    }
    if q[1] < 1e-4 {
        q[2] = p[2];
    }
    let mut dh = q[2] - p[2];
    if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    let u = 1.0 - t;
    let (l, c, h, al) = (
        p[0] * t + q[0] * u,
        p[1] * t + q[1] * u,
        (p[2] + dh * u).to_radians(),
        p[3] * t + q[3] * u,
    );
    Color::from_oklab([l, c * h.cos(), c * h.sin(), al])
}

/// `Faceplate`: 4×4 pads, lit in the pack colour, hits flashing.
pub fn faceplate(color: &str, lit: [bool; 16], hits: &[usize], large: bool) -> El {
    let th = t();
    let c = css_color(color, th.primary);
    let (size, gap, pad, r) = if large {
        (132.0, 5.0, 8.0, 3.0)
    } else {
        (68.0, 3.0, 5.0, 2.0)
    };
    let cell = (size - 2.0 * pad - 3.0 * gap - 2.0) / 4.0;
    let mut grid = col().gap(gap);
    for rw in faceplate_rows() {
        let mut r_el = row().gap(gap);
        for p in rw {
            let on = lit[p];
            let hit = hits.contains(&p);
            let mut cell_el = el().size(cell, cell).radius(r);
            cell_el = if on {
                if hit {
                    cell_el
                        .fill(mix(c, th.white, 0.7))
                        .shadow(0.0, 10.0, c.alpha(0.7))
                } else {
                    cell_el.fill(mix(c, th.surface1, if p < 8 { 0.62 } else { 0.46 }))
                }
            } else {
                cell_el.fill(th.surface2)
            };
            r_el = r_el.child(cell_el);
        }
        grid = grid.child(r_el);
    }
    el().size(size, size)
        .padding(pad)
        .radius(R_MD)
        .border(1.0, th.hairline_strong)
        .fill(th.surface0)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 3.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.45),
            inset: true,
        })
        .child(grid)
}

/// `InstallButton` (sm / md) with the "Cancelling…" state of the Rust service.
pub fn install_button(s: &App, state: InstallButtonState, name: &str, id: &str, md: bool) -> El {
    let th = t();
    let h = if md { 32.0 } else { 28.0 };
    let size = if md { Size::Md } else { Size::Sm };
    let (i1, i2) = (id.to_owned(), id.to_owned());
    if s.p9.packs.cancelling.contains(id) {
        return row()
            .key(format!("cancelling-{id}"))
            .h(h)
            .items_center()
            .gap(6.0)
            .px(8.0)
            .radius(R_MD)
            .border(1.0, th.border)
            .fill(th.surface0)
            .child(progress_ring(None, 18.0, "Cancelling"))
            .child(text("Cancelling…").font(ui(11.0, 500.0)).color(th.muted_fg));
    }
    match state {
        InstallButtonState::Builtin => row()
            .h(h)
            .px(6.0)
            .items_center()
            .gap(4.0)
            .child(icon(&icons::ZAP, 14.0).color(th.primary))
            .child(text("Ready").font(ui(11.0, 500.0)).color(th.muted_fg)),
        InstallButtonState::Get => Button::new(Variant::Primary, size)
            .icon(&icons::DOWNLOAD)
            .label("Get")
            .a11y(format!("Get {name}"))
            .key(format!("pack-get-{id}"))
            .build(move |s: &mut App, _| s.get_pack(&i1)),
        InstallButtonState::Downloading { progress, label } => row()
            .key(format!("pack-downloading-{id}"))
            .h(h)
            .items_center()
            .gap(6.0)
            .pl(6.0)
            .radius(R_MD)
            .border(1.0, th.border)
            .fill(th.surface0)
            .child(progress_ring(progress, 18.0, "Download progress"))
            .child(text(label).font(mono(11.0, 400.0)).color(th.fg).min_w(30.0))
            .child(
                row()
                    .key(format!("pack-cancel-{id}"))
                    .role("button")
                    .label(format!("Cancel downloading {name}"))
                    .help("Cancel download")
                    .h_full()
                    .px(6.0)
                    .items_center()
                    .border_sides([0.0, 0.0, 0.0, 1.0], th.border)
                    .hover(th.surface3)
                    .child(icon(&icons::X, 14.0).color(th.muted_fg))
                    .on_press(move |s: &mut App| s.pack_cancel(&i2)),
            ),
        InstallButtonState::Installed => row()
            .h(h)
            .px(6.0)
            .items_center()
            .gap(4.0)
            .child(icon(&icons::CHECK, 14.0).color(th.play))
            .child(text("Installed").font(ui(11.0, 500.0)).color(th.play)),
        InstallButtonState::Resume => Button::new(Variant::Outline, size)
            .icon(&icons::DOWNLOAD)
            .label("Resume")
            .a11y(format!("Resume downloading {name}"))
            .tip("Download paused: continues where it stopped", None)
            .key(format!("pack-resume-{id}"))
            .build(move |s: &mut App, _| s.get_pack(&i1)),
        InstallButtonState::Error { message } => Button::new(Variant::Outline, size)
            .icon(&icons::ROTATE_CW)
            .label("Retry")
            .a11y(format!("Retry downloading {name}"))
            .tip(
                if message.is_empty() {
                    "Download failed".to_owned()
                } else {
                    message
                },
                None,
            )
            .border(th.rec.alpha(0.4))
            .ink(th.rec)
            .key(format!("pack-retry-{id}"))
            .build(move |s: &mut App, _| s.get_pack(&i1)),
    }
}

fn license_badge(item: &PackItem, short: bool) -> El {
    let th = t();
    match item {
        PackItem::Remote { manifest, .. } => {
            let label = if short && manifest.license == PackLicense::PublicDomain {
                "PD"
            } else {
                license_label(manifest.license)
            };
            row()
                .px(4.0)
                .py(1.0)
                .radius(R_SM - 2.0)
                .border(1.0, th.border)
                .child(
                    text(label)
                        .font(ui(10.0, 600.0).tracking(0.25))
                        .color(th.fg.alpha(0.8)),
                )
        }
        PackItem::Builtin { .. } => row()
            .px(4.0)
            .py(1.0)
            .radius(R_SM - 2.0)
            .border(1.0, th.primary.alpha(0.3))
            .child(
                text("Synth")
                    .font(ui(10.0, 600.0).tracking(0.25))
                    .color(th.primary),
            ),
    }
}

fn state_of(s: &App, item: &PackItem) -> InstallButtonState {
    let snap = s.svc.packs.snapshot();
    let installed = snap.store.installed.iter().any(|p| p.id == item.id());
    install_button_state(item, snap.store.install_states.get(item.id()), installed)
}

fn item_color(item: &PackItem) -> String {
    match item {
        PackItem::Builtin { color, .. } | PackItem::Remote { color, .. } => color.clone(),
    }
}

fn card(s: &App, item: &PackItem, w: f32) -> El {
    let th = t();
    let selected = s.p9.packs.selected.as_deref() == Some(item.id());
    let state = state_of(s, item);
    let color = item_color(item);
    let c = css_color(&color, th.primary);
    let size = match item {
        PackItem::Remote { size_bytes, .. } if state == InstallButtonState::Get => {
            format_bytes(size_bytes.map(|b| b as f64))
        }
        _ => String::new(),
    };
    let hits = s.p9.kit_preview.flash(&format!("{}/", item.id()), s.now);
    let id = item.id().to_owned();
    let mut meta = row()
        .w_full()
        .min_h(28.0)
        .items_center()
        .gap(8.0)
        .pt(6.0)
        .child(license_badge(item, true))
        .child(
            row()
                .min_w(0.0)
                .child(
                    text(format!("{} sounds", item.sound_count()))
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg),
                )
                .child(if size.is_empty() {
                    el()
                } else {
                    text(format!(", {size}"))
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg.alpha(0.7))
                }),
        )
        .child(spacer());
    meta = meta.child(install_button(s, state, item.name(), item.id(), false));
    let mut a = row()
        .key(format!("pack-card-{}", item.id()))
        .role("button")
        .label(format!("{}: {}", item.name(), item.tagline()))
        .checked(selected)
        .w(w)
        .items_stretch()
        .gap(12.0)
        .padding(10.0)
        .pb(8.0)
        .radius(R_LG)
        .border(
            1.0,
            if selected {
                Color::TRANSPARENT
            } else {
                th.border
            },
        )
        .fill(if selected { th.surface2 } else { th.surface1 })
        .child(faceplate(&color, lit_pads(item), &hits, false))
        .child(
            col()
                .min_w(0.0)
                .grow(1.0)
                .child(
                    text(item.name().to_owned())
                        .font(ui(13.5, 600.0).lh(16.9))
                        .color(th.fg)
                        .truncate()
                        .pt(2.0),
                )
                .child(
                    text(item.tagline().to_owned())
                        .font(ui(12.0, 400.0).lh(16.5))
                        .color(th.muted_fg)
                        .wrap()
                        .max_lines(2)
                        .mt(2.0),
                )
                .child(spacer())
                .child(meta),
        )
        .on_press(move |s: &mut App| s.select_pack(&id));
    if selected {
        a = a.box_shadow(Shadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            spread: 1.5,
            color: c,
            inset: false,
        });
    } else {
        a = a.hover(th.surface2);
    }
    a
}

fn grid(s: &App, items: &[PackItem], width: f32) -> El {
    let cols = (((width + GAP) / (CARD_MIN + GAP)).floor() as usize).max(1);
    let w = (width - GAP * (cols - 1) as f32) / cols as f32;
    let mut g = col().w_full().gap(GAP);
    for chunk in items.chunks(cols) {
        let mut r = row().gap(GAP);
        for it in chunk {
            r = r.child(card(s, it, w));
        }
        g = g.child(r);
    }
    g
}

fn section_header(title: &str, count: Option<usize>, note: &str, extra: Option<El>) -> El {
    let th = t();
    let mut h = row()
        .w_full()
        .items_end()
        .gap(10.0)
        .mb(10.0)
        .child(text(title.to_owned()).font(ui(13.0, 600.0)).color(th.fg));
    if let Some(n) = count {
        h = h.child(text(n.to_string()).font(ui(11.0, 400.0)).color(th.muted_fg));
    }
    h = h.child(
        text(note.to_owned())
            .font(ui(12.0, 400.0))
            .color(th.muted_fg),
    );
    if let Some(e) = extra {
        h = h.child(spacer()).child(e);
    }
    h
}

fn skeletons(width: f32) -> El {
    let th = t();
    let cols = (((width + GAP) / (CARD_MIN + GAP)).floor() as usize).max(1);
    let w = (width - GAP * (cols - 1) as f32) / cols as f32;
    let sk = |ww: f32, h: f32| el().w(ww).h(h).radius(R_SM).fill(th.surface3);
    let mut g = col()
        .key("packs-loading")
        .role("group")
        .label("Loading packs")
        .w_full()
        .gap(GAP);
    let cards: Vec<El> = (0..4)
        .map(|_| {
            row()
                .w(w)
                .gap(12.0)
                .padding(10.0)
                .radius(R_LG)
                .border(1.0, th.border)
                .fill(th.surface1)
                .child(el().size(68.0, 68.0).radius(R_MD).fill(th.surface3))
                .child(
                    col()
                        .grow(1.0)
                        .gap(8.0)
                        .pt(4.0)
                        .child(sk((w - 100.0) * 0.66, 14.0))
                        .child(sk(w - 100.0, 12.0))
                        .child(spacer())
                        .child(sk((w - 100.0) * 0.33, 12.0)),
                )
        })
        .collect();
    let mut it = cards.into_iter();
    loop {
        let mut r = row().gap(GAP);
        let mut any = false;
        for _ in 0..cols {
            if let Some(c) = it.next() {
                r = r.child(c);
                any = true;
            }
        }
        if !any {
            break;
        }
        g = g.child(r);
    }
    g
}

fn dashed_box(content: El) -> El {
    let th = t();
    col()
        .w_full()
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(th.surface1.alpha(0.5))
        .child(content)
}

/// `PacksBrowser`.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    let (builtins, remotes) = pack_items(s);
    let snap = s.svc.packs.snapshot();
    let installed: Vec<String> = snap.store.installed.iter().map(|p| p.id.clone()).collect();
    let all: Vec<PackItem> = builtins.iter().chain(remotes.iter()).cloned().collect();
    let tag = s.p9.packs.tag.clone();
    let query = s.p9.packs.query.text.clone();
    // Chips: All, Installed (when any), topTags(all, 9).
    let mut opts: Vec<(Option<String>, String, usize)> = vec![(None, "All".into(), all.len())];
    if !installed.is_empty() {
        opts.push((Some(INSTALLED.into()), "Installed".into(), installed.len()));
    }
    for tc in top_tags(&all, 9) {
        opts.push((Some(tc.tag.clone()), tc.tag.clone(), tc.count));
    }
    let chips = opts
        .iter()
        .map(|(v, l, n)| Chip {
            key: format!("pack-tag-{}", v.as_deref().unwrap_or("all")),
            label: l.clone(),
            count: Some(*n),
            active: *v == tag,
        })
        .collect();
    let values: Vec<Option<String>> = opts.iter().map(|o| o.0.clone()).collect();
    let visible = |items: &[PackItem]| -> Vec<PackItem> {
        if tag.as_deref() == Some(INSTALLED) {
            filter_items(items, &query, None)
                .into_iter()
                .filter(|i| !i.is_builtin() && installed.iter().any(|x| x == i.id()))
                .collect()
        } else {
            filter_items(items, &query, tag.as_deref())
        }
    };
    let shown_b = visible(&builtins);
    let shown_r = visible(&remotes);
    let filtering = !query.trim().is_empty() || tag.is_some();
    let status = catalog_view(s);
    let width = cx
        .layout_size("packs-column")
        .map_or(1012.0, |z| z[0])
        .max(CARD_MIN);

    let search = row()
        .w(260.0)
        .h(32.0)
        .items_center()
        .gap(6.0)
        .pl(10.0)
        .pr(6.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0)
        .child(icon(&icons::SEARCH, 14.0).color(th.muted_fg))
        .child(
            text_input(&s.p9.packs.query, ui(13.0, 400.0))
                .key("packs-search")
                .label("Search packs")
                .single_line()
                .placeholder("Search packs, sounds, styles", th.muted_fg)
                .color(th.fg)
                .grow(1.0)
                .on_edit(|s: &mut App| &mut s.p9.packs.query)
                .on_input_key(|s: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx| {
                    if matches!(ev.key, rune::Key::Escape) && !s.p9.packs.query.text.is_empty() {
                        s.p9.packs.query.clear();
                        return true;
                    }
                    false
                }),
        )
        .child(if query.is_empty() {
            el()
        } else {
            row()
                .key("packs-search-clear")
                .role("button")
                .label("Clear search")
                .size(20.0, 20.0)
                .center()
                .radius(R_SM)
                .child(icon(&icons::X, 14.0).color(th.muted_fg))
                .on_press(|s: &mut App| s.p9.packs.query.clear())
        });
    let header = view_header(
        &icons::PACKAGE,
        "Packs",
        "Drum kits for your pads. Built-in machines load instantly; free sample packs download once.",
        Some(search),
        Some(filter_chips(
            "Filter by tag",
            chips,
            move |s: &mut App, i| {
                s.p9.packs.tag = values[i].clone();
            },
        )),
        [24.0, 16.0, 12.0],
    );

    let mut column = col()
        .key("packs-column")
        .w_full()
        .max_w(1400.0)
        .gap(28.0)
        .px(24.0)
        .pt(20.0)
        .pb(40.0);
    if !shown_b.is_empty() || !filtering {
        column = column.child(
            col()
                .key("packs-builtin")
                .label("Built-in kits")
                .w_full()
                .child(section_header(
                    "Built-in kits",
                    None,
                    "Synthesized in the app, no download",
                    None,
                ))
                .child(grid(s, &shown_b, width - 48.0)),
        );
    }
    let updating = (status == CatalogView::Loading && !remotes.is_empty()).then(|| {
        row()
            .items_center()
            .gap(6.0)
            .role("status")
            .child(progress_ring(None, 13.0, "Loading catalog"))
            .child(
                text("Updating catalog")
                    .font(ui(11.0, 400.0))
                    .color(th.muted_fg),
            )
    });
    let mut downloadable = col()
        .key("packs-remote")
        .label("Downloadable packs")
        .w_full()
        .child(section_header(
            "Downloadable packs",
            (status == CatalogView::Ready).then_some(shown_r.len()),
            "Free recordings with open licences",
            updating,
        ));
    let try_again = |key: &str| {
        Button::new(Variant::Outline, Size::Sm)
            .icon(&icons::ROTATE_CW)
            .label("Try again")
            .key(key.to_owned())
            .build(|s: &mut App, _| s.svc.packs.load_catalog(true))
    };
    if status == CatalogView::Loading && remotes.is_empty() {
        downloadable = downloadable.child(skeletons(width - 48.0));
    } else if status == CatalogView::Error && remotes.is_empty() {
        let err = snap
            .catalog_error
            .clone()
            .filter(|e| !e.is_empty())
            .or_else(|| {
                (!snap.available).then(|| "Downloading packs needs the desktop app.".into())
            })
            .unwrap_or_else(|| "Check your internet connection.".into());
        downloadable = downloadable.child(dashed_box(empty_state(
            Some(&icons_views::CLOUD_OFF),
            "Can't reach the pack catalog",
            vec![
                empty_text(&format!("{err} Built-in kits still work offline.")),
                el().h(12.0),
                try_again("packs-retry"),
            ],
        )));
    } else if !shown_r.is_empty() {
        if status == CatalogView::Error {
            downloadable = downloadable.child(
                row()
                    .w_full()
                    .mb(10.0)
                    .items_center()
                    .gap(8.0)
                    .px(12.0)
                    .py(6.0)
                    .radius(R_MD)
                    .border(1.0, th.border)
                    .fill(th.surface1)
                    .child(icon(&icons_views::CLOUD_OFF, 14.0).color(th.muted_fg))
                    .child(
                        text("Offline: showing installed packs only.")
                            .font(ui(12.0, 400.0))
                            .color(th.muted_fg),
                    )
                    .child(spacer())
                    .child(
                        Button::new(Variant::Ghost, Size::Xs)
                            .icon(&icons::ROTATE_CW)
                            .label("Try again")
                            .key("packs-retry-offline")
                            .build(|s: &mut App, _| s.svc.packs.load_catalog(true)),
                    ),
            );
        }
        downloadable = downloadable.child(grid(s, &shown_r, width - 48.0));
    } else if filtering {
        let q = query.trim().to_owned();
        let title = if q.is_empty() {
            "No packs with this tag".to_owned()
        } else {
            format!("No packs match \"{q}\"")
        };
        downloadable = downloadable.child(dashed_box(empty_state(
            Some(&icons::SEARCH),
            &title,
            vec![
                el().h(8.0),
                Button::new(Variant::Outline, Size::Sm)
                    .label("Show all packs")
                    .key("packs-show-all")
                    .build(|s: &mut App, _| {
                        s.p9.packs.query.clear();
                        s.p9.packs.tag = None;
                    }),
            ],
        )));
    } else {
        downloadable = downloadable.child(dashed_box(empty_state(
            Some(&icons::PACKAGE),
            "No downloadable packs yet",
            vec![empty_text(
                "The catalog is empty right now. Built-in kits are ready to play above.",
            )],
        )));
    }
    column = column.child(downloadable);

    let mut body =
        row()
            .grow(1.0)
            .min_h(0.0)
            .w_full()
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .h_full()
                    .child(col().w_full().child(column).scroll(rune::ScrollSpec {
                        stick_bottom: false,
                    })),
            );
    if let Some(sel) = s.p9.packs.selected.clone()
        && let Some(item) = all.iter().find(|i| i.id() == sel)
    {
        body = body.child(detail(s, item));
    }
    col()
        .key("packs-view")
        .w_full()
        .h_full()
        .fill(th.surface0)
        .child(header)
        .child(body)
}

fn section_title(title: &str, aside: Option<String>) -> El {
    let th = t();
    let mut r = row()
        .w_full()
        .mb(6.0)
        .px(4.0)
        .items_end()
        .justify_between()
        .child(text(title.to_owned()).font(ui(12.0, 600.0)).color(th.fg));
    if let Some(a) = aside {
        r = r.child(text(a).font(ui(11.0, 400.0)).color(th.muted_fg));
    }
    r
}

struct SoundRow {
    key: String,
    label: String,
    arg: String,
    title: String,
}

fn sound_list(s: &App, item_id: &str, rows: Vec<SoundRow>) -> El {
    let th = t();
    let previewing = s.svc.packs.snapshot().previewing.clone();
    let mut list = col().w_full().gap(2.0);
    for pair in rows.chunks(2) {
        let mut r = row().w_full().gap(2.0);
        for sr in pair {
            let error = s.p9.packs.sound_errors.get(&sr.key).cloned();
            let playing = previewing.as_deref() == Some(sr.key.as_str());
            let (ic, ic_col): (&'static Icon, Color) = if error.is_some() {
                (&icons::TRIANGLE_ALERT, th.rec)
            } else if playing {
                (&icons::VOLUME_2, th.primary)
            } else {
                (&icons::PLAY_FILLED, th.muted_fg)
            };
            let (id, arg) = (item_id.to_owned(), sr.arg.clone());
            let mut b = row()
                .key(format!("sound-{}", sr.key))
                .role("button")
                .label(match &error {
                    Some(e) => format!("Retry {} (preview failed: {e})", sr.label),
                    None => format!("Play {}", sr.label),
                })
                .help(match &error {
                    Some(e) => format!("Couldn't play {}: {e}. Click to retry.", sr.label),
                    None => sr.title.clone(),
                })
                .w_frac(0.5)
                .min_w(0.0)
                .items_center()
                .gap(6.0)
                .px(6.0)
                .py(4.0)
                .radius(R_SM - 2.0)
                .hover(th.surface3)
                .child(icon(ic, 12.0).color(ic_col))
                .child(
                    text(sr.label.clone())
                        .font(ui(12.0, 400.0).lh(16.8))
                        .color(if error.is_some() || playing {
                            th.fg
                        } else {
                            th.fg.alpha(0.8)
                        })
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0),
                )
                .on_press(move |s: &mut App| s.play_pack_sound(&id, &arg));
            if error.is_some() {
                b = b.fill(th.rec.alpha(0.1)).child(
                    row()
                        .items_center()
                        .gap(2.0)
                        .child(icon(&icons::ROTATE_CW, 10.0).color(th.rec))
                        .child(text("Retry").font(ui(10.5, 500.0)).color(th.rec)),
                );
            }
            r = r.child(b);
        }
        list = list.child(r);
    }
    list
}

/// `PackDetail`.
pub fn detail(s: &App, item: &PackItem) -> El {
    let th = t();
    let snap = s.svc.packs.snapshot();
    let installed = snap.store.installed.iter().any(|p| p.id == item.id());
    let ready = item.is_builtin() || installed;
    let state = state_of(s, item);
    let downloading = matches!(state, InstallButtonState::Downloading { .. });
    let color = item_color(item);
    let c = css_color(&color, th.primary);
    let hits = s.p9.kit_preview.flash(&format!("{}/", item.id()), s.now);
    let id = item.id().to_owned();
    let size = match item {
        PackItem::Remote { size_bytes, .. } => format_bytes(size_bytes.map(|b| b as f64)),
        _ => String::new(),
    };
    let mut badges = row()
        .flex_wrap()
        .items_center()
        .gaps(6.0, 6.0)
        .pt(8.0)
        .child(license_badge(item, false))
        .child(
            text(format!("{} sounds", item.sound_count()))
                .font(ui(11.0, 400.0))
                .color(th.muted_fg),
        );
    if !size.is_empty() {
        badges = badges.child(text(size.clone()).font(ui(11.0, 400.0)).color(th.muted_fg));
    }
    let mut head = col()
        .w_full()
        .padding(16.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .radial_gradient(
            [0.0, 0.0],
            [456.0, 200.0],
            [(0.0, c.alpha(0.16 * 0.6)), (0.6, c.alpha(0.0))],
        )
        .child(
            row()
                .w_full()
                .gap(16.0)
                .child(faceplate(&color, lit_pads(item), &hits, true))
                .child(
                    col()
                        .min_w(0.0)
                        .grow(1.0)
                        .pt(2.0)
                        .pr(20.0)
                        .child(
                            text(item.name().to_owned())
                                .font(ui(17.0, 600.0).lh(21.25).tracking(-0.4))
                                .color(th.fg)
                                .wrap(),
                        )
                        .child(
                            text(item.tagline().to_owned())
                                .font(ui(12.5, 400.0).lh(17.2))
                                .color(th.muted_fg)
                                .wrap()
                                .mt(4.0),
                        )
                        .child(spacer())
                        .child(badges),
                ),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::X)
                .a11y("Close details")
                .tip("Close", Some("Esc"))
                .key("pack-detail-close")
                .build(|s: &mut App, _| s.close_pack_detail())
                .absolute(380.0 - 10.0 - 24.0 - 1.0, 10.0),
        );
    if let PackItem::Remote { name, .. } = item {
        let used = installed && s.pack_used_by_project(item.id());
        let confirm = s.p9.packs.confirm_uninstall.is_some();
        let mut actions = row()
            .w_full()
            .mt(12.0)
            .items_center()
            .gap(8.0)
            .child(install_button(s, state.clone(), name, item.id(), true));
        match &state {
            InstallButtonState::Get => {
                actions = actions.child(
                    text("Downloads once, then works offline.")
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg),
                );
            }
            InstallButtonState::Resume => {
                actions = actions.child(
                    text("Paused: picks up where it stopped.")
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg),
                );
            }
            InstallButtonState::Error { message } => {
                actions = actions.child(
                    text(if message.is_empty() {
                        "Download failed".to_owned()
                    } else {
                        message.clone()
                    })
                    .font(ui(11.0, 400.0))
                    .color(th.rec)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
                );
            }
            _ => {}
        }
        let has_update = snap
            .store
            .catalog
            .as_ref()
            .and_then(|c| c.packs.iter().find(|p| p.id == item.id()))
            .is_some_and(|m| s.svc.packs.has_update(m));
        if installed && has_update && state == InstallButtonState::Installed {
            let i = id.clone();
            actions = actions.child(
                Button::new(Variant::Outline, Size::Sm)
                    .icon(&icons::REFRESH_CW)
                    .label("Update")
                    .key("pack-update")
                    .build(move |s: &mut App, _| s.get_pack(&i)),
            );
        }
        if installed && let Some(err) = snap.uninstall_errors.get(item.id()) {
            actions = actions.child(
                text(format!("Couldn't uninstall: {err}"))
                    .role("alert")
                    .font(ui(11.0, 400.0))
                    .color(th.rec)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            );
        }
        if installed {
            let i = id.clone();
            actions = actions.child(spacer()).child(
                Button::new(Variant::Ghost, Size::Sm)
                    .icon(&icons::TRASH_2)
                    .label(if confirm {
                        "Confirm uninstall"
                    } else {
                        "Uninstall"
                    })
                    .ink(if confirm { th.rec } else { th.muted_fg })
                    .key("pack-uninstall")
                    .build(move |s: &mut App, _| s.uninstall_click(&i, used)),
            );
        }
        head = head.child(actions);
        if installed && confirm && used {
            head = head.child(
                row()
                    .role("alert")
                    .w_full()
                    .mt(8.0)
                    .items_start()
                    .gap(6.0)
                    .px(8.0)
                    .py(6.0)
                    .radius(R_MD)
                    .border(1.0, th.warn.alpha(0.4))
                    .fill(th.warn.alpha(0.1))
                    .child(icon(&icons::TRIANGLE_ALERT, 14.0).color(th.warn).mt(1.0))
                    .child(
                        text("Your open project plays sounds from this pack. Its pads go silent until you get the pack again.")
                            .font(ui(11.5, 400.0).lh(15.8))
                            .color(th.fg.alpha(0.9))
                            .wrap()
                            .grow(1.0)
                            .min_w(0.0),
                    ),
            );
        }
    }

    // Kits
    let kits: Vec<(String, String, usize)> = match item {
        PackItem::Builtin { machine, .. } => vec![(
            machine.id.id().to_owned(),
            format!("{} kit", item.name()),
            lit_pads(item).iter().filter(|b| **b).count(),
        )],
        PackItem::Remote { manifest, .. } => manifest
            .kits
            .iter()
            .map(|k| (k.id.clone(), k.name.clone(), k.pads.len()))
            .collect(),
    };
    let mut kits_sec = col()
        .key("pack-kits")
        .label("Kits")
        .w_full()
        .padding(12.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(section_title(
            "Kits",
            (kits.len() > 1).then(|| format!("{} kits", kits.len())),
        ));
    if kits.is_empty() {
        kits_sec = kits_sec.child(
            text("This pack has no ready-made kit; play its sounds below.")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg)
                .px(4.0),
        );
    }
    let previewing = s.p9.kit_preview.playing.clone();
    let pending = snap.pending_load.clone();
    let mut list = col().w_full().gap(4.0);
    for (i, (kid, kname, pads)) in kits.iter().enumerate() {
        let key = format!("{}/{kid}", item.id());
        let playing = previewing.as_deref() == Some(key.as_str());
        let err = s.p9.kit_preview.errors.get(&key).cloned();
        let waiting = pending.as_deref() == Some(key.as_str());
        let can_get = !item.is_builtin() && !installed;
        let (i1, k1, i2, k2, i3, k3) = (
            id.clone(),
            kid.clone(),
            id.clone(),
            kid.clone(),
            id.clone(),
            kid.clone(),
        );
        let preview_btn = Button::new(Variant::Ghost, Size::IconSm)
            .icon(if playing {
                &icons::SQUARE_FILLED
            } else {
                &icons::PLAY_FILLED
            })
            .icon_size(14.0)
            .a11y(if playing {
                format!("Stop preview of {kname}")
            } else {
                format!("Preview {kname}")
            })
            .pressed(playing)
            .tip(
                if playing {
                    "Stop preview"
                } else if ready {
                    "Play a short groove on this kit"
                } else {
                    "Play a short groove (streams the sounds, nothing is installed)"
                },
                None,
            )
            .key(format!("kit-preview-{kid}"))
            .build(move |s: &mut App, _| {
                if playing {
                    s.stop_kit_preview();
                } else {
                    s.preview_kit(&i1, &k1);
                }
            });
        let mut line = row()
            .w_full()
            .items_center()
            .gap(8.0)
            .child(icon(&icons_views::LAYOUT_GRID, 14.0).color(th.muted_fg))
            .child(
                col()
                    .min_w(0.0)
                    .grow(1.0)
                    .child(
                        text(kname.clone())
                            .font(ui(12.5, 500.0))
                            .color(th.fg)
                            .truncate(),
                    )
                    .child(
                        text(format!("{pads} pads"))
                            .font(ui(10.5, 400.0))
                            .color(th.muted_fg),
                    ),
            )
            .child(preview_btn);
        if ready {
            line = line.child(
                Button::new(
                    if i == 0 {
                        Variant::Primary
                    } else {
                        Variant::Outline
                    },
                    Size::Sm,
                )
                .label("Load on pads")
                .tip("Replace the pads of the current drum track", None)
                .key(format!("kit-load-{kid}"))
                .build(move |s: &mut App, _| s.load_pack_kit(&i2, &k2)),
            );
        } else if can_get {
            line = line.child(
                Button::new(
                    if i == 0 {
                        Variant::Primary
                    } else {
                        Variant::Outline
                    },
                    Size::Sm,
                )
                .icon(&icons::DOWNLOAD)
                .label(if waiting { "Getting…" } else { "Get & load" })
                .a11y(if waiting {
                    format!("Getting {}, then loading {kname}", item.name())
                } else {
                    format!("Get {} and load {kname}", item.name())
                })
                .tip(
                    if waiting {
                        "Loads on the pads when the download finishes".to_owned()
                    } else {
                        format!("Download {}, then load this kit on the pads", item.name())
                    },
                    None,
                )
                .disabled(downloading)
                .key(format!("kit-get-{kid}"))
                .build(move |s: &mut App, _| s.get_and_load(&i3, &k3)),
            );
        }
        let mut li = col()
            .w_full()
            .radius(R_MD)
            .border(1.0, th.border)
            .fill(th.surface2)
            .py(6.0)
            .pr(6.0)
            .pl(10.0)
            .child(line);
        if let Some(e) = err {
            let (i4, k4) = (id.clone(), kid.clone());
            li = li.child(
                row()
                    .role("alert")
                    .w_full()
                    .mt(4.0)
                    .pl(22.0)
                    .items_center()
                    .gap(6.0)
                    .child(
                        text(if playing {
                            e.clone()
                        } else {
                            format!("Preview failed: {e}")
                        })
                        .font(ui(11.0, 400.0))
                        .color(th.rec)
                        .truncate()
                        .grow(1.0)
                        .min_w(0.0)
                        .help(e),
                    )
                    .child(
                        Button::new(Variant::Ghost, Size::Xs)
                            .icon(&icons::ROTATE_CW)
                            .label("Retry")
                            .ink(th.rec)
                            .key(format!("kit-retry-{kid}"))
                            .build(move |s: &mut App, _| s.preview_kit(&i4, &k4)),
                    ),
            );
        }
        list = list.child(li);
    }
    kits_sec = kits_sec.child(list);

    // Sounds
    let mut sounds = col()
        .key("pack-sounds")
        .label("Sounds")
        .w_full()
        .padding(12.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(section_title(
            "Sounds",
            Some(item.sound_count().to_string()),
        ));
    let errors = &s.p9.packs.sound_errors;
    let failed = match item {
        PackItem::Builtin { machine, .. } => machine.voices.iter().find_map(|v| {
            errors
                .get(&sound_key(item.id(), &SoundRef::Voice(&v.id)))
                .map(|e| (v.label.clone(), e.clone()))
        }),
        PackItem::Remote { manifest, .. } => manifest.files.iter().find_map(|f| {
            errors
                .get(&sound_key(item.id(), &SoundRef::File(f)))
                .map(|e| (pack_file_label(f), e.clone()))
        }),
    };
    if let Some((label, e)) = failed {
        sounds = sounds.child(
            row()
                .role("alert")
                .w_full()
                .mb(6.0)
                .items_start()
                .gap(6.0)
                .px(6.0)
                .py(4.0)
                .radius(R_SM - 2.0)
                .fill(th.rec.alpha(0.1))
                .child(icon(&icons::TRIANGLE_ALERT, 12.0).color(th.rec).mt(1.0))
                .child(
                    text(format!("Couldn't play {label}: {e}. Click it to retry."))
                        .font(ui(11.0, 400.0))
                        .color(th.rec)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                ),
        );
    }
    match item {
        PackItem::Builtin { machine, .. } => {
            let rows = machine
                .voices
                .iter()
                .map(|v| SoundRow {
                    key: sound_key(item.id(), &SoundRef::Voice(&v.id)),
                    label: v.label.clone(),
                    arg: v.id.clone(),
                    title: format!("{} ({})", v.label, v.id),
                })
                .collect();
            sounds = sounds.child(sound_list(s, item.id(), rows));
        }
        PackItem::Remote { manifest, .. } => {
            for g in group_files_by_folder(
                &sound_files(&manifest.files)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>(),
            ) {
                let mut gc = col().w_full().mb(6.0);
                if !g.folder.is_empty() {
                    gc = gc.child(
                        row()
                            .w_full()
                            .items_center()
                            .gap(6.0)
                            .px(4.0)
                            .pt(4.0)
                            .pb(2.0)
                            .child(icon(&icons::FOLDER, 12.0).color(th.muted_fg))
                            .child(
                                text(g.folder.clone())
                                    .font(ui(11.0, 500.0))
                                    .color(th.muted_fg)
                                    .truncate(),
                            )
                            .child(spacer())
                            .child(
                                text(g.files.len().to_string())
                                    .font(ui(11.0, 500.0))
                                    .color(th.muted_fg),
                            ),
                    );
                }
                let rows = g
                    .files
                    .iter()
                    .map(|f| {
                        let label = pack_file_label(f);
                        SoundRow {
                            key: sound_key(item.id(), &SoundRef::File(f)),
                            title: if label == f.path {
                                f.path.clone()
                            } else {
                                format!("{label} ({})", f.path)
                            },
                            label,
                            arg: f.path.clone(),
                        }
                    })
                    .collect();
                gc = gc.child(sound_list(s, item.id(), rows));
                sounds = sounds.child(gc);
            }
            if !installed {
                sounds = sounds.child(
                    text("Previews stream from the source before you download.")
                        .font(ui(11.0, 400.0))
                        .color(th.muted_fg)
                        .mt(8.0)
                        .px(4.0),
                );
            }
        }
    }

    // About
    let mut about = col()
        .key("pack-about")
        .label("About")
        .w_full()
        .padding(12.0)
        .pb(20.0)
        .child(section_title("About", None))
        .child(
            text(item.description().to_owned())
                .font(ui(12.5, 400.0).lh(20.3))
                .color(th.fg.alpha(0.85))
                .wrap()
                .px(4.0),
        );
    match item {
        PackItem::Remote { manifest, .. } => {
            let link = |label: String, url: Option<String>, key: &str| {
                let mut r = row()
                    .key(key.to_owned())
                    .items_center()
                    .gap(4.0)
                    .min_w(0.0)
                    .child(text(label).font(ui(12.0, 400.0)).color(th.fg).truncate());
                if let Some(u) = url {
                    r = r
                        .role("link")
                        .child(icon(&icons_views::EXTERNAL_LINK, 12.0).color(th.muted_fg))
                        .on_press_tx(move |_s: &mut App, tx: &mut Tx| {
                            let _ = tx.open_url(&u);
                        });
                }
                r
            };
            let dt = |l: &str| {
                text(l.to_owned())
                    .font(ui(12.0, 400.0))
                    .color(th.muted_fg)
                    .w(70.0)
            };
            let mut dl = col()
                .w_full()
                .mt(12.0)
                .px(4.0)
                .gap(6.0)
                .child(row().gap(12.0).child(dt("Sounds by")).child(link(
                    manifest.author.clone(),
                    Some(manifest.source.clone()),
                    "pack-source",
                )))
                .child(row().gap(12.0).child(dt("Licence")).child(link(
                    license_label(manifest.license).to_owned(),
                    license_url(manifest.license).map(str::to_owned),
                    "pack-licence",
                )));
            if let Some(a) = manifest.attribution.as_ref().filter(|a| !a.is_empty()) {
                dl = dl.child(
                    row().gap(12.0).child(dt("Credit")).child(
                        text(a.clone())
                            .font(ui(12.0, 400.0))
                            .color(th.fg.alpha(0.85))
                            .wrap()
                            .grow(1.0)
                            .min_w(0.0),
                    ),
                );
            }
            about = about.child(dl);
        }
        PackItem::Builtin { .. } => {
            about = about.child(
                text("Inspired by the machine's sound; not affiliated with or endorsed by its maker.")
                    .font(ui(11.5, 400.0).lh(18.7))
                    .color(th.muted_fg)
                    .wrap()
                    .mt(8.0)
                    .px(4.0),
            );
        }
    }

    col()
        .key("pack-detail")
        .label(format!("{} details", item.name()))
        .w(380.0)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 0.0, 1.0], th.border)
        .child(head)
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(col().w_full().child(kits_sec).child(sounds).child(about))
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
}
