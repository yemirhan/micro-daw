//! `components/ui/view-kit.tsx` (ViewHeader, EmptyState, FilterChips, Readout, Kbd), shadcn's
//! `Switch` and `Slider`, the dialog frame (`DialogContent` / the custom modals) and the
//! Samples view's `ParamSlider` / `EffectSection`.

use rune::{Color, El, Icon, Phase, Shadow, Tx, col, el, icon, row, text};

use crate::app::App;
use crate::theme::{R_LG, R_MD, R_XL, daw_label, mono, t, ui};

/// `ViewHeader`: title (20 px, icon 20) + subtitle (13 px), actions on the right, an optional
/// row below. `px` / `pt` / `pb` are the header's paddings.
pub fn view_header(
    ic: &'static Icon,
    title: &str,
    subtitle: &str,
    actions: Option<El>,
    below: Option<El>,
    pad: [f32; 3],
) -> El {
    let th = t();
    let mut top = row().w_full().items_start().justify_between().gap(24.0).child(
        col()
            .min_w(0.0)
            .child(
                row()
                    .items_center()
                    .gap(10.0)
                    .child(icon(ic, 20.0).color(th.muted_fg))
                    .child(
                        text(title.to_owned())
                            .font(ui(20.0, 600.0).tracking(-0.5))
                            .color(th.fg),
                    ),
            )
            .child(
                text(subtitle.to_owned())
                    .font(ui(13.0, 400.0))
                    .color(th.muted_fg)
                    .mt(4.0),
            ),
    );
    if let Some(a) = actions {
        top = top.child(row().items_center().gap(12.0).child(a));
    }
    let mut h = col()
        .w_full()
        .px(pad[0])
        .pt(pad[1])
        .pb(pad[2])
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(top);
    if let Some(b) = below {
        h = h.child(el().h(16.0)).child(b);
    }
    h
}

/// `EmptyState`: icon tile, title, body.
pub fn empty_state(ic: Option<&'static Icon>, title: &str, body: Vec<El>) -> El {
    let th = t();
    let mut c = col().w_full().center().gap(8.0).px(24.0).py(40.0);
    if let Some(i) = ic {
        c = c.child(
            row()
                .size(40.0, 40.0)
                .center()
                .mb(4.0)
                .radius(R_LG)
                .border(1.0, th.border)
                .fill(th.surface2)
                .child(icon(i, 20.0).color(th.muted_fg)),
        );
    }
    c = c.child(text(title.to_owned()).font(ui(14.0, 500.0)).color(th.fg));
    if !body.is_empty() {
        let mut b = col().max_w(320.0).items_center();
        for e in body {
            b = b.child(e);
        }
        c = c.child(b);
    }
    c
}

/// Body text of an empty state (12 px, relaxed, muted, centred).
pub fn empty_text(s: &str) -> El {
    text(s.to_owned())
        .font(ui(12.0, 400.0).lh(19.5))
        .color(t().muted_fg)
        .wrap()
        .align(rune::Align::Center)
}

/// One chip of `FilterChips`.
pub struct Chip {
    pub key: String,
    pub label: String,
    pub count: Option<usize>,
    pub active: bool,
}

/// `FilterChips`: a wrapping radiogroup of chips (h-8, px-3, 12 px).
pub fn filter_chips(label: &str, chips: Vec<Chip>, on: impl Fn(&mut App, usize) + Clone + 'static) -> El {
    let th = t();
    let mut g = row()
        .role("radiogroup")
        .label(label.to_owned())
        .flex_wrap()
        .gaps(6.0, 6.0);
    for (i, c) in chips.into_iter().enumerate() {
        let on = on.clone();
        let mut b = row()
            .key(c.key)
            .role("radio")
            .label(c.label.clone())
            .checked(c.active)
            .h(32.0)
            .items_center()
            .gap(6.0)
            .px(12.0)
            .radius(R_MD)
            .border(1.0, if c.active { th.primary.alpha(0.5) } else { th.border })
            .fill(if c.active {
                th.primary.alpha(0.12)
            } else {
                th.surface2
            })
            .child(text(c.label).font(ui(12.0, 500.0)).color(if c.active {
                th.fg
            } else {
                th.muted_fg
            }))
            .on_press(move |s: &mut App| on(s, i));
        if !c.active {
            b = b.hover(th.surface3);
        }
        if let Some(n) = c.count {
            b = b.child(text(n.to_string()).font(mono(10.0, 400.0)).color(if c.active {
                th.primary
            } else {
                th.muted_fg.alpha(0.7)
            }));
        }
        g = g.child(b);
    }
    g
}

