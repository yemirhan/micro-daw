//! `PadGrid.tsx` + `PadThumb.tsx` + `waveform.ts` drawing: the 16 pads (4×4 or 2×8, or one
//! bank of the dock), hit flashes (every live hit: MIDI, mouse, keys), pointer / Enter / Space
//! holds with velocity from the click height, the context menu, sample drops.

use mdaw_dsp::buffer::SharedBuffer;
use mdaw_model::beat_helpers::{
    PAD_KEY_HINTS, PadLayout, Peaks, bank_rows, hardware_note_label, pad_color, pad_label,
    pad_name, pad_rows, velocity_from_pointer,
};
use mdaw_model::fs_entries::AUDIO_EXTENSIONS;
use mdaw_model::types::PadSettings;
use rune::{
    Color, Cx, El, Key, KeyEvent, Phase, PointerPress, Shadow, Shape, Spring, Tx, canvas, col, el,
    icon, row, spacer, text,
};

use crate::app::{App, Menu, PadHold};
use crate::buffers::BufferStatus;
use crate::icons;
use crate::theme::{css_color, mono, t, ui};

/// What a grid shows (`PadGridProps`).
#[derive(Debug, Clone, Copy)]
pub struct GridOpts {
    pub layout: PadLayout,
    /// One bank only (the dock): 'A' or 'B'.
    pub bank: Option<char>,
    pub compact: bool,
    /// Key caps visible (`[data-show-keys]`).
    pub key_hints: bool,
    /// Context menu + drop target (the Beat view's grid).
    pub full: bool,
    /// Key prefix of the pads.
    pub prefix: &'static str,
}

/// The pad height threshold of the container queries (`max-height: 56px`).
const SHORT: f32 = 56.0;

/// Mirrored min/max waveform (`drawPeaks`) as a shape in a `w` × `h` box, optionally only the
/// columns inside `[a, b]` (points), each column clipped to it.
pub fn peaks_shape(
    shape: Shape,
    peaks: &Peaks,
    w: f32,
    h: f32,
    y0: f32,
    color: Color,
    clip: Option<(f32, f32)>,
) -> Shape {
    let cols = peaks.min.len().max(1);
    let col_w = w / cols as f32;
    let mid = y0 + h / 2.0;
    let half = h / 2.0;
    let mut shape = shape;
    for i in 0..cols {
        let top = mid - (0.5 / half).max(peaks.max[i]) * half;
        let bottom = mid - (-0.5 / half).min(peaks.min[i]) * half;
        let mut x0 = i as f32 * col_w;
        let mut x1 = x0 + (col_w - 0.25).max(1.0);
        let y1 = top + (bottom - top).max(1.0);
        if let Some((a, b)) = clip {
            x0 = x0.max(a);
            x1 = x1.min(b);
            if x1 <= x0 {
                continue;
            }
        }
        shape = shape.fill(vec![[x0, top], [x1, top], [x1, y1], [x0, y1]], color);
    }
    shape
}

/// A buffer's trim range as fractions `[a, b]` of its length (`PadThumb`).
pub fn trim_fractions(settings: &PadSettings, duration: f64) -> (f32, f32) {
    let dur = if duration > 0.0 { duration } else { 1.0 };
    let a = (settings.start_sec / dur).clamp(0.0, 1.0);
    let b = (settings.end_sec.unwrap_or(dur) / dur).clamp(a, 1.0);
    (a as f32, b as f32)
}

pub fn buffer_duration(b: &SharedBuffer) -> f64 {
    b.frames() as f64 / b.sample_rate.max(1) as f64
}

