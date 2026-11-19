//! `components/TopBar.tsx` (DAW mode, Beat view): sidebar trigger, project / save, undo / redo,
//! browser toggle, Beat | Song switch, the live-input readout (centre), MIDI input, controller
//! setup, instrument, volume, full screen.

use mdaw_model::notes::midi_to_note_name;
use rune::{Align, El, Phase, Tx, el, icon, row, text};

use crate::app::{App, AppMode, DawView};
use crate::icons;
use crate::theme::{R_MD, mono, t, ui};
use crate::ui::start::mark_glyph;
use crate::ui::widgets::{Button, Segmented, Size, Variant, lcd, mod_key, vrule};

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

/// The instrument trigger (`InstrumentSelector`'s button): category chip + preset name; it
/// opens the preset browser popover (phase 9, `ui::p9::rack::instrument_menu`).
fn instrument(s: &App, wide: bool, win_w: f32) -> El {
    let th = t();
    let (name, category) = (s.preset.0.clone(), s.preset.1);
    // `flex min-w-0` (shrinks: Rune's flex items don't by default, which pushed the chevron
    // out of the pill) around a `shrink-0` chip and a `truncate` name.
    let mut left = row().shrink().items_center().gap(8.0);
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
    left = left.child(
        text(name)
            .font(ui(12.0, 600.0))
            .color(th.fg)
            .truncate()
            .shrink(),
    );
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
        .focusable(true)
        .child(left)
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg))
        .on_press(|s: &mut App| {
            // Each opening starts with an empty search and the first result highlighted.
            s.p9.instrument_search = rune::TextEdit::new("");
            s.p9.instrument_active = 0;
            s.toggle_p9_menu(crate::views::P9Menu::Instrument)
        })
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

/// The topbar's height: also the window's titlebar band (`crate::titlebar`), so macOS centres
/// the traffic lights in this row.
pub const HEIGHT: f32 = 44.0;

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
        .on_press_tx(|s: &mut App, tx: &mut Tx| s.save_project(tx))
        .child(app_mark())
        .child(
            text(s.project_name())
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
    // The centre column grows to fill the row: only its content keeps its presses, so the
    // empty space around it moves the window and a double click there zooms (or minimises)
    // it as the system's titlebar setting says, like the rest of the bare band.
    let centre = row()
        .key("topbar-centre")
        .min_w(0.0)
        .grow(1.0)
        .justify_center()
        .items_center()
        .child(
            if daw && s.view == DawView::Song {
                crate::ui::song::transport::view(s, win_w)
            } else {
                live_readout(s)
            }
            .no_window_drag(),
        );
    // No MIDI device select: every connected MIDI input plays (the armed tracks, else the live
    // sound picked here).
    let mut right = row().items_center().justify_end().gap(8.0).no_window_drag();
    right = right
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::SLIDERS_HORIZONTAL)
                .a11y("Controller setup")
                .tip("Controller setup: map pads and knobs", None)
                .key("controller-setup")
                .build(|s: &mut App, _| s.open_dialog(crate::views::Dialog::Controller)),
        )
        .child(
            row()
                .items_center()
                .gap(4.0)
                .child(instrument(s, win_w >= 1500.0, win_w))
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::SLIDERS_HORIZONTAL)
                        .a11y("Toggle synth editor")
                        .tip(
                            if s.p9.synth_editor {
                                "Hide synth editor"
                            } else {
                                "Edit sound"
                            },
                            None,
                        )
                        .pressed(s.p9.synth_editor)
                        .key("synth-editor-toggle")
                        .build(|s: &mut App, _| s.p9.synth_editor = !s.p9.synth_editor),
                ),
        );
    if win_w >= 1180.0 {
        right = right.child(vrule(16.0, th.border)).child(volume(s, win_w));
    }
    // `isFullscreen` (`fullscreen-changed`): a macOS window shows no traffic lights over the
    // content only in full screen.
    let full =
        s.svc.mode == crate::services::RunMode::Window && cfg!(target_os = "macos") && inset == 0.0;
    right = right.child(
        Button::new(Variant::Ghost, Size::IconSm)
            .icon(if full {
                &icons::MINIMIZE_2
            } else {
                &icons::MAXIMIZE_2
            })
            .a11y(if full {
                "Exit fullscreen"
            } else {
                "Fullscreen"
            })
            .tip(
                if full {
                    "Exit full screen"
                } else {
                    "Full screen"
                },
                None,
            )
            .key("fullscreen")
            .build(|_s: &mut App, tx: &mut Tx| tx.toggle_full_screen()),
    );
    row()
        .key("topbar")
        .h(HEIGHT)
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
