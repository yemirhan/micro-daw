//! `MissingPacks.tsx` (strip, `GetPackButton`, `UnavailablePackActions`),
//! `PendingLoadChip.tsx`, the `InstallButton` / `ProgressRing` they use, and the pure parts of
//! `usePackAvailability.ts` (`missingPacks`, `missingPackTitles`).

use mdaw_model::beat_helpers::{pad_label, pending_load_text};
use mdaw_model::kit::pack_id_of_source;
use mdaw_model::packs::PackInstallState;
use mdaw_model::packs::model::{
    InstallButtonState, download_progress, is_paused_install, pack_button_state,
};
use mdaw_model::types::PadSettings;
use rune::{El, LineCap, Shape, Tx, canvas, col, el, icon, row, text};

use crate::actions::MissingPadFix;
use crate::app::App;
use crate::icons;
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::widgets::{Button, Size, Variant};

/// `MissingPack`: a pack some pads need that is not installed.
#[derive(Debug, Clone, PartialEq)]
pub struct MissingPack {
    pub id: String,
    pub name: String,
    pub pads: Vec<usize>,
    pub state: PackInstallState,
    /// False once the loaded catalog no longer lists the pack.
    pub available: bool,
}

/// `missingPacks(pads, avail)`: in pad order, grouped by pack. "Missing" only once installed
/// packs are known (`loaded && !installed`).
pub fn missing_packs(s: &App, pads: &[PadSettings]) -> Vec<MissingPack> {
    let snap = s.svc.packs.snapshot();
    let loaded = snap.store.installed_stamps.is_some();
    let mut out: Vec<MissingPack> = Vec::new();
    if !loaded {
        return out;
    }
    let dir = snap.packs_dir.as_deref();
    for (i, p) in pads.iter().enumerate() {
        // `packIdOfSource`: only `{ kind: 'pack' }` sources (the TS reads no folder paths here).
        let Some(id) = p.sample.as_ref().and_then(|x| match &x.source {
            mdaw_model::types::PadSampleSource::Pack { .. } => pack_id_of_source(&x.source, dir),
            _ => None,
        }) else {
            continue;
        };
        if snap.store.is_installed(&id) {
            continue;
        }
        if let Some(m) = out.iter_mut().find(|m| m.id == id) {
            m.pads.push(i);
        } else {
            out.push(MissingPack {
                name: s.svc.packs.pack_name(&id),
                state: snap
                    .store
                    .install_states
                    .get(&id)
                    .cloned()
                    .unwrap_or(PackInstallState::Idle),
                available: !snap.store.is_unavailable(&id),
                pads: vec![i],
                id,
            });
        }
    }
    out
}

/// `missingPackTitles(missing, n)`.
pub fn missing_pack_titles(missing: &[MissingPack], n: usize) -> Vec<Option<String>> {
    let mut out = vec![None; n];
    for m in missing {
        let title = if m.available {
            format!("Pack not installed: {}", m.name)
        } else {
            format!("Pack no longer available: {}", m.name)
        };
        for &p in &m.pads {
            if p < n {
                out[p] = Some(title.clone());
            }
        }
    }
    out
}

/// `ProgressRing` (indeterminate: a 30% arc).
pub fn progress_ring(progress: Option<f64>, size: f32, label: &str) -> El {
    let th = t();
    let r = (size - 3.0) / 2.0;
    let c = size / 2.0;
    let ring = |from: f32, to: f32| -> Vec<[f32; 2]> {
        let n = 48;
        (0..=n)
            .map(|i| {
                let a = (from + (to - from) * i as f32 / n as f32 - 0.25) * std::f32::consts::TAU;
                [c + r * a.cos(), c + r * a.sin()]
            })
            .collect()
    };
    let p = progress.map_or(0.3, |p| p as f32);
    let mut shape = Shape::new().stroke_with(ring(0.0, 1.0), true, 2.5, LineCap::Butt, th.surface4);
    if p > 0.0 {
        shape = shape.stroke_with(ring(0.0, p), false, 2.5, LineCap::Round, th.primary);
    }
    let mut ring = canvas(shape)
        .size(size, size)
        .role("progressbar")
        .label(label.to_owned());
    // aria-valuemin 0, -max 100, -now = round(progress × 100) (none while indeterminate).
    if let Some(p) = progress {
        ring = ring.range_value((p * 100.0).round(), 0.0, 100.0);
    }
    ring
}

