//! `components/dev/DevView.tsx`: the MIDI monitor (device chip, type filter, pause / resume,
//! clear, the event table, the footer), fed by `mdaw-midi`'s monitor buffer
//! (`useMidiMonitor`: clock / active sensing dropped, at most 500 kept, paused drops input).

use mdaw_midi::monitor::{MonitorEvent, MonitorFilter};
use rune::{Align, Color, El, col, el, icon, row, spacer, text};

use crate::app::App;
use crate::icons;
use crate::icons_views;
use crate::theme::{R_MD, daw_label, mono, t, ui};
use crate::ui::widgets::{Button, Size, Variant, popover_frame, select_option, select_popup};
use crate::views::P9Menu;

const FILTERS: [(MonitorFilter, &str); 7] = [
    (MonitorFilter::All, "All messages"),
    (MonitorFilter::NoteOn, "Note On"),
    (MonitorFilter::NoteOff, "Note Off"),
    (MonitorFilter::ControlChange, "Control Change"),
    (MonitorFilter::PitchBend, "Pitch Bend"),
    (MonitorFilter::Aftertouch, "Aftertouch"),
    (MonitorFilter::ProgramChange, "Program Change"),
];

/// Column widths (`grid-cols-[6rem_8.5rem_3rem_minmax(0,1fr)_9rem] gap-3`).
const COLS: [f32; 5] = [96.0, 136.0, 48.0, 0.0, 144.0];

fn type_color(kind: &str) -> Color {
    let th = t();
    match kind {
        "noteon" => th.play,
        "noteoff" => th.rec,
        "controlchange" => th.select,
        "pitchbend" => th.warn,
        "aftertouch" => rune::oklch(0.74, 0.13, 300.0),
        "programchange" => th.primary,
        _ => th.muted_fg,
    }
}

/// `formatTimestamp`: seconds mod 1000, 3 decimals, padded to 8.
pub fn format_timestamp(ms: f64) -> String {
    let s = (ms / 1000.0) % 1000.0;
    format!("{:>8}", format!("{s:.3}"))
}

/// `formatHex`.
pub fn format_hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn grid_row(cells: [El; 5], h: f32) -> El {
    let mut r = row().w_full().h(h).items_center().gap(12.0).px(16.0);
    for (i, c) in cells.into_iter().enumerate() {
        r = r.child(if COLS[i] > 0.0 {
            c.w(COLS[i])
        } else {
            c.grow(1.0).min_w(0.0)
        });
    }
    r
}

fn event_row(e: &MonitorEvent) -> El {
    let th = t();
    let c = type_color(&e.kind);
    grid_row(
        [
            // HTML collapses the padStart spaces of the cell.
            text(format_timestamp(e.timestamp).trim_start().to_owned())
                .font(mono(12.0, 400.0))
                .color(th.muted_fg),
            row()
                .items_center()
                .gap(8.0)
                .child(el().size(6.0, 6.0).radius(3.0).fill(c))
                .child(text(e.label.clone()).font(mono(12.0, 500.0)).color(c)),
            text(e.channel.map_or("-".to_owned(), |c| c.to_string()))
                .font(mono(12.0, 400.0))
                .color(th.muted_fg)
                .align(Align::Center),
            text(e.detail.clone())
                .font(mono(12.0, 400.0))
                .color(th.fg.alpha(0.9))
                .truncate(),
            text(format_hex(&e.raw_data))
                .font(mono(12.0, 400.0))
                .color(th.muted_fg.alpha(0.7)),
        ],
        24.0,
    )
    .role("row")
    .hover(th.surface2)
}

/// The filter list.
pub fn filter_menu(s: &App) -> El {
    let cur = s.p9.monitor.filter;
    let mut list = popover_frame(Some(150.0), 4.0);
    for (f, l) in FILTERS {
        list = list.child(
            select_option(
                format!("dev-filter-{l}").replace(' ', "-").to_lowercase(),
                row().child(text(l).font(ui(12.0, 400.0)).color(t().fg)),
                f == cur,
            )
            .on_press(move |s: &mut App| s.monitor_set_filter(f)),
        );
    }
    select_popup("dev-filter", Align::Start, list)
}

