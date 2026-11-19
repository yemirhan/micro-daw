//! The Song view (phase 8): `App.tsx`'s Song branch — the piano roll when a region is being
//! edited, otherwise the arrangement (`ArrangementView`), then the mixer (X), then the
//! instrument dock (hidden while the piano roll is open). The Song transport sits in the top
//! bar (`transport.rs`). Menus and popovers are drawn at the root (`overlay`).

pub mod dock;
pub mod fx;
pub mod headers;
pub mod menus;
pub mod mixer;
pub mod timeline;
pub mod toolbar;
pub mod transport;

pub use fx::knob_value;

use rune::{Color, Cx, El, col, row, text};

use crate::app::App;
use crate::song::{HEADER_W, RULER_H, SongMenu};
use crate::theme::{daw_label, mono, t, ui};

/// CSS `color-mix(in oklab, a p%, b)` (premultiplied alpha, as the spec mixes).
pub fn cmix(a: Color, b: Color, p: f32) -> Color {
    let (la, lb) = (a.to_oklab(), b.to_oklab());
    let (wa, wb) = (p, 1.0 - p);
    let alpha = la[3] * wa + lb[3] * wb;
    if alpha <= 0.0 {
        return Color::TRANSPARENT;
    }
    let ch = |i: usize| (la[i] * la[3] * wa + lb[i] * lb[3] * wb) / alpha;
    Color::from_oklab([ch(0), ch(1), ch(2), alpha])
}

/// A track / region / marker colour string (`oklch(...)` or hex).
pub fn track_color(s: &str) -> Color {
    crate::theme::css_color(s, t().primary)
}

/// The Song view's main column.
pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let mut c = col().key("song").grow(1.0).min_w(0.0).min_h(0.0);
    c = if s.song.editing.is_some() {
        c.child(crate::ui::piano::view(s, cx))
    } else {
        c.child(arrangement(s, cx))
    };
    if s.song.show_mixer {
        c = c.child(mixer::view(s));
    }
    // Phase 9: the synth editor (when open) and the Live FX strip.
    if s.p9.synth_editor {
        c = c.child(crate::ui::p9::rack::synth_editor(s));
    }
    c = c.child(crate::ui::p9::rack::live_fx(s));
    if s.song.editing.is_none() {
        c = c.child(dock::view_cx(s, Some(cx)));
    }
    c
}

/// `ArrangementView`: toolbar, then the header column beside the ruler + lanes, and the FX
/// drawer over the right edge.
fn arrangement(s: &App, cx: &mut Cx<App>) -> El {
    let th = t();
    if let Some(size) = cx.layout_size("song-lanes") {
        s.song.viewport.set(size);
    }
    let tracks = s.tracks();
    let header = col()
        .key("song-headers")
        .w(HEADER_W)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .box_shadow(rune::Shadow {
            x: 2.0,
            y: 0.0,
            blur: 8.0,
            spread: -4.0,
            color: Color::BLACK.alpha(0.5),
            inset: false,
        })
        .child(
            row()
                .h(RULER_H)
                .w_full()
                .items_center()
                .justify_between()
                .px(12.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(
                    text("Tracks")
                        .font(daw_label(11.0))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    text(tracks.len().to_string())
                        .key("song-track-count")
                        .font(mono(10.0, 400.0))
                        .color(th.muted_fg),
                ),
        )
        .child(headers::column(s));
    let mut body = row()
        .key("song-body")
        .grow(1.0)
        .min_h(0.0)
        .child(header)
        .child(timeline::view(s));
    if let Some(id) = &s.song.fx_track
        && let Some(track) = s.find_track(id)
    {
        body = body.child(fx::view(s, track).absolute_right());
    }
    col()
        .key("arrangement")
        .grow(1.0)
        .min_h(0.0)
        .min_w(0.0)
        .fill(th.surface0)
        .child(toolbar::view(s))
        .child(body)
}

/// Extension: pin an element to its parent's right edge, full height.
pub trait PinRight {
    fn absolute_right(self) -> El;
}

impl PinRight for El {
    fn absolute_right(self) -> El {
        self.pin(None, Some(0.0), Some(0.0), Some(0.0))
    }
}

/// The Song view's popovers / menus at the root.
pub fn overlay(s: &App, m: &SongMenu, win: [f32; 2]) -> Option<El> {
    match m {
        SongMenu::Region {
            x,
            y,
            track,
            region,
        } => Some(menus::region_menu(s, *x, *y, track, region, win)),
        SongMenu::Marker {
            x,
            y,
            marker,
            renaming,
        } => menus::marker_menu(s, *x, *y, marker, *renaming, win),
        SongMenu::Automation { track } => Some(headers::automation_menu(s, track)),
        SongMenu::Instrument { track } => Some(headers::instrument_menu(s, track)),
        SongMenu::Snap => Some(toolbar::snap_menu(s)),
        SongMenu::MarkerJump => Some(transport::marker_jump_menu(s)),
        SongMenu::FilterType => Some(fx::filter_type_menu(s)),
        SongMenu::LfoWave => Some(fx::lfo_wave_menu(s)),
        SongMenu::PrGrid => Some(crate::ui::piano::toolbar::grid_menu(s)),
        SongMenu::PrLength => Some(crate::ui::piano::toolbar::length_menu(s)),
        SongMenu::PrShortcuts => Some(crate::ui::piano::toolbar::shortcuts_popover()),
    }
}

/// A small text helper (`text-[size] font-[weight]`).
pub fn label(s: impl Into<String>, size: f32, weight: f32, c: Color) -> El {
    text(s).font(ui(size, weight)).color(c)
}