/// `InstallButton` for `packButtonState(state, false)`.
pub fn install_button(state: InstallButtonState, name: &str, id: &str) -> El {
    let th = t();
    let id_get = id.to_owned();
    let id_cancel = id.to_owned();
    match state {
        InstallButtonState::Builtin => row()
            .h(28.0)
            .px(6.0)
            .items_center()
            .gap(4.0)
            .child(icon(&icons::ZAP, 14.0).color(th.primary))
            .child(text("Ready").font(ui(11.0, 500.0)).color(th.muted_fg)),
        InstallButtonState::Get => Button::new(Variant::Primary, Size::Sm)
            .icon(&icons::DOWNLOAD)
            .label("Get")
            .a11y(format!("Get {name}"))
            .key(format!("get-{id}"))
            .build(move |s: &mut App, _| s.get_pack(&id_get)),
        InstallButtonState::Downloading { progress, label } => row()
            .key(format!("downloading-{id}"))
            .h(28.0)
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
                    .key(format!("cancel-{id}"))
                    .role("button")
                    .label(format!("Cancel downloading {name}"))
                    .help("Cancel download")
                    .h_full()
                    .px(6.0)
                    .items_center()
                    .border_sides([0.0, 0.0, 0.0, 1.0], th.border)
                    .hover(th.surface3)
                    .child(icon(&icons::X, 14.0).color(th.muted_fg))
                    .on_press(move |s: &mut App| s.svc.packs.cancel(&id_cancel)),
            ),
        InstallButtonState::Installed => row()
            .h(28.0)
            .px(6.0)
            .items_center()
            .gap(4.0)
            .child(icon(&icons::CHECK, 14.0).color(th.play))
            .child(text("Installed").font(ui(11.0, 500.0)).color(th.play)),
        InstallButtonState::Resume => Button::new(Variant::Outline, Size::Sm)
            .icon(&icons::DOWNLOAD)
            .label("Resume")
            .a11y(format!("Resume downloading {name}"))
            .tip("Download paused: continues where it stopped", None)
            .build(move |s: &mut App, _| s.get_pack(&id_get)),
        InstallButtonState::Error { message } => Button::new(Variant::Outline, Size::Sm)
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
            .build(move |s: &mut App, _| s.get_pack(&id_get)),
    }
}

/// `GetPackButton`.
pub fn get_pack_button(_s: &App, m: &MissingPack) -> El {
    install_button(pack_button_state(Some(&m.state), false), &m.name, &m.id)
}

/// `UnavailablePackActions`: Clear pads / Replace with built-in.
pub fn unavailable_actions(m: &MissingPack) -> El {
    let pads = if m.pads.len() == 1 {
        "this pad".to_owned()
    } else {
        format!("these {} pads", m.pads.len())
    };
    let (p1, p2) = (m.pads.clone(), m.pads.clone());
    row()
        .items_center()
        .gap(4.0)
        .child(
            Button::new(Variant::Ghost, Size::Xs)
                .icon(&icons::ERASER)
                .label("Clear pads")
                .a11y(format!("Clear the pads that use {}", m.name))
                .tip(format!("Empty {pads}"), None)
                .key(format!("clear-pads-{}", m.id))
                .build(move |s: &mut App, _| s.fix_missing(p1.clone(), MissingPadFix::Clear)),
        )
        .child(
            Button::new(Variant::Outline, Size::Xs)
                .icon(&icons::REPLACE)
                .label("Replace with built-in")
                .a11y(format!(
                    "Replace the pads that use {} with built-in sounds",
                    m.name
                ))
                .tip(format!("Put the built-in kit's sounds on {pads}"), None)
                .key(format!("replace-pads-{}", m.id))
                .build(move |s: &mut App, _| s.fix_missing(p2.clone(), MissingPadFix::Builtin)),
        )
}

