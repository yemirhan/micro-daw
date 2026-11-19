//! The app's shadcn building blocks (`components/ui/button.tsx`, `tip.tsx`, `select.tsx`,
//! `popover.tsx`) and the repeated Tailwind patterns (segmented radiogroups, the `.daw-lcd`
//! well, hairline rules), with the same sizes, radii and colours.

use rune::{Align, Color, El, Icon, Shadow, Tx, col, el, icon, row, text};

use crate::app::App;
use crate::theme::{R_LG, R_MD, R_SM, mono, t, ui};

/// `buttonVariants.variant`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Primary fill (`default`).
    Primary,
    Outline,
    Ghost,
    Secondary,
}

/// `buttonVariants.size`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Xs,
    Sm,
    Md,
    IconXs,
    IconSm,
    Icon,
}

impl Size {
    fn height(self) -> f32 {
        match self {
            Size::Xs | Size::IconXs => 24.0,
            Size::Sm | Size::IconSm => 28.0,
            Size::Md | Size::Icon => 32.0,
        }
    }
    fn is_icon(self) -> bool {
        matches!(self, Size::IconXs | Size::IconSm | Size::Icon)
    }
    fn icon_size(self) -> f32 {
        match self {
            Size::Xs | Size::IconXs => 14.0,
            _ => 16.0,
        }
    }
    fn font(self) -> f32 {
        match self {
            Size::Xs => 11.0,
            Size::Sm => 12.0,
            _ => 13.0,
        }
    }
}

/// A shadcn `<Button>`: variant + size, optional icon and label, pressed (`aria-pressed`) and
/// disabled states (opacity 0.4, no handler).
pub struct Button {
    pub variant: Variant,
    pub size: Size,
    pub icon: Option<&'static Icon>,
    pub icon_size: Option<f32>,
    pub label: Option<String>,
    pub pressed: bool,
    pub disabled: bool,
    pub key: Option<String>,
    pub a11y: Option<String>,
    pub help: Option<(String, Option<String>)>,
    pub ink: Option<Color>,
    pub icon_ink: Option<Color>,
    pub fill: Option<Color>,
    pub border: Option<Color>,
    pub weight: f32,
    pub font_size: Option<f32>,
    pub px: Option<f32>,
}

impl Button {
    pub fn new(variant: Variant, size: Size) -> Self {
        Self {
            variant,
            size,
            icon: None,
            icon_size: None,
            label: None,
            pressed: false,
            disabled: false,
            key: None,
            a11y: None,
            help: None,
            ink: None,
            icon_ink: None,
            fill: None,
            border: None,
            weight: if variant == Variant::Primary {
                600.0
            } else {
                500.0
            },
            font_size: None,
            px: None,
        }
    }
    pub fn icon(mut self, i: &'static Icon) -> Self {
        self.icon = Some(i);
        self
    }
    pub fn icon_size(mut self, s: f32) -> Self {
        self.icon_size = Some(s);
        self
    }
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }
    pub fn pressed(mut self, p: bool) -> Self {
        self.pressed = p;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }
    /// `aria-label`.
    pub fn a11y(mut self, l: impl Into<String>) -> Self {
        self.a11y = Some(l.into());
        self
    }
    /// The `<Tip>` (label + optional shortcut keycap).
    pub fn tip(mut self, label: impl Into<String>, shortcut: Option<&str>) -> Self {
        self.help = Some((label.into(), shortcut.map(str::to_owned)));
        self
    }
    pub fn ink(mut self, c: Color) -> Self {
        self.ink = Some(c);
        self
    }
    pub fn icon_ink(mut self, c: Color) -> Self {
        self.icon_ink = Some(c);
        self
    }
    pub fn fill(mut self, c: Color) -> Self {
        self.fill = Some(c);
        self
    }
    pub fn border(mut self, c: Color) -> Self {
        self.border = Some(c);
        self
    }
    pub fn weight(mut self, w: f32) -> Self {
        self.weight = w;
        self
    }
    pub fn font_size(mut self, s: f32) -> Self {
        self.font_size = Some(s);
        self
    }
    pub fn px(mut self, p: f32) -> Self {
        self.px = Some(p);
        self
    }

    pub fn build(self, on: impl Fn(&mut App, &mut Tx) + 'static) -> El {
        let th = t();
        let (fill, ink, border, hover) = match self.variant {
            Variant::Primary => (th.primary, th.primary_fg, None, th.primary.alpha(0.88)),
            Variant::Outline => (th.surface2, th.fg, Some(th.border), th.surface3),
            Variant::Secondary => (th.surface3, th.fg, None, th.surface4),
            Variant::Ghost => {
                if self.pressed {
                    (th.surface3, th.fg, None, th.surface3)
                } else {
                    (Color::TRANSPARENT, th.muted_fg, None, th.surface3)
                }
            }
        };
        let fill = self.fill.unwrap_or(fill);
        let ink = self.ink.unwrap_or(ink);
        let border = self.border.or(border);
        let h = self.size.height();
        let radius = if matches!(self.size, Size::Xs | Size::IconXs) {
            R_SM
        } else {
            R_MD
        };
        let mut b = row()
            .h(h)
            .center()
            .gap(if self.size == Size::Xs { 4.0 } else { 6.0 })
            .radius(radius)
            .fill(fill)
            .role("button");
        if self.size.is_icon() {
            b = b.w(h);
        } else {
            let px = self.px.unwrap_or(match (self.size, self.icon.is_some()) {
                (Size::Xs, true) => 6.0,
                (Size::Xs, false) => 8.0,
                (Size::Sm, true) => 8.0,
                (Size::Sm, false) => 10.0,
                (_, true) => 10.0,
                _ => 12.0,
            });
            b = b.px(px);
        }
        if let Some(c) = border {
            b = b.border(1.0, c);
        }
        if let Some(i) = self.icon {
            let s = self.icon_size.unwrap_or(self.size.icon_size());
            b = b.child(icon(i, s).color(self.icon_ink.unwrap_or(ink)));
        }
        if let Some(l) = &self.label {
            b = b.child(
                text(l.clone())
                    .font(ui(self.font_size.unwrap_or(self.size.font()), self.weight))
                    .color(ink),
            );
        }
        if let Some(k) = self.key {
            b = b.key(k);
        }
        if let Some(l) = self.a11y.clone().or(self.label.clone()) {
            b = b.label(l);
        }
        if self.pressed {
            b = b.checked(true);
        }
        if let Some((label, keys)) = self.help {
            b = b.help(label);
            if let Some(k) = keys {
                b = b.help_keys([k]);
            }
        }
        if self.disabled {
            b.opacity(0.4).disabled(true)
        } else {
            b.hover(hover).on_press_tx(on)
        }
    }
}

