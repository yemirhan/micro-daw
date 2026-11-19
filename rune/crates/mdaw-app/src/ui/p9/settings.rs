//! `components/settings/SettingsView.tsx`: General (updates), Audio (default master volume,
//! buffer size), MIDI (auto-connect), About. Saved under `micro-daw-settings` like the TS.
//!
//! The update rows: the Rust app has no auto-updater (Electron's `electron-updater`); the
//! "Software update" row shows the installed version and "Check now" reports the state the
//! updater would (`idle` → nothing shown). See the phase-9 doc.

use mdaw_model::types::UpdateStatus;
use rune::{Align, Color, El, Icon, Tx, col, icon, row, text};
use serde_json::json;

use crate::app::App;
use crate::icons;
use crate::icons_views;
use crate::theme::{R_MD, R_XL, daw_label, mono, t, ui};
use crate::ui::p9::kit::{SliderSpec, slider, switch, view_header};
use crate::ui::widgets::{Button, Size, Variant, lcd, popover_frame, select_option, select_popup};
use crate::views::P9Menu;

fn section(ic: &'static Icon, title: &str, rows: Vec<El>) -> El {
    let th = t();
    let mut list = col()
        .w_full()
        .radius(R_XL + 2.0)
        .border(1.0, th.border)
        .fill(th.surface1)
        .clip_rounded(R_XL + 2.0);
    // `divide-y`: a 1-point rule between the rows (it adds to the height, as a border does).
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            list = list.child(rune::el().h(1.0).w_full().fill(th.border));
        }
        list = list.child(r);
    }
    col()
        .key(format!("settings-{}", title.to_lowercase()))
        .label(title.to_owned())
        .w_full()
        .child(
            row()
                .mb(8.0)
                .px(4.0)
                .items_center()
                .gap(8.0)
                .child(icon(ic, 14.0).color(th.muted_fg))
                .child(
                    text(title.to_owned())
                        .font(daw_label(11.0))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                ),
        )
        .child(list)
}

fn setting_row(label: &str, description: Option<El>, control: Option<El>, below: Option<El>) -> El {
    let th = t();
    let mut left = col()
        .min_w(0.0)
        .grow(1.0)
        .child(text(label.to_owned()).font(ui(13.0, 500.0)).color(th.fg));
    if let Some(d) = description {
        left = left.child(d.mt(2.0));
    }
    let mut top = row()
        .w_full()
        .min_h(36.0)
        .items_center()
        .justify_between()
        .gap(24.0)
        .child(left);
    if let Some(c) = control {
        top = top.child(row().items_center().gap(8.0).child(c));
    }
    let mut r = col().w_full().px(16.0).py(12.0).child(top);
    if let Some(b) = below {
        r = r.child(b.mt(10.0));
    }
    r
}

fn desc(s: &str) -> El {
    text(s.to_owned())
        .font(ui(12.0, 400.0))
        .color(t().muted_fg)
        .wrap()
}

fn update_status(st: &UpdateStatus) -> Option<El> {
    let th = t();
    let line = |ic: &'static Icon, s: String, c: Color, w: f32| {
        row()
            .items_center()
            .gap(6.0)
            .child(icon(ic, 14.0).color(c))
            .child(text(s).font(ui(12.0, w)).color(c))
    };
    Some(match st {
        UpdateStatus::Idle => return None,
        UpdateStatus::Checking => line(
            &icons::LOADER_CIRCLE,
            "Checking for updates…".into(),
            th.muted_fg,
            400.0,
        ),
        UpdateStatus::Available { version } => line(
            &icons::DOWNLOAD,
            format!("Version {version} is available"),
            th.primary,
            500.0,
        ),
        UpdateStatus::NotAvailable => line(
            &icons_views::CIRCLE_CHECK,
            "You're up to date".into(),
            th.play,
            400.0,
        ),
        UpdateStatus::Downloading { percent } => row()
            .items_center()
            .gap(8.0)
            .child(
                text(format!("Downloading {}%", percent.round()))
                    .font(mono(12.0, 400.0))
                    .color(th.muted_fg),
            )
            .child(
                row().h(6.0).w(160.0).radius(3.0).fill(th.surface3).child(
                    rune::el()
                        .h(6.0)
                        .w_frac((*percent as f32 / 100.0).clamp(0.0, 1.0))
                        .radius(3.0)
                        .fill(th.primary),
                ),
            ),
        UpdateStatus::Downloaded { version } => line(
            &icons_views::CIRCLE_CHECK,
            format!("Version {version} is ready to install"),
            th.play,
            500.0,
        ),
        UpdateStatus::Error { message } => line(
            &icons_views::CIRCLE_ALERT,
            message.clone(),
            th.destructive,
            400.0,
        ),
    })
}

/// The buffer size list.
pub fn buffer_menu(s: &App) -> El {
    let cur = s.p9.settings.audio.buffer_size_hint;
    let mut list = popover_frame(Some(128.0), 4.0);
    for n in [128, 256, 512, 1024, 2048] {
        list = list.child(
            select_option(
                format!("buffer-{n}"),
                row().child(
                    text(format!("{n} samples"))
                        .font(mono(12.0, 400.0))
                        .color(t().fg),
                ),
                cur as i64 == n,
            )
            .on_press(move |s: &mut App| {
                s.update_settings(json!({ "audio": { "bufferSizeHint": n } }));
                s.menu = None;
            }),
        );
    }
    select_popup("buffer-size", Align::End, list)
}

