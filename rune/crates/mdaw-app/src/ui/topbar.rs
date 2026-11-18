//! `components/TopBar.tsx` (DAW mode, Beat view): sidebar trigger, project / save, undo / redo,
//! browser toggle, Beat | Song switch, the live-input readout (centre), MIDI input, controller
//! setup, instrument, volume, full screen.

use mdaw_engine::engine::TransportState;
use mdaw_model::notes::midi_to_note_name;
use rune::{Align, El, Phase, Tx, el, icon, row, spacer, text};

use crate::app::{App, AppMode, DawView, Menu};
use crate::icons;
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::start::mark_glyph;
use crate::ui::widgets::{
    Button, Segmented, Size, Variant, lcd, menu_row, mod_key, popover_frame, select_option,
    select_popup, vrule,
};

/// The small app mark (`AppMark`).
pub fn app_mark() -> El {
    let th = t();
    row()
        .size(20.0, 20.0)
        .center()
        .radius(5.0)
        .fill(th.primary)
        .child(mark_glyph(16.0, 4.5, th.primary_fg))
}

fn divider() -> El {
    vrule(16.0, t().border).mx(4.0)
}

/// `LiveInputReadout`: chord (or "—") | up to 8 held notes, or "Play something…".
fn live_readout(s: &App) -> El {
    let th = t();
    let notes = s.active_notes();
    let chord =
        mdaw_model::chords::detect_chord(&notes.iter().map(|n| *n as i32).collect::<Vec<_>>());
    let chord_el = row()
        .min_w(44.0)
        .px(6.0)
        .py(1.0)
        .radius(3.0)
        .justify_center()
        .child(
            text(chord.as_ref().map_or("—".to_owned(), |c| c.display.clone()))
                .font(mono(11.0, 600.0))
                .color(if chord.is_some() {
                    th.primary
                } else {
                    th.muted_fg
                }),
        );
    let chord_el = if chord.is_some() {
        chord_el.fill(th.primary.alpha(0.15))
    } else {
        chord_el
    };
    let mut notes_row = row().grow(1.0).min_w(0.0).items_center().gap(4.0);
    if notes.is_empty() {
        notes_row = notes_row.child(
            text("Play something…")
                .font(mono(11.0, 400.0))
                .color(th.muted_fg),
        );
    } else {
        for n in notes.iter().take(8) {
            notes_row = notes_row.child(
                text(midi_to_note_name(*n as f64))
                    .font(mono(11.0, 400.0))
                    .color(th.fg),
            );
        }
    }
    lcd()
        .key("live-readout")
        .h(28.0)
        .min_w(260.0)
        .items_center()
        .gap(8.0)
        .px(8.0)
        .child(chord_el)
        .child(el().w(1.0).h(14.0).fill(th.border))
        .child(notes_row)
}

/// The MIDI input select trigger (`MidiDeviceSelector`).
fn midi_select(s: &App, wide: bool) -> El {
    let th = t();
    let host = &s.svc.midi.host;
    let active = host.active_device_id().map(str::to_owned);
    let name = active
        .as_ref()
        .and_then(|id| host.devices().iter().find(|d| &d.id == id))
        .map(|d| d.name.clone());
    let connected = active.is_some();
    let dot = el()
        .absolute(11.0, 11.0)
        .size(6.0, 6.0)
        .radius(3.0)
        .fill(if connected { th.play } else { th.surface4 })
        .border(2.0, th.surface2);
    let glyph = el()
        .size(16.0, 16.0)
        .center()
        .child(icon(&icons::CABLE, 14.0).color(if connected { th.fg } else { th.muted_fg }))
        .child(dot);
    let value = match name {
        Some(n) => text(n).font(ui(12.0, 400.0)).color(th.fg).truncate(),
        None => text("No MIDI input")
            .font(ui(12.0, 400.0))
            .color(th.muted_fg),
    };
    let open = s.menu == Some(Menu::MidiDevice);
    row()
        .key("midi-select")
        .role("combobox")
        .label("MIDI input device")
        .focusable(true)
        .help(if connected {
            "MIDI input"
        } else {
            "No MIDI input connected"
        })
        .w(if wide { 168.0 } else { 140.0 })
        .h(28.0)
        .items_center()
        .gap(8.0)
        .px(8.0)
        .radius(R_MD)
        .border(1.0, if open { th.input } else { th.border })
        .fill(th.surface2)
        .hover(th.surface3)
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .items_center()
                .gap(8.0)
                .child(glyph)
                .child(value),
        )
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.6)))
        .on_press(|s: &mut App| s.toggle_menu(Menu::MidiDevice))
}