/// One option of a segmented radiogroup.
pub struct SegOption<T> {
    pub value: T,
    pub label: String,
}

/// The app's segmented `role="radiogroup"` (Beat/Song, 4×4|2×8, Bars, Count-in, Step size…):
/// `h` tall, rounded-md, `border-border bg-surface-0 p-0.5`; the chosen item `bg-surface-3`.
pub struct Segmented<T> {
    pub label: String,
    pub options: Vec<SegOption<T>>,
    pub value: T,
    pub height: f32,
    pub mono: bool,
    pub size: f32,
    pub weight: f32,
    pub selected_weight: f32,
    pub item_px: f32,
    pub min_item_w: f32,
    pub key: Option<String>,
}

impl<T: PartialEq + Copy + 'static> Segmented<T> {
    pub fn new(label: impl Into<String>, value: T, options: Vec<(T, String)>) -> Self {
        Self {
            label: label.into(),
            options: options
                .into_iter()
                .map(|(value, label)| SegOption { value, label })
                .collect(),
            value,
            height: 28.0,
            mono: true,
            size: 11.0,
            weight: 400.0,
            selected_weight: 400.0,
            item_px: 6.0,
            min_item_w: 0.0,
            key: None,
        }
    }
    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }
    pub fn sans(mut self) -> Self {
        self.mono = false;
        self
    }
    pub fn size(mut self, s: f32) -> Self {
        self.size = s;
        self
    }
    pub fn weight(mut self, w: f32, selected: f32) -> Self {
        self.weight = w;
        self.selected_weight = selected;
        self
    }
    pub fn item_px(mut self, p: f32) -> Self {
        self.item_px = p;
        self
    }
    pub fn min_item_w(mut self, w: f32) -> Self {
        self.min_item_w = w;
        self
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }

    pub fn build(self, on: impl Fn(&mut App, T) + Clone + 'static) -> El {
        let th = t();
        let mut g = row()
            .h(self.height)
            .items_stretch()
            .padding(2.0)
            .radius(R_MD)
            .border(1.0, th.border)
            .fill(th.surface0)
            .role("radiogroup")
            .label(self.label.clone());
        if let Some(k) = &self.key {
            g = g.key(k.clone());
        }
        for (i, o) in self.options.into_iter().enumerate() {
            let sel = o.value == self.value;
            let font = if self.mono {
                mono(
                    self.size,
                    if sel {
                        self.selected_weight
                    } else {
                        self.weight
                    },
                )
            } else {
                ui(
                    self.size,
                    if sel {
                        self.selected_weight
                    } else {
                        self.weight
                    },
                )
            };
            let v = o.value;
            let on = on.clone();
            let mut item = row()
                .center()
                .px(self.item_px)
                .radius(R_SM)
                .role("radio")
                .label(o.label.clone())
                .checked(sel)
                .child(
                    text(o.label)
                        .font(font)
                        .color(if sel { th.fg } else { th.muted_fg }),
                )
                .on_press(move |s: &mut App| on(s, v));
            if self.min_item_w > 0.0 {
                item = item.min_w(self.min_item_w);
            }
            if let Some(k) = &self.key {
                item = item.key(format!("{k}-{i}"));
            }
            g = g.child(if sel { item.fill(th.surface3) } else { item });
        }
        g
    }
}