/// `Readout`: a tiny label over a mono value.
pub fn readout(label: &str, value: El) -> El {
    let th = t();
    col()
        .px(12.0)
        .py(6.0)
        .radius(R_MD)
        .border(1.0, th.border)
        .fill(th.surface0)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.35),
            inset: true,
        })
        .child(
            text(label.to_owned())
                .font(daw_label(9.5).tracking(0.95))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
        .child(value)
}

/// `Kbd` of the view kit (h-5, 10 px mono).
pub fn kbd(label: &str) -> El {
    let th = t();
    row()
        .h(20.0)
        .min_w(20.0)
        .px(4.0)
        .center()
        .radius(4.0)
        .border(1.0, th.border)
        .fill(th.surface2)
        .child(text(label.to_owned()).font(mono(10.0, 500.0)).color(th.muted_fg))
}

/// shadcn `Switch` (default size: 32 × 18.4, 16-point thumb).
pub fn switch(key: &str, label: &str, on: bool, f: impl Fn(&mut App, bool) + 'static) -> El {
    let th = t();
    row()
        .key(key.to_owned())
        .role("switch")
        .label(label.to_owned())
        .checked(on)
        .focusable(true)
        .w(32.0)
        .h(18.4)
        .radius(9.2)
        .items_center()
        .fill(if on { th.primary } else { th.surface4 })
        .child(
            el().size(16.0, 16.0)
                .radius(8.0)
                .ml(if on { 15.2 } else { 1.2 })
                .fill(if on { th.primary_fg } else { th.fg.alpha(0.8) })
                .shadow(1.0, 2.0, Color::BLACK.alpha(0.4)),
        )
        .on_press(move |s: &mut App| f(s, !on))
}

/// A slider's numbers.
#[derive(Debug, Clone, Copy)]
pub struct SliderSpec {
    pub id: u16,
    pub min: f64,
    pub max: f64,
    pub step: f64,
}

impl SliderSpec {
    pub fn snap(&self, v: f64) -> f64 {
        let s = ((v - self.min) / self.step).round() * self.step + self.min;
        let s = s.clamp(self.min, self.max);
        // trim float noise of the step (0.1 → 0.30000000000000004)
        (s * 1e9).round() / 1e9
    }
}

/// shadcn `Slider` (Radix): a 4-point track, the primary range, a 12-point thumb. Click / drag
/// sets the value from the pointer; arrows ±step (Shift ×10 is Radix's Page), Home / End.
pub fn slider(
    key: &str,
    label: &str,
    spec: SliderSpec,
    value: f64,
    disabled: bool,
    on: impl Fn(&mut App, f64) + Clone + 'static,
) -> El {
    let th = t();
    let frac = ((value - spec.min) / (spec.max - spec.min)).clamp(0.0, 1.0) as f32;
    let mut s = el()
        .key(key.to_owned())
        .role("slider")
        .label(label.to_owned())
        .range_value(value, spec.min, spec.max)
        .range_step(spec.step)
        .focusable(!disabled)
        .h(16.0)
        .grow(1.0)
        .min_w(0.0)
        .child(
            el().pin(Some(0.0), Some(6.0), Some(0.0), None)
                .h(4.0)
                .radius(2.0)
                .fill(th.surface4)
                .child(el().h(4.0).w_frac(frac).radius(2.0).fill(th.primary)),
        )
        .child(
            row()
                .pin(Some(0.0), Some(2.0), Some(0.0), None)
                .h(12.0)
                .child(el().h(12.0).w_frac(frac))
                .child(
                    el().size(12.0, 12.0)
                        .ml(-6.0)
                        .radius(6.0)
                        .fill(th.fg)
                        .border(1.0, Color::BLACK.alpha(0.4))
                        .shadow(1.0, 2.0, Color::BLACK.alpha(0.5)),
                ),
        );
    if disabled {
        return s.opacity(0.5).disabled(true);
    }
    let on_drag = on.clone();
    let on_key = on.clone();
    let on_step = on.clone();
    s = s
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up {
                return;
            }
            let x = d.x.clamp(0.0, d.w.max(1.0));
            let v = spec.snap(spec.min + (x / d.w.max(1.0)) as f64 * (spec.max - spec.min));
            on_drag(s, v);
        })
        .on_key(move |s: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx| {
            let big = if ev.mods.shift { 10.0 } else { 1.0 };
            let next = match ev.key {
                rune::Key::Right | rune::Key::Up => value + spec.step * big,
                rune::Key::Left | rune::Key::Down => value - spec.step * big,
                rune::Key::Home => spec.min,
                rune::Key::End => spec.max,
                _ => return false,
            };
            on_key(s, spec.snap(next));
            true
        })
        .on_step(move |s: &mut App, delta: i32, _tx: &mut Tx| {
            on_step(s, spec.snap(value + delta as f64 * spec.step));
        });
    s
}