/// The open MIDI device list (Select content, `position="popper" align="end"`).
pub fn midi_menu(s: &App) -> El {
    let th = t();
    let host = &s.svc.midi.host;
    let active = host.active_device_id().map(str::to_owned);
    let mut list = popover_frame(None, 4.0).min_w(220.0);
    if host.devices().is_empty() {
        list = list.child(menu_row(
            row().child(text("No devices found").font(ui(13.0, 400.0)).color(th.fg)),
            true,
        ));
    }
    for (i, d) in host.devices().iter().enumerate() {
        let id = d.id.clone();
        let sel = active.as_deref() == Some(d.id.as_str());
        list = list.child(
            select_option(
                format!("midi-select-opt-{i}"),
                row()
                    .label(d.name.clone())
                    .child(text(d.name.clone()).font(ui(13.0, 400.0)).color(if sel {
                        th.primary
                    } else {
                        th.fg
                    }))
                    .child(spacer())
                    .child(if sel {
                        icon(&icons::CHECK, 14.0).color(th.primary)
                    } else {
                        el()
                    }),
                sel,
            )
            .on_press(move |s: &mut App| {
                s.svc.midi.host.select_device(&id);
                s.menu = None;
            }),
        );
    }
    select_popup("midi-select", Align::End, list)
}

/// The instrument trigger (`InstrumentSelector`'s button): category chip + preset name. Its
/// preset browser popover is phase 9 (synth editor / instrument selector).
fn instrument(s: &App, wide: bool, win_w: f32) -> El {
    let th = t();
    let (name, category) = (s.preset.0.clone(), s.preset.1);
    let mut left = row().min_w(0.0).items_center().gap(8.0);
    if win_w >= 1360.0 {
        left = left.child(
            row().px(4.0).py(1.0).radius(3.0).fill(th.surface4).child(
                text(category)
                    .font(ui(9.5, 700.0).tracking(9.5 * 0.06))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg),
            ),
        );
    }
    left = left.child(text(name).font(ui(12.0, 600.0)).color(th.fg).truncate());
    row()
        .key("instrument")
        .role("combobox")
        .label("Choose instrument sound")
        .w(if wide { 184.0 } else { 150.0 })
        .h(28.0)
        .items_center()
        .justify_between()
        .gap(8.0)
        .px(8.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface2)
        .hover(th.surface3)
        .child(left)
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg))
}