/// `.daw-lcd`: inset well for counters.
pub fn lcd() -> El {
    let th = t();
    row()
        .fill(th.surface0)
        .border(1.0, th.border)
        .radius(R_MD)
        .box_shadow(Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.35),
            inset: true,
        })
}

/// A 1px vertical rule `h` tall.
pub fn vrule(h: f32, c: Color) -> El {
    el().w(1.0).h(h).fill(c)
}

/// A 1px horizontal rule.
pub fn hrule(c: Color) -> El {
    el().h(1.0).w_full().fill(c)
}

/// `<PopoverContent>` / `<SelectContent>` frame: rounded-lg, border, popover fill, big soft
/// shadow.
pub fn popover_frame(width: Option<f32>, padding: f32) -> El {
    let th = t();
    let mut p = col()
        .fill(th.popover)
        .border(1.0, th.border)
        .radius(R_LG)
        .padding(padding)
        .box_shadow(Shadow {
            x: 0.0,
            y: 16.0,
            blur: 40.0,
            spread: -10.0,
            color: Color::BLACK.alpha(0.65),
            inset: false,
        });
    if let Some(w) = width {
        p = p.w(w);
    }
    p
}

/// A popover anchored under (or over) the element keyed `anchor`, closed by Escape or a click
/// outside (`modal`, like Radix's `onOpenChange(false)`).
pub fn popover(anchor: &str, align: Align, content: El) -> El {
    content
        .anchored(anchor.to_owned(), rune::Side::Bottom, align, 4.0)
        .modal(true, |s: &mut App| s.close_menu())
}

/// A select's open list (Radix `SelectContent`): a `listbox` focus group in a popover. It
/// opens with the chosen option focused (`autofocus` on it), Up / Down / Home / End move,
/// Return chooses (the option's `on_press`), Escape or a click outside closes, and focus goes
/// back to the trigger keyed `anchor` when it closes.
pub fn select_popup(anchor: &str, align: Align, list: El) -> El {
    popover(
        anchor,
        align,
        list.key(format!("{anchor}-list"))
            .role("listbox")
            .focus_group()
            .focus_return(anchor.to_owned()),
    )
}

/// One option of [`select_popup`] (`SelectItem`): the row's highlight follows the keyboard and
/// the pointer alike (`focus_highlight`), `aria-selected` on the chosen one.
pub fn select_option(key: String, content: El, selected: bool) -> El {
    let r = content
        .key(key)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(8.0)
        .py(6.0)
        .radius(R_SM)
        .role("option")
        .focus_selected(selected)
        .focusable(true)
        .focus_highlight(t().surface3);
    if selected { r.autofocus() } else { r }
}

/// A menu row (`flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-xs hover:bg-surface-3`).
pub fn menu_row(content: El, disabled: bool) -> El {
    let r = content
        .w_full()
        .items_center()
        .gap(8.0)
        .px(8.0)
        .py(6.0)
        .radius(R_SM)
        .role("menuitem");
    if disabled {
        r.opacity(0.5).disabled(true)
    } else {
        r.hover(t().surface3)
    }
}

/// `<Kbd>`.
pub fn kbd(label: &str) -> El {
    let th = t();
    row()
        .h(18.0)
        .min_w(18.0)
        .px(4.0)
        .center()
        .radius(4.0)
        .border(1.0, th.border)
        .fill(th.surface3)
        .child(text(label).font(mono(10.0, 500.0)).color(th.muted_fg))
}

/// Text helper: `size` px in the UI font, `weight`, `color`.
pub fn label(s: impl Into<String>, size: f32, weight: f32, c: Color) -> El {
    text(s).font(ui(size, weight)).color(c)
}

/// Mono text helper.
pub fn mono_label(s: impl Into<String>, size: f32, weight: f32, c: Color) -> El {
    text(s).font(mono(size, weight)).color(c)
}

/// `⌘` on macOS, `Ctrl+` elsewhere.
pub fn mod_key() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘"
    } else {
        "Ctrl+"
    }
}