/// `PadThumb`: "no pack" / "missing" badges, else the waveform (whole at 16% white, the
/// trimmed range in the pad colour).
fn thumb(s: &App, settings: &PadSettings, color: Color, missing_title: Option<&str>, w: f32) -> El {
    let th = t();
    let cache = s.svc.engine.cache().clone();
    let source = settings.sample.as_ref().map(|x| &x.source);
    let (buf, status) = s.buffers.borrow_mut().get(&cache, source);
    if status == BufferStatus::Missing {
        let (ic, label, title) = match missing_title {
            Some(title) => (&icons::PACKAGE_X, "no pack", title.to_owned()),
            None => (
                &icons::TRIANGLE_ALERT,
                "missing",
                "Sample file not found".to_owned(),
            ),
        };
        return row()
            .w_full()
            .h_full()
            .items_center()
            .gap(4.0)
            .help(title)
            .child(icon(ic, 12.0).color(th.warn))
            .child(
                text(label)
                    .font(mono(11.0, 400.0))
                    .color(th.warn)
                    .truncate(),
            );
    }
    let Some(buf) = buf else {
        // Loading: an empty canvas (the TS pulses it).
        return el().w_full().h_full().opacity(0.5);
    };
    let h = 18.0;
    let w = w.max(16.0);
    let cols = ((w / 2.0).round() as usize).clamp(16, 160);
    let key = source.map(|src| cache.key(src)).unwrap_or_default();
    let peaks = s.buffers.borrow_mut().peaks(&key, &buf, cols).clone();
    let (a, b) = trim_fractions(settings, buffer_duration(&buf));
    let shape = peaks_shape(Shape::new(), &peaks, w, h, 0.0, th.white.alpha(0.16), None);
    let clip_b = (b * w).max(a * w + 1.0);
    let shape = peaks_shape(shape, &peaks, w, h, 0.0, color, Some((a * w, clip_b)));
    canvas(shape.view_box(w, h)).w_full().h(h)
}