/// `VolumeControl`: mute + an 80-point slider (−40..0 dB, step 1) + dB readout (≥1500 px).
fn volume(s: &App, win_w: f32) -> El {
    let th = t();
    let db = s.svc.engine.get_master_volume_db();
    let muted = s.svc.midi.host.shared().lock().router.muted();
    let silent = muted || db <= mdaw_model::settings::MIN_VOLUME;
    let mute = Button::new(Variant::Ghost, Size::IconSm)
        .icon(if silent {
            &icons::VOLUME_X
        } else {
            &icons::VOLUME_2
        })
        .a11y(if muted { "Unmute" } else { "Mute" })
        .tip(
            if muted {
                "Unmute output"
            } else {
                "Mute output"
            },
            None,
        )
        .pressed(muted)
        .key("mute");
    let mute = if muted {
        mute.ink(th.rec).icon_ink(th.rec)
    } else {
        mute
    };
    let mute = mute.build(|s: &mut App, _| s.toggle_mute());
    let t01 = ((db - mdaw_model::settings::MIN_VOLUME)
        / (mdaw_model::settings::MAX_VOLUME - mdaw_model::settings::MIN_VOLUME))
        .clamp(0.0, 1.0) as f32;
    let w = 80.0;
    let track = el()
        .absolute(0.0, 6.0)
        .size(w, 4.0)
        .radius(2.0)
        .fill(th.surface4)
        .child(
            el().absolute(0.0, 0.0)
                .size(w * t01, 4.0)
                .radius(2.0)
                .fill(th.primary),
        );
    let thumb = el()
        .absolute((w * t01 - 8.0).max(0.0).min(w - 16.0), 0.0)
        .size(16.0, 16.0)
        .radius(8.0)
        .fill(th.fg)
        .border(1.0, th.primary);
    let slider = el()
        .key("volume")
        .role("slider")
        .label("Output volume")
        .range_value(
            db.round(),
            mdaw_model::settings::MIN_VOLUME,
            mdaw_model::settings::MAX_VOLUME,
        )
        .range_step(1.0)
        .a11y_value(format!("{} dB", fmt_db_int(db)))
        .help(format!("Output {} dB", fmt_db_int(db)))
        .size(w, 16.0)
        .opacity(if muted { 0.5 } else { 1.0 })
        .child(track)
        .child(thumb)
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up {
                let db = s.svc.engine.get_master_volume_db();
                s.with_settings_mut(|st| {
                    let _ = mdaw_model::settings::write_master_volume(st, db);
                });
                return;
            }
            let f = (d.x / d.w.max(1.0)).clamp(0.0, 1.0) as f64;
            let db = (mdaw_model::settings::MIN_VOLUME
                + f * (mdaw_model::settings::MAX_VOLUME - mdaw_model::settings::MIN_VOLUME))
                .round();
            s.svc.engine.set_master_volume_db(db);
        })
        // Radix Slider keys: arrows ±1 dB, Page Up / Down ±10, Home / End.
        .on_step(|s: &mut App, delta: i32, _tx: &mut Tx| {
            let db = s.svc.engine.get_master_volume_db();
            s.set_output_volume(db.round() + delta as f64);
        })
        .on_step_limit(|s: &mut App, max: bool, _tx: &mut Tx| {
            s.set_output_volume(if max {
                mdaw_model::settings::MAX_VOLUME
            } else {
                mdaw_model::settings::MIN_VOLUME
            });
        });
    let mut r = row().items_center().gap(6.0).child(mute).child(slider);
    if win_w >= 1500.0 {
        r = r.child(
            text(format!("{} dB", fmt_db_int(db)))
                .font(mono(11.0, 400.0))
                .color(th.muted_fg)
                .w(46.0)
                .align(Align::End),
        );
    }
    r
}

impl App {
    /// The output volume (clamped to −40..0 dB, whole dB) and its preference.
    pub fn set_output_volume(&mut self, db: f64) {
        let db = db
            .clamp(
                mdaw_model::settings::MIN_VOLUME,
                mdaw_model::settings::MAX_VOLUME,
            )
            .round();
        self.svc.engine.set_master_volume_db(db);
        self.with_settings_mut(|st| {
            let _ = mdaw_model::settings::write_master_volume(st, db);
        });
    }
}

fn fmt_db_int(db: f64) -> String {
    let v = db.round() as i64;
    if v > 0 {
        format!("+{v}")
    } else {
        format!("{v}")
    }
}