/// `DevView`.
pub fn view(s: &App) -> El {
    let th = t();
    let host = &s.svc.midi.host;
    let device = host
        .active_device_id()
        .and_then(|id| host.devices().iter().find(|d| d.id == id))
        .map(|d| d.name.clone());
    let paused = s.p9.monitor.paused;
    let filter = s.p9.monitor.filter;
    let events = s.p9.monitor.events();
    let chip = row()
        .h(24.0)
        .items_center()
        .gap(6.0)
        .px(8.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(if device.is_some() {
            th.surface2
        } else {
            Color::TRANSPARENT
        })
        .child(el().size(6.0, 6.0).radius(3.0).fill(if device.is_some() {
            th.play
        } else {
            th.muted_fg.alpha(0.5)
        }))
        .child(
            text(device.clone().unwrap_or_else(|| "No device".into()))
                .font(ui(12.0, 400.0))
                .color(if device.is_some() { th.fg } else { th.muted_fg }),
        );
    let label = FILTERS.iter().find(|f| f.0 == filter).map_or("All messages", |f| f.1);
    let header = row()
        .h(48.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .px(16.0)
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(icon(&icons::TERMINAL, 16.0).color(th.muted_fg))
        .child(text("MIDI Monitor").font(ui(14.0, 600.0)).color(th.fg))
        .child(chip)
        .child(spacer())
        .child(
            row()
                .key("dev-filter")
                .role("combobox")
                .label("Message type filter")
                .focusable(true)
                .h(32.0)
                .w(150.0)
                .px(10.0)
                .items_center()
                .justify_between()
                .radius(R_MD)
                .border(1.0, th.border)
                .fill(th.surface2)
                .hover(th.surface3)
                .child(text(label).font(ui(12.0, 400.0)).color(th.fg))
                .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
                .on_press(|s: &mut App| s.toggle_p9_menu(P9Menu::DevFilter)),
        )
        .child(
            Button::new(
                if paused {
                    Variant::Secondary
                } else {
                    Variant::Ghost
                },
                Size::Sm,
            )
            .icon(if paused {
                &icons::PLAY
            } else {
                &icons_views::PAUSE
            })
            .icon_size(14.0)
            .label(if paused { "Resume" } else { "Pause" })
            .font_size(12.0)
            .px(10.0)
            .pressed(paused)
            .ink(if paused { th.warn } else { th.muted_fg })
            .tip(if paused { "Resume capture" } else { "Pause capture" }, None)
            .key("dev-pause")
            .build(|s: &mut App, _| s.monitor_toggle_pause()),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::TRASH_2)
                .icon_size(14.0)
                .a11y("Clear log")
                .tip("Clear log", None)
                .key("dev-clear")
                .build(|s: &mut App, _| s.monitor_clear()),
        );
    let heads = ["Time (s)", "Type", "Ch", "Detail", "Raw hex"].map(|h| {
        text(h)
            .font(daw_label(10.0))
            .text_case(rune::TextCase::Uppercase)
            .color(th.muted_fg)
            .role("columnheader")
    });
    let [a, b, c, d, e] = heads;
    let columns = grid_row([a, b, c.align(Align::Center), d, e], 28.0)
        .role("row")
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border);
    let log: El = if events.is_empty() {
        let (title, hint) = if device.is_some() {
            (
                "Waiting for MIDI input…",
                "Play a key, move a knob or press a pad. Clock and active-sensing messages are hidden.",
            )
        } else {
            (
                "No MIDI device connected",
                "Connect a keyboard or controller and pick it in the top bar to see its raw messages here.",
            )
        };
        col()
            .key("dev-log")
            .role("table")
            .label("MIDI messages")
            .w_full()
            .grow(1.0)
            .center()
            .gap(8.0)
            .child(
                row()
                    .size(40.0, 40.0)
                    .center()
                    .radius(8.0)
                    .border(1.0, th.border)
                    .fill(th.surface1)
                    .child(icon(&icons::TERMINAL, 20.0).color(th.muted_fg)),
            )
            .child(text(title).font(ui(14.0, 500.0)).color(th.fg))
            .child(
                text(hint)
                    .font(ui(12.0, 400.0))
                    .color(th.muted_fg)
                    .wrap()
                    .align(Align::Center)
                    .max_w(320.0),
            )
    } else {
        let mut list = col().w_full().py(4.0);
        for ev in &events {
            list = list.child(event_row(ev));
        }
        col()
            .key("dev-log")
            .role("table")
            .label("MIDI messages")
            .w_full()
            .grow(1.0)
            .min_h(0.0)
            .child(list)
            .scroll(rune::ScrollSpec {
                stick_bottom: s.p9.following,
            })
    };
    let mut footer = row()
        .h(28.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .px(16.0)
        .fill(th.surface1)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(
            row()
                .child(
                    text(events.len().to_string())
                        .font(mono(11.0, 400.0))
                        .color(th.fg),
                )
                .child(
                    text(format!(
                        " events{}",
                        if filter != MonitorFilter::All {
                            " (filtered)"
                        } else {
                            ""
                        }
                    ))
                    .font(ui(11.0, 400.0))
                    .color(th.muted_fg),
                ),
        );
    if paused {
        footer = footer.child(
            row()
                .items_center()
                .gap(4.0)
                .child(icon(&icons_views::PAUSE, 12.0).color(th.warn))
                .child(text("Capture paused").font(ui(11.0, 600.0)).color(th.warn)),
        );
    }
    if !events.is_empty() {
        footer = footer.child(spacer()).child(if s.p9.following {
            text("Following live")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
        } else {
            row()
                .key("dev-jump")
                .role("button")
                .label("Jump to latest")
                .items_center()
                .gap(4.0)
                .px(6.0)
                .py(2.0)
                .radius(4.0)
                .hover(th.surface2)
                .child(icon(&icons_views::ARROW_DOWN_TO_LINE, 12.0).color(th.primary))
                .child(text("Jump to latest").font(ui(11.0, 500.0)).color(th.primary))
                .on_press(|s: &mut App| s.p9.following = true)
        });
    }
    col()
        .key("dev-view")
        .w_full()
        .h_full()
        .fill(th.surface0)
        .child(header)
        .child(columns)
        .child(log)
        .child(footer)
}