/// One pad (`Pad`).
#[allow(clippy::too_many_arguments)]
fn pad(
    s: &App,
    opts: GridOpts,
    p: usize,
    settings: Option<&PadSettings>,
    is_selected: bool,
    hardware_note: Option<i64>,
    missing_title: Option<&str>,
    bank_gap: bool,
    size: [f32; 2],
) -> El {
    let th = t();
    // The dock's strip is a performance surface: a press plays the pad (and still selects it
    // for the editor) but leaves no lasting outline or fill on it, which read as a stuck focus
    // ring. Keyboard focus shows the ring below (`:focus-visible`) in both grids.
    let selected = is_selected && !opts.compact;
    let color_css = pad_color(settings, p);
    let color = css_color(&color_css, th.primary);
    let name = pad_name(settings);
    let empty = settings.is_none_or(|x| x.sample.is_none());
    let drag_over = opts.full
        && (s.beat.drop_over == Some(p)
            || s.sample_drag.as_ref().is_some_and(|d| d.over == Some(p)));
    let short = !opts.compact && size[1] > 0.0 && size[1] <= SHORT;
    let tall = !opts.compact && size[1] >= 80.0;
    let label = pad_label(p);

    let mut top = row()
        .items_start()
        .justify_between()
        .gap(4.0)
        .px(8.0)
        .pt(if opts.compact { 6.0 } else { 8.0 });
    let mut tag = row().min_w(0.0).items_center().gap(4.0).child(
        text(label.clone())
            .font(mono(11.0, 600.0).lh(11.0))
            .color(if selected { th.fg } else { th.muted_fg }),
    );
    if let Some(title) = missing_title
        && (opts.compact || short)
    {
        tag = tag.child(
            el().size(6.0, 6.0)
                .radius(3.0)
                .fill(th.warn)
                .help(title.to_owned()),
        );
    }
    top = top.child(tag);
    if !opts.compact {
        top = top.child(
            text(hardware_note_label(hardware_note.map(|n| n as f64)))
                .font(mono(11.0, 400.0).lh(11.0))
                .color(th.muted_fg)
                .help(match hardware_note {
                    Some(n) => format!("MIDI note {n}"),
                    None => "No MIDI note mapped".into(),
                }),
        );
    }
    let name_text = if drag_over {
        "Drop to load".to_owned()
    } else if name.is_empty() {
        "Empty".to_owned()
    } else {
        name.clone()
    };
    let name_font = if empty && !drag_over {
        ui(12.0, 500.0).lh(13.8).italic()
    } else {
        ui(12.0, 600.0).lh(13.8)
    };
    let mut name_el = text(name_text)
        .font(name_font)
        .color(if empty && !drag_over {
            th.muted_fg
        } else {
            th.fg
        })
        .max_lines(if tall { 2 } else { 1 })
        .px(8.0)
        .mt(0.0);
    if !name.is_empty() {
        name_el = name_el.help(name.clone());
    }
    if opts.compact {
        name_el = name_el.mb(6.0);
    } else if short {
        name_el = name_el.mb(8.0);
    }
    let mut body = col()
        .w_full()
        .h_full()
        .child(top)
        .child(spacer())
        .child(name_el);
    if !opts.compact && !short {
        let inner_w = (size[0] - 16.0).max(16.0);
        let mut wave = row().mx(8.0).mt(4.0).mb(6.0).h(18.0).items_end();
        wave = match settings.filter(|x| x.sample.is_some()) {
            Some(st) => wave.child(thumb(s, st, color, missing_title, inner_w)),
            None => wave.child(el().h(1.0).w_full().fill(th.hairline_strong)),
        };
        if opts.key_hints {
            wave = wave.child(
                text(PAD_KEY_HINTS[p])
                    .font(mono(11.0, 400.0).lh(14.0))
                    .color(th.muted_fg)
                    .px(4.0)
                    .radius(3.0)
                    .fill(th.surface0.alpha(0.9))
                    .pin(None, None, Some(0.0), Some(0.0)),
            );
        }
        body = body.child(wave);
    }
    let light = el()
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radial_gradient(
            [0.5, 1.0],
            [size[0].max(1.0) * 1.2, size[1].max(1.0) * 0.9],
            vec![(0.0, color), (1.0, color.alpha(0.55))],
        )
        .live_opacity(
            s.live.flash[p]
                .smooth(Spring::new(0.02, 0.0))
                .rest(Spring::new(0.26, 0.0)),
        );
    let strip = el()
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(3.0)
        .fill(color.alpha(if empty { 0.35 } else { 0.95 }));
    let a11y = if name.is_empty() {
        format!("Pad {label}, empty")
    } else {
        format!("Pad {label}: {name}")
    };
    let key = format!("{}{p}", opts.prefix);
    let mut b = col()
        .key(key)
        .role("gridcell")
        .label(a11y)
        // aria-selected (a gridcell's selection, not a checked state).
        .focus_selected(is_selected)
        .grow(1.0)
        .min_w(0.0)
        .min_h(0.0)
        .radius(7.0)
        .clip_rounded(7.0)
        .fill(if drag_over {
            th.surface4
        } else if selected {
            th.surface3
        } else {
            th.surface2
        })
        .hover(th.surface3)
        .focusable(true)
        // `focus-visible:ring-2 focus-visible:ring-ring/70`: keyboard focus only (a gridcell
        // draws no default ring; a pointer press shows none).
        .focus_ring(2.0, 0.0, th.primary.alpha(0.7))
        .child(light)
        .child(strip)
        .child(body)
        .live_scale(
            s.live.scale[p]
                .smooth(Spring::new(0.02, 0.0))
                .rest(Spring::new(0.14, 0.0)),
        );
    if let Some(title) = missing_title {
        b = b.description(title.to_owned());
    }
    b = if drag_over {
        b.box_shadow(inset(0.0, 0.0, 2.0, th.primary))
    } else if selected {
        b.box_shadow(inset(0.0, 0.0, 2.0, color))
            .box_shadow(Shadow {
                x: 0.0,
                y: 0.0,
                blur: 0.0,
                spread: 1.0,
                color: th.black.alpha(0.5),
                inset: false,
            })
    } else {
        b.box_shadow(inset(1.0, 0.0, 0.0, th.white.alpha(0.06)))
            .box_shadow(inset(-2.0, 0.0, 0.0, th.black.alpha(0.35)))
            .shadow(1.0, 2.0, th.black.alpha(0.4))
    };
    if bank_gap {
        b = b.ml(8.0);
    }
    b = b
        .on_drag(
            move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| match d.phase {
                Phase::Down => {
                    s.release_pad_hold();
                    s.beat.hold = Some(PadHold::Pointer(p));
                    s.select_pad(p);
                    let v = velocity_from_pointer(d.y as f64, 0.0, d.h as f64);
                    s.svc.drum_press(p, v);
                }
                Phase::Move => {}
                Phase::Up => {
                    if s.beat.hold == Some(PadHold::Pointer(p)) {
                        s.release_pad_hold();
                    }
                }
            },
        )
        .on_key(move |s: &mut App, ev: &KeyEvent, tx: &mut Tx| {
            let hold_key =
                matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(t) if t == " ");
            if !hold_key || ev.mods.cmd || ev.mods.ctrl || ev.mods.alt {
                return false;
            }
            // A pad the pointer clicked leaves Space to the transport (`pointer_focus_key`);
            // Return, and Space under keyboard focus, play the pad.
            if !matches!(&ev.key, Key::Enter) && !tx.focus_visible() && s.beat.hold.is_none() {
                return false;
            }
            if tx.key_repeat() || s.beat.hold.is_some() {
                return true;
            }
            s.beat.hold = Some(PadHold::Key(p));
            s.select_pad(p);
            s.svc.drum_press(p, 0.8);
            true
        })
        .on_key_up(move |s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
            let hold_key =
                matches!(&ev.key, Key::Enter) || matches!(&ev.key, Key::Text(t) if t == " ");
            if !hold_key {
                return false;
            }
            if s.beat.hold == Some(PadHold::Key(p)) {
                s.release_pad_hold();
            }
            true
        });
    if opts.full {
        b = b
            .on_secondary_press(move |s: &mut App, at: PointerPress, _tx: &mut Tx| {
                s.select_pad(p);
                s.beat.rename = rune::TextEdit::new("");
                s.menu = Some(Menu::Pad {
                    pad: p,
                    x: at.x,
                    y: at.y,
                    renaming: false,
                });
            })
            .on_drop_files(
                move |s: &mut App, files: &[std::path::PathBuf], _tx: &mut Tx| {
                    s.beat.drop_over = None;
                    if let Some(f) = files.first() {
                        let path = f.to_string_lossy().into_owned();
                        let name = mdaw_model::paths::display_name(
                            &f.file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                        );
                        s.assign_file(p, &path, &name);
                        s.select_pad(p);
                    }
                },
            )
            .accepts_extensions(&AUDIO_EXTENSIONS)
            .on_drop_hover(move |s: &mut App, over: bool, _tx: &mut Tx| {
                if over {
                    s.beat.drop_over = Some(p);
                } else if s.beat.drop_over == Some(p) {
                    s.beat.drop_over = None;
                }
            });
    } else {
        b = b.on_press(move |s: &mut App| s.select_pad(p));
    }
    b
}