/// `MissingPacksStrip`.
pub fn strip(s: &App, missing: &[MissingPack], can_fix: bool) -> Option<El> {
    if missing.is_empty() {
        return None;
    }
    let th = t();
    let mut col_el = col()
        .key("missing-packs")
        .role("status")
        .label("Packs not installed")
        .gap(4.0)
        .mb(8.0);
    for m in missing {
        let error = if m.available
            && matches!(&m.state, PackInstallState::Error { .. })
            && !is_paused_install(Some(&m.state))
        {
            match &m.state {
                PackInstallState::Error { message } if !message.is_empty() => Some(message.clone()),
                _ => Some("Download failed".to_owned()),
            }
        } else {
            None
        };
        let pads_title = m
            .pads
            .iter()
            .map(|p| pad_label(*p))
            .collect::<Vec<_>>()
            .join(", ");
        let count = if m.pads.len() == 1 {
            "1 pad".to_owned()
        } else {
            format!("{} pads", m.pads.len())
        };
        let label = rune::rich(
            format!(
                "{}{} · {}",
                if m.available {
                    "Not installed: "
                } else {
                    "No longer available: "
                },
                m.name,
                count
            ),
            {
                let pre = if m.available {
                    "Not installed: ".len()
                } else {
                    "No longer available: ".len()
                };
                let name_end = pre + m.name.len();
                vec![
                    rune::Span::new(pre, name_end).weight(600.0),
                    rune::Span::new(name_end, name_end + " · ".len() + count.len())
                        .color(th.muted_fg),
                ]
            },
        )
        .font(ui(11.5, 400.0).lh(15.0))
        .color(th.fg.alpha(0.9))
        .help(format!("Pads {pads_title} play sounds from {}", m.name))
        .grow(1.0)
        .min_w(120.0)
        .py(2.0);
        let action = if m.available {
            Some(get_pack_button(s, m))
        } else if can_fix {
            Some(unavailable_actions(m))
        } else {
            None
        };
        let mut r = row()
            .key(format!("missing-{}", m.id))
            .w_full()
            .flex_wrap()
            .items_center()
            .gaps(2.0, 8.0)
            .py(2.0)
            .pr(2.0)
            .pl(8.0)
            .radius(R_MD)
            .border(1.0, th.warn.alpha(0.4))
            .fill(th.warn.alpha(0.10))
            .child(icon(&icons::PACKAGE_X, 14.0).color(th.warn))
            .child(label);
        if let Some(a) = action {
            r = r.child(a);
        }
        if let Some(e) = error {
            r = r.child(
                text(e)
                    .role("alert")
                    .font(ui(11.0, 400.0))
                    .color(th.rec)
                    .w_full()
                    .pl(22.0)
                    .pb(2.0),
            );
        }
        col_el = col_el.child(r);
    }
    Some(col_el)
}

/// `PendingLoadChip`: "Getting X… ✕" / "After take: X ✕" while a Get & load is pending.
pub fn pending_chip(s: &App, width: f32) -> Option<El> {
    let th = t();
    let snap = s.svc.packs.snapshot();
    let info = snap.pending_load_info.clone()?;
    let install = snap.store.install_states.get(&info.pack_id).cloned();
    let txt = pending_load_text(&info.pack_name, info.waiting_for_recording);
    let downloading = matches!(install, Some(PackInstallState::Downloading { .. }));
    let progress = install.as_ref().and_then(download_progress);
    let lead: El = if info.waiting_for_recording {
        el().size(8.0, 8.0).radius(4.0).fill(th.rec)
    } else {
        progress_ring(progress, 13.0, &format!("Downloading {}", info.pack_name))
    };
    let shown = if width >= 150.0 {
        txt.text.clone()
    } else {
        txt.name.clone()
    };
    let pack_id = info.pack_id.clone();
    let cancel = row()
        .key("pending-cancel")
        .role("button")
        .label(format!("Cancel loading {}", info.pack_name))
        .help(if info.waiting_for_recording {
            "Don't load this kit"
        } else {
            "Stop getting this kit"
        })
        .size(20.0, 20.0)
        .center()
        .radius(4.0)
        .hover(th.surface3)
        .child(icon(&icons::X, 12.0).color(th.muted_fg))
        .on_press_tx(move |s: &mut App, _tx: &mut Tx| {
            if downloading {
                s.svc.packs.cancel(&pack_id);
            } else {
                s.svc.packs.cancel_pending_load();
            }
        });
    Some(
        row()
            .key("pending-load")
            .role("status")
            .help(txt.title)
            .h(24.0)
            .min_w(96.0)
            .max_w(220.0)
            .grow(1.0)
            .items_center()
            .gap(4.0)
            .pl(4.0)
            .pr(2.0)
            .radius(R_MD)
            .border(1.0, th.primary.alpha(0.4))
            .fill(th.primary.alpha(0.10))
            .child(lead)
            .child(
                text(shown)
                    .font(ui(11.0, 400.0).lh(11.0))
                    .color(th.fg)
                    .truncate()
                    .grow(1.0)
                    .min_w(0.0),
            )
            .child(cancel),
    )
}

impl App {
    /// `packs.get(id)`: load the catalog, then install.
    pub fn get_pack(&mut self, id: &str) {
        self.svc.packs.load_catalog(false);
        self.svc.packs.install(id);
    }
}
