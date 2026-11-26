//! Track I/O controls of the Song view (Rust-only: the Electron app had no audio input and no
//! plugin hosting): an audio track's input select, the Track FX drawer's plugin list, and the
//! plugin picker.

use mdaw_model::{InstrumentType, Track};
use mdaw_plugin_host::PluginKind;
use rune::{Align, El, col, el, icon, row, spacer, text};

use crate::app::{App, Menu};
use crate::icons;
use crate::icons_io;
use crate::song::SongMenu;
use crate::theme::{R_LG, R_MD, daw_label, t, ui};
use crate::ui::widgets::{Button, Size, Variant, popover, popover_frame, select_option};

/// The input choices of an audio track for a device with `channels` inputs: every channel
/// alone, then the stereo pairs (1/2, 3/4, …).
pub fn input_choices(channels: usize) -> Vec<(u32, bool)> {
    let n = channels.max(1) as u32;
    (0..n)
        .map(|c| (c, false))
        .chain((0..n / 2).map(|p| (p * 2, true)))
        .collect()
}

/// An audio track's input select trigger (row 2 of its header): "In 1", "In 3/4".
pub fn input_trigger(s: &App, track: &Track) -> El {
    let th = t();
    let open = s.menu
        == Some(Menu::Song(SongMenu::Input {
            track: track.id.clone(),
        }));
    let armed = s.svc.engine.doc().is_track_armed(&track.id);
    let id = track.id.clone();
    let failed = armed && s.svc.audio_io.error.is_some();
    let ink = if failed {
        th.destructive
    } else if open || armed {
        th.fg
    } else {
        th.muted_fg
    };
    let help = match (&s.svc.audio_io.error, armed) {
        (Some(e), true) => format!("Audio input unavailable: {e}"),
        _ => "Audio input: what this track hears and records while armed".to_owned(),
    };
    let mut tr = row()
        .key(format!("input-{}", track.id))
        .role("combobox")
        .label("Audio input")
        .help(help)
        .h(16.0)
        .ml(-4.0)
        .px(4.0)
        .items_center()
        .gap(4.0)
        .radius(3.0)
        .hover(th.surface3)
        .focusable(true)
        .child(icon(&icons_io::MIC, 12.0).color(ink))
        .child(
            text(track.input_or_default().label())
                .font(ui(11.0, 400.0))
                .color(ink),
        )
        .child(icon(&icons::CHEVRON_DOWN, 12.0).color(th.muted_fg.alpha(0.6)))
        .on_press(move |s: &mut App| {
            s.refresh_audio_devices();
            s.toggle_menu(Menu::Song(SongMenu::Input { track: id.clone() }));
        });
    if open {
        tr = tr.fill(th.surface3);
    }
    row().h(16.0).items_center().child(tr)
}

/// The input select's list (under its trigger).
pub fn input_menu(s: &App, track_id: &str) -> El {
    let th = t();
    let Some(track) = s.find_track(track_id) else {
        return el();
    };
    let current = track.input_or_default();
    let device = s
        .svc
        .audio_io
        .info
        .as_ref()
        .map(|i| i.device_name.clone())
        .or_else(|| {
            s.p9.audio_devices
                .input(&s.p9.settings.audio)
                .map(|d| d.name.clone())
        });
    let mut list = popover_frame(None, 4.0)
        .key("input-menu")
        .role("listbox")
        .min_w(168.0);
    list = list.child(
        text(device.unwrap_or_else(|| "Audio input".to_owned()))
            .font(daw_label(10.5))
            .text_case(rune::TextCase::Uppercase)
            .color(th.muted_fg)
            .px(8.0)
            .pt(6.0)
            .pb(4.0)
            .truncate()
            .max_w(220.0),
    );
    for (channel, stereo) in input_choices(s.input_channel_count()) {
        let sel = current.channel == channel && current.stereo == stereo;
        let label = mdaw_model::TrackInput { channel, stereo }.label();
        let tid = track_id.to_owned();
        list = list.child(
            select_option(
                format!("input-opt-{channel}-{}", if stereo { "st" } else { "m" }),
                row()
                    .child(text(label).font(ui(12.0, 400.0)).color(if sel {
                        th.primary
                    } else {
                        th.fg
                    }))
                    .child(spacer())
                    .child(
                        text(if stereo { "stereo" } else { "mono" })
                            .font(ui(11.0, 400.0))
                            .color(th.muted_fg),
                    ),
                sel,
            )
            .on_press(move |s: &mut App| {
                s.set_track_input(&tid, channel, stereo);
                s.menu = None;
            }),
        );
    }
    popover(&format!("input-{track_id}"), Align::Start, list)
}