pub fn view(s: &App, win_w: f32, inset: f32) -> El {
    let th = t();
    let m = mod_key();
    let daw = s.mode == AppMode::Daw;
    let um = s.svc.engine.doc().undo_manager();
    let mut left = row().min_w(0.0).items_center().gap(4.0).no_window_drag();
    left = left.child(
        Button::new(Variant::Ghost, Size::IconSm)
            .icon(&icons::PANEL_LEFT)
            .a11y("Toggle Sidebar")
            .key("sidebar-trigger")
            .build(|s: &mut App, _| s.sidebar_open = !s.sidebar_open),
    );
    left = left.child(divider());
    let name_w = if win_w >= 1280.0 { 180.0 } else { 120.0 };
    let dirty = s.dirty();
    let project = row()
        .key("project")
        .role("button")
        .label("Save project")
        .help(if dirty {
            "Unsaved changes. Save"
        } else {
            "Save project"
        })
        .help_keys([format!("{m}S")])
        .h(28.0)
        .min_w(0.0)
        .items_center()
        .gap(8.0)
        .px(6.0)
        .radius(R_MD)
        .hover(th.surface3)
        .child(app_mark())
        .child(
            text("Untitled")
                .font(ui(13.0, 600.0))
                .color(th.fg)
                .truncate()
                .max_w(name_w),
        );
    let project = if dirty {
        project.child(
            el().key("dirty-dot")
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(th.warn)
                .label("Unsaved changes"),
        )
    } else {
        project
    };
    left = left.child(project);
    if daw {
        left = left.child(
            row()
                .ml(4.0)
                .items_center()
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::UNDO_2)
                        .a11y("Undo")
                        .tip("Undo", Some(&format!("{m}Z")))
                        .disabled(!um.can_undo())
                        .key("undo")
                        .build(|s: &mut App, _| s.undo()),
                )
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::REDO_2)
                        .a11y("Redo")
                        .tip("Redo", Some(&format!("{m}⇧Z")))
                        .disabled(!um.can_redo())
                        .key("redo")
                        .build(|s: &mut App, _| s.redo()),
                ),
        );
        left = left.child(divider());
        left = left.child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::PANEL_LEFT)
                .a11y("Sample browser")
                .tip(
                    if s.browser_open {
                        "Hide sample browser"
                    } else {
                        "Show sample browser"
                    },
                    Some("B"),
                )
                .pressed(s.browser_open)
                .key("browser-toggle")
                .build(|s: &mut App, _| s.toggle_browser()),
        );
        left = left.child(
            Segmented::new(
                "View",
                s.view,
                vec![
                    (DawView::Beat, "Beat".into()),
                    (DawView::Song, "Song".into()),
                ],
            )
            .sans()
            .size(12.0)
            .weight(600.0, 600.0)
            .item_px(10.0)
            .key("view-switch")
            .build(|s: &mut App, v| s.view = v)
            .ml(4.0)
            .help("Switch Beat / Song view")
            .help_keys(["Tab"]),
        );
    }
    let centre = row()
        .min_w(0.0)
        .grow(1.0)
        .justify_center()
        .items_center()
        .no_window_drag()
        .child(if daw && s.view == DawView::Song {
            song_transport_placeholder(s)
        } else {
            live_readout(s)
        });
    let mut right = row().items_center().justify_end().gap(8.0).no_window_drag();
    if win_w >= 1060.0 {
        right = right.child(midi_select(s, win_w >= 1500.0));
    }
    right = right
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::SLIDERS_HORIZONTAL)
                .a11y("Controller setup")
                .tip("Controller setup: map pads and knobs", None)
                .key("controller-setup")
                .build(|_s: &mut App, _| {}),
        )
        .child(
            row()
                .items_center()
                .gap(4.0)
                .child(instrument(s, win_w >= 1500.0, win_w))
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::SLIDERS_HORIZONTAL)
                        .a11y("Synth editor")
                        .tip("Edit the synth sound", None)
                        .build(|_s: &mut App, _| {}),
                ),
        );
    if win_w >= 1180.0 {
        right = right.child(vrule(16.0, th.border)).child(volume(s, win_w));
    }
    right = right.child(
        Button::new(Variant::Ghost, Size::IconSm)
            .icon(&icons::MAXIMIZE_2)
            .a11y("Fullscreen")
            .tip("Full screen", None)
            .key("fullscreen")
            .build(|_s: &mut App, tx: &mut Tx| tx.toggle_full_screen()),
    );
    row()
        .key("topbar")
        .h(44.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .pl(if inset > 0.0 { inset } else { 8.0 })
        .pr(8.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .window_drag()
        .child(left)
        .child(centre)
        .child(right)
}

/// The Song view's transport sits in the centre in Song view (phase 8).
fn song_transport_placeholder(s: &App) -> El {
    let th = t();
    let label = match s.transport() {
        TransportState::Stopped => "Song transport (phase 8)",
        _ => "Playing",
    };
    lcd()
        .h(28.0)
        .px(8.0)
        .items_center()
        .child(text(label).font(mono(11.0, 400.0)).color(th.muted_fg))
}