/// `SettingsView`.
pub fn view(s: &App) -> El {
    let th = t();
    let st = &s.p9.settings;
    let version = env!("CARGO_PKG_VERSION");
    let status = UpdateStatus::Idle;
    let general = section(
        &icons::SETTINGS_2,
        "General",
        vec![
            setting_row(
                "Check for updates automatically",
                Some(desc("Looks for a new version when the app starts.")),
                Some(switch(
                    "auto-check-updates",
                    "Check for updates automatically",
                    st.general.auto_check_updates,
                    |s: &mut App, v| {
                        s.update_settings(json!({ "general": { "autoCheckUpdates": v } }))
                    },
                )),
                None,
            ),
            setting_row(
                "Software update",
                Some(desc(&format!("Installed: {version}"))),
                Some(
                    Button::new(Variant::Outline, Size::Sm)
                        .icon(&icons::REFRESH_CW)
                        .icon_size(14.0)
                        .label("Check now")
                        .key("check-updates")
                        .build(|_s: &mut App, _tx: &mut Tx| {}),
                ),
                update_status(&status),
            ),
        ],
    );
    let vol = st.audio.default_master_volume;
    let audio = section(
        &icons::SLIDERS_HORIZONTAL,
        "Audio",
        vec![
            setting_row(
                "Default master volume",
                Some(desc("Used for new projects.")),
                None,
                Some(
                    row()
                        .w_full()
                        .items_center()
                        .gap(12.0)
                        .child(slider(
                            "default-master-volume",
                            "Default master volume",
                            SliderSpec {
                                id: 100,
                                min: -40.0,
                                max: 0.0,
                                step: 1.0,
                            },
                            vol,
                            false,
                            |s: &mut App, v| {
                                s.update_settings(json!({ "audio": { "defaultMasterVolume": v } }))
                            },
                        ))
                        .child(
                            lcd()
                                .h(28.0)
                                .w(80.0)
                                .px(8.0)
                                .items_center()
                                .justify_end()
                                .child(
                                    text(format!("{} dB", mdaw_model::js::number_to_string(vol)))
                                        .font(mono(12.0, 400.0))
                                        .color(th.fg),
                                ),
                        ),
                ),
            ),
            setting_row(
                "Buffer size",
                Some(desc(
                    "Smaller is more responsive; larger is safer on slow machines.",
                )),
                Some(
                    row()
                        .key("buffer-size")
                        .role("combobox")
                        .label("Buffer size")
                        .focusable(true)
                        .h(32.0)
                        .w(128.0)
                        .px(10.0)
                        .items_center()
                        .justify_between()
                        .radius(R_MD)
                        .border(1.0, th.border)
                        .fill(th.surface2)
                        .hover(th.surface3)
                        .child(
                            text(format!("{} samples", st.audio.buffer_size_hint as i64))
                                .font(mono(12.0, 400.0))
                                .color(th.fg),
                        )
                        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
                        .on_press(|s: &mut App| s.toggle_p9_menu(P9Menu::BufferSize)),
                ),
                None,
            ),
        ],
    );
    let midi = section(
        &icons_views::USB,
        "MIDI",
        vec![setting_row(
            "Auto-connect last device",
            Some(desc("Reconnect to the keyboard you used last time.")),
            Some(switch(
                "auto-connect-midi",
                "Auto-connect last device",
                st.midi.auto_connect_last_device,
                |s: &mut App, v| {
                    s.update_settings(json!({ "midi": { "autoConnectLastDevice": v } }))
                },
            )),
            None,
        )],
    );
    let link = |label: &str, url: &'static str, key: &str| {
        row()
            .key(key.to_owned())
            .role("link")
            .label(label.to_owned())
            .items_center()
            .gap(4.0)
            .child(
                text(label.to_owned())
                    .font(ui(12.0, 500.0))
                    .color(th.primary),
            )
            .on_press_tx(move |_s: &mut App, tx: &mut Tx| {
                let _ = tx.open_url(url);
            })
    };
    let about = section(
        &icons_views::INFO,
        "About",
        vec![
            setting_row(
                "Version",
                None,
                Some(text(version).font(mono(12.0, 400.0)).color(th.fg)),
                None,
            ),
            setting_row(
                "Author",
                None,
                Some(text("Yusuf Emirhan Sahin").font(ui(12.0, 400.0)).color(th.fg)),
                None,
            ),
            setting_row(
                "Source code",
                None,
                Some(
                    link("GitHub", "https://github.com/yemirhan/micro-daw", "source-link")
                        .child(icon(&icons_views::EXTERNAL_LINK, 12.0).color(th.primary)),
                ),
                None,
            ),
            setting_row(
                "Credits",
                Some(
                    row()
                        .flex_wrap()
                        .child(desc(
                            "Piano samples: Salamander Grand Piano V3 by Alexander Holm, licensed under ",
                        ))
                        .child(link(
                            "CC BY 3.0",
                            "https://creativecommons.org/licenses/by/3.0/",
                            "credits-link",
                        ))
                        .child(desc(".")),
                ),
                None,
                None,
            ),
        ],
    );
    col()
        .key("settings-view")
        .w_full()
        .h_full()
        .fill(th.surface0)
        .child(view_header(
            &icons::SETTINGS_2,
            "Settings",
            "Preferences are saved on this computer.",
            None,
            None,
            [32.0, 20.0, 16.0],
        ))
        .child(
            col()
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(
                    col().w_full().px(32.0).py(24.0).items_center().child(
                        col()
                            .w_full()
                            .max_w(672.0)
                            .gap(28.0)
                            .pb(32.0)
                            .child(general)
                            .child(audio)
                            .child(midi)
                            .child(about),
                    ),
                )
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
}