/// One insert plugin's row: name (opens the editor), bypass, remove.
fn plugin_row(s: &App, track: &Track, p: &mdaw_model::PluginSlot) -> El {
    let th = t();
    let m = s.svc.plugins.0.borrow();
    let missing = !m.is_installed(p);
    let error = m.error(&p.id).map(str::to_owned);
    let editing = m.editor_open(&p.id);
    drop(m);
    let (tid, pid) = (track.id.clone(), p.id.clone());
    let (tid2, pid2) = (tid.clone(), pid.clone());
    let pid3 = pid.clone();
    let off = p.bypassed || missing || error.is_some();
    let note = if missing {
        Some("Not installed".to_owned())
    } else {
        error
    };
    let mut name = col().min_w(0.0).grow(1.0).child(
        text(p.name.clone())
            .font(ui(12.0, 600.0))
            .color(if off { th.muted_fg } else { th.fg })
            .truncate(),
    );
    name = name.child(
        text(note.clone().unwrap_or_else(|| p.vendor.clone()))
            .font(ui(10.5, 400.0))
            .color(if note.is_some() {
                th.destructive
            } else {
                th.muted_fg
            })
            .truncate(),
    );
    let open = row()
        .key(format!("plugin-open-{}", p.id))
        .role("button")
        .label(format!("Open {}", p.name))
        .help("Open the plugin's window")
        .grow(1.0)
        .min_w(0.0)
        .h(36.0)
        .items_center()
        .gap(8.0)
        .px(8.0)
        .radius(R_MD)
        .hover(th.surface3)
        .child(icon(&icons_io::PLUG, 14.0).color(if editing { th.primary } else { th.muted_fg }))
        .child(name)
        .on_press(move |s: &mut App| s.open_plugin_editor(&pid3));
    let mut bypass = Button::new(Variant::Ghost, Size::IconXs)
        .icon(&icons_io::POWER)
        .icon_size(13.0)
        .a11y(if p.bypassed {
            "Turn the plugin on"
        } else {
            "Bypass the plugin"
        })
        .tip(if p.bypassed { "Turn on" } else { "Bypass" }, None)
        .pressed(!p.bypassed)
        .key(format!("plugin-bypass-{}", p.id));
    if !p.bypassed {
        bypass = bypass.ink(th.primary).icon_ink(th.primary);
    }
    row()
        .key(format!("plugin-{}", p.id))
        .w_full()
        .items_center()
        .gap(2.0)
        .child(open)
        .child(
            bypass
                .build(move |s: &mut App, _| s.toggle_plugin_bypass(&tid, &pid))
                .size(24.0, 24.0),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::TRASH_2)
                .icon_size(13.0)
                .a11y("Remove the plugin")
                .tip("Remove", None)
                .key(format!("plugin-remove-{}", p.id))
                .build(move |s: &mut App, _| s.remove_track_plugin(&tid2, &pid2))
                .size(24.0, 24.0),
        )
}

/// A plugin's slot in the Track FX chain: the chain's drag handle and its row (open, bypass,
/// remove).
pub fn plugin_slot(
    s: &App,
    track: &Track,
    p: &mdaw_model::PluginSlot,
    grip: El,
    dragged: bool,
) -> El {
    let th = t();
    row()
        .key(format!("fx-plugin-{}", p.id))
        .w_full()
        .items_center()
        .gap(2.0)
        .radius(R_LG)
        .padding(3.0)
        .fill(th.surface2)
        .border(
            1.0,
            if dragged {
                th.primary
            } else if p.bypassed {
                th.border
            } else {
                th.input
            },
        )
        .child(grip)
        .child(el().grow(1.0).min_w(0.0).child(plugin_row(s, track, p)))
}