/// `ParamSlider`: label (80 px, daw-label), slider, a 64-point LCD readout, optional suffix.
#[allow(clippy::too_many_arguments)]
pub fn param_slider(
    key: &str,
    label: &str,
    spec: SliderSpec,
    value: f64,
    display: String,
    disabled: bool,
    suffix: Option<El>,
    on: impl Fn(&mut App, f64) + Clone + 'static,
) -> El {
    let th = t();
    let mut r = row()
        .h(32.0)
        .w_full()
        .items_center()
        .gap(12.0)
        .child(
            text(label.to_owned())
                .font(daw_label(10.5).tracking(0.84))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .w(80.0),
        )
        .child(slider(key, label, spec, value, disabled, on))
        .child(
            crate::ui::widgets::lcd()
                .h(24.0)
                .w(64.0)
                .px(8.0)
                .items_center()
                .justify_end()
                .child(text(display).font(mono(11.0, 400.0)).color(th.fg)),
        );
    if let Some(sf) = suffix {
        r = r.child(sf);
    }
    if disabled { r.opacity(0.45) } else { r }
}

/// `EffectSection`: switch + title + description; parameters below while on.
pub fn effect_section(
    key: &str,
    title: &str,
    description: &str,
    enabled: bool,
    on: impl Fn(&mut App, bool) + 'static,
    body: Vec<El>,
) -> El {
    let th = t();
    let mut sec = col()
        .key(format!("fx-{key}"))
        .w_full()
        .radius(R_LG)
        .border(1.0, if enabled { th.hairline_strong } else { th.border })
        .fill(if enabled { th.surface2 } else { th.surface1 })
        .child(
            row()
                .w_full()
                .min_h(40.0)
                .items_center()
                .gap(12.0)
                .px(14.0)
                .py(10.0)
                .child(switch(&format!("fx-{key}-switch"), title, enabled, on))
                .child(
                    row()
                        .min_w(0.0)
                        .grow(1.0)
                        .items_end()
                        .gap(8.0)
                        .child(text(title.to_owned()).font(ui(13.0, 600.0)).color(if enabled {
                            th.fg
                        } else {
                            th.fg.alpha(0.8)
                        }))
                        .child(
                            text(description.to_owned())
                                .font(ui(12.0, 400.0))
                                .color(th.muted_fg)
                                .truncate(),
                        ),
                ),
        );
    if enabled && !body.is_empty() {
        let mut b = col()
            .w_full()
            .gap(4.0)
            .px(14.0)
            .py(10.0)
            .border_sides([1.0, 0.0, 0.0, 0.0], th.border);
        for e in body {
            b = b.child(e);
        }
        sec = sec.child(b);
    }
    sec
}

/// The dialogs' backdrop (`bg-[oklch(0.1 0.004 65 / 0.72)]`) with the content centred
/// (or at `top` from the top). A click outside / Escape runs `close`.
pub fn dialog_layer(key: &str, content: El, top: Option<f32>, close: fn(&mut App)) -> El {
    let layer = el()
        .key(format!("{key}-layer"))
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .fill(rune::oklcha(0.1, 0.004, 65.0, 0.72));
    let content = content.key(key.to_owned()).role("dialog").modal(true, close);
    match top {
        Some(y) => layer.child(col().w_full().items_center().pt(y).child(content)),
        None => layer.center().child(content),
    }
}

/// The dialogs' panel (`rounded-xl border bg-surface-1` + the big shadow).
pub fn dialog_panel(w: f32, padding: f32) -> El {
    let th = t();
    col()
        .w(w)
        .padding(padding)
        .radius(R_XL + 2.0)
        .border(1.0, th.border)
        .fill(th.surface1)
        .box_shadow(Shadow {
            x: 0.0,
            y: 24.0,
            blur: 64.0,
            spread: -12.0,
            color: Color::BLACK.alpha(0.7),
            inset: false,
        })
}