fn inset(y: f32, x: f32, spread: f32, color: Color) -> Shadow {
    Shadow {
        x,
        y,
        blur: 0.0,
        spread,
        color,
        inset: true,
    }
}

/// `PadGrid`.
pub fn grid(s: &App, cx: &mut Cx<App>, opts: GridOpts, missing_titles: &[Option<String>]) -> El {
    let kit = s.kit();
    let selected = s.selected_pad();
    let notes: Vec<Option<i64>> = s
        .svc
        .midi
        .host
        .shared()
        .lock()
        .router
        .mapping
        .get_profile()
        .pad_map
        .notes
        .clone();
    let rows = match opts.bank {
        Some(b) => bank_rows(b),
        None => pad_rows(opts.layout),
    };
    let gap = if opts.compact { 4.0 } else { 6.0 };
    let mut g = col()
        .key(format!("{}grid", opts.prefix))
        .role("grid")
        .label("Pads")
        .gap(gap);
    for r in rows {
        let mut line = row().grow(1.0).min_h(0.0).gap(gap);
        for (c, &p) in r.iter().enumerate() {
            let size = cx
                .layout_size(&format!("{}{p}", opts.prefix))
                .unwrap_or([0.0, 0.0]);
            let title = missing_titles.get(p).and_then(|x| x.as_deref());
            line = line.child(pad(
                s,
                opts,
                p,
                kit.pads.get(p),
                p == selected,
                notes.get(p).copied().flatten(),
                title,
                opts.bank.is_none() && opts.layout == PadLayout::TwoByEight && c == 4,
                size,
            ));
        }
        g = g.child(line);
    }
    g
}