/// The chain's "Add plugin" button (its list: [`add_plugin_menu`]).
pub fn add_plugin_button(s: &App, track: &Track) -> El {
    let th = t();
    let open = s.menu
        == Some(Menu::Song(SongMenu::AddPlugin {
            track: track.id.clone(),
        }));
    let id = track.id.clone();
    row()
        .key(format!("plugin-add-{}", track.id))
        .role("button")
        .label("Add plugin")
        .help("Add an Audio Unit effect to the end of the chain")
        .grow(1.0)
        .h(32.0)
        .center()
        .gap(6.0)
        .radius(R_LG)
        .border(1.0, if open { th.input } else { th.border })
        .focusable(true)
        .hover(th.surface3)
        .expanded(open)
        .child(icon(&icons_io::PLUG, 14.0).color(th.primary))
        .child(text("Add plugin").font(ui(12.0, 600.0)).color(th.fg))
        .on_press(move |s: &mut App| {
            s.toggle_menu(Menu::Song(SongMenu::AddPlugin { track: id.clone() }))
        })
}

/// An audio track without a plugin: what plugins are for.
pub fn plugins_hint(track: &Track) -> Option<El> {
    (track.plugins.is_empty() && track.instrument.kind == InstrumentType::Audio).then(|| {
        text("Add an amp or effect plugin: an armed track's input plays through it.")
            .key("plugins-hint")
            .font(ui(11.0, 400.0).lh(16.0))
            .color(t().muted_fg)
            .wrap()
            .px(4.0)
            .pt(6.0)
    })
}

/// The "Add plugin" list: the installed effects by vendor.
pub fn add_plugin_menu(s: &App, track_id: &str) -> El {
    let th = t();
    let m = s.svc.plugins.0.borrow();
    let effects = m.of_kind(PluginKind::Effect);
    let mut list = col().w_full().padding(4.0);
    let mut vendor: Option<&str> = None;
    for (i, p) in effects.iter().enumerate() {
        if vendor != Some(p.vendor.as_str()) {
            vendor = Some(p.vendor.as_str());
            list = list.child(
                text(if p.vendor.is_empty() {
                    "Other".to_owned()
                } else {
                    p.vendor.clone()
                })
                .font(daw_label(10.5))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .px(8.0)
                .pt(8.0)
                .pb(4.0),
            );
        }
        let tid = track_id.to_owned();
        list = list.child(
            select_option(
                format!("plugin-choice-{i}"),
                row().child(text(p.name.clone()).font(ui(13.0, 400.0)).color(th.fg)),
                false,
            )
            .on_press(move |s: &mut App| {
                s.menu = None;
                s.add_track_plugin(&tid, i);
            }),
        );
    }
    if effects.is_empty() {
        list = list.child(
            text("No Audio Unit effects are installed.")
                .key("plugin-empty")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg)
                .px(8.0)
                .py(16.0),
        );
    }
    let content = popover_frame(Some(260.0), 0.0)
        .key("add-plugin-menu")
        .role("listbox")
        .child(
            col()
                .w_full()
                .max_h(320.0)
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    popover(&format!("plugin-add-{track_id}"), Align::End, content)
}

#[cfg(test)]
mod tests {
    use super::input_choices;

    #[test]
    fn inputs_are_every_channel_then_the_stereo_pairs() {
        assert_eq!(input_choices(1), vec![(0, false)]);
        assert_eq!(
            input_choices(4),
            vec![
                (0, false),
                (1, false),
                (2, false),
                (3, false),
                (0, true),
                (2, true)
            ]
        );
        assert_eq!(input_choices(0), vec![(0, false)]);
        assert_eq!(input_choices(3).iter().filter(|c| c.1).count(), 1);
    }
}
