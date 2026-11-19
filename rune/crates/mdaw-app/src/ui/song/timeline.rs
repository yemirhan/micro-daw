//! The ruler + lanes column of `ArrangementView`: `TimelineRuler` (ticks, labels, loop region
//! and handles, `MarkerFlag`s), the lanes (`TrackLane` grid, `RegionBlock`s,
//! `LiveRegionBlock`, `AutomationLaneView`), the rubber band and the `LivePlayhead`.
//!
//! The column scrolls itself (`SongUi::scroll`): everything is drawn in viewport points
//! (`x = beat × pxPerBeat − scrollLeft`), so only what shows is built. The wheel scrolls
//! (⌘ / Ctrl + wheel zooms about the pointer).

use mdaw_engine::engine::TransportState;
use mdaw_model::arrangement_helpers::snap_beat;
use mdaw_model::automation::get_automation_parameter_label;
use mdaw_model::doc::MarkerUpdate;
use mdaw_model::{AutomationParameter, Region, Track};
use rune::{
    Color, El, LineCap, Phase, PointerPress, Shape, Tx, WheelEvent, canvas, col, el, icon, row,
    text,
};

use crate::app::{App, Menu};
use crate::icons;
use crate::song::{AUTO_H, ArrTool, GROUP_H, RULER_H, Row, SongMenu, TRACK_H};
use crate::theme::{daw_label, mono, t, ui};
use crate::ui::song::{cmix, track_color};

/// Rect helper for shapes.
fn rect(sh: Shape, x: f32, y: f32, w: f32, h: f32, c: Color) -> Shape {
    if w <= 0.0 || h <= 0.0 {
        return sh;
    }
    sh.fill(vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]], c)
}

/// The wheel over the ruler / lanes: ⌘ / Ctrl zooms about the pointer, else scrolls.
pub fn timeline_wheel(s: &mut App, e: WheelEvent) -> bool {
    if e.mods.cmd || e.mods.ctrl {
        // `deltaY` of the DOM is the opposite of Rune's content delta.
        s.song_wheel_zoom(-e.dy as f64, e.x);
        return true;
    }
    s.song.scroll[0] -= e.dx;
    s.song.scroll[1] -= e.dy;
    s.clamp_song_scroll();
    true
}

pub fn view(s: &App) -> El {
    let th = t();
    let rolling = matches!(
        s.song_state(),
        TransportState::Playing | TransportState::Recording
    );
    let recording = s.song_state() == TransportState::Recording;
    let pc = if recording { th.rec } else { th.primary };
    let head = canvas(Shape::new().fill(
        vec![[0.0, 0.0], [11.0, 0.0], [11.0, 3.0], [5.5, 8.0], [0.0, 3.0]],
        pc,
    ))
    .absolute(-5.0, 0.0)
    .size(11.0, 8.0);
    let playhead = el()
        .key("song-playhead")
        .absolute(0.0, 0.0)
        .w(1.0)
        .h_full()
        .fill(pc)
        .opacity(if rolling { 1.0 } else { 0.55 })
        .live_offset(&s.song.live.playhead)
        .inert()
        .child(head);
    col()
        .key("song-timeline")
        .grow(1.0)
        .min_w(0.0)
        .h_full()
        .clip_rounded(0.0)
        .child(ruler(s))
        .child(lanes(s))
        .child(playhead)
}

// --- ruler -----------------------------------------------------------------------------------

fn ruler(s: &App) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let ppb = s.song.ppb;
    let sx = s.song.scroll[0];
    let [vw, _] = s.song.viewport.get();
    let len = doc.get_arrangement().length_beats;
    let x_of = |b: f64| (b * ppb) as f32 - sx;
    let mut r = el()
        .key("song-ruler")
        .h(RULER_H)
        .w_full()
        .fill(th.surface1)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .clip_rounded(0.0)
        .cursor(rune::Cursor::Pointer)
        .help("Click to move the playhead")
        .on_wheel(|s: &mut App, e: WheelEvent, _tx: &mut Tx| timeline_wheel(s, e))
        .on_drag(|s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up && !d.cancelled() && d.dx.abs() < 4.0 {
                // A click: clear the selection (the timeline's onClick) and seek.
                s.clear_selection();
                let x = d.x + s.song.scroll[0];
                s.ruler_seek(x);
            }
        });
    // Loop region.
    let loop_on = doc.is_loop_enabled();
    if let Some(m) = doc.get_loop_markers() {
        let lx = x_of(m.start_beat);
        let lw = ((m.end_beat - m.start_beat) * ppb) as f32;
        r = r.child(
            el().key("loop-region")
                .absolute(lx, 0.0)
                .size(lw.max(0.0), RULER_H - 1.0)
                .fill(if loop_on {
                    th.primary.alpha(0.14)
                } else {
                    th.fg.alpha(0.04)
                })
                .inert()
                .child(el().absolute(0.0, 0.0).w_full().h(3.0).fill(if loop_on {
                    th.primary
                } else {
                    th.muted_fg.alpha(0.4)
                })),
        );
    }
    // Ticks and labels (visible range only).
    let bar_px = ppb * 4.0;
    let label_every = [1i64, 2, 4, 8, 16, 32]
        .into_iter()
        .find(|n| *n as f64 * bar_px >= 32.0)
        .unwrap_or(64);
    let first = ((sx as f64 / ppb).floor() as i64 - 1).max(0);
    let last = (((sx + vw) as f64 / ppb).ceil() as i64 + 1).min(len.floor() as i64);
    let mut ticks = Shape::new();
    for beat in first..=last {
        let x = x_of(beat as f64);
        if beat % 4 == 0 {
            ticks = rect(ticks, x, RULER_H - 13.0, 1.0, 12.0, th.muted_fg.alpha(0.45));
            let bar = beat / 4;
            if bar % label_every == 0 {
                r = r.child(
                    text(format!("{}", bar + 1))
                        .font(mono(10.0, 400.0))
                        .color(th.muted_fg)
                        .absolute(x + 4.0, RULER_H - 15.0)
                        .inert(),
                );
            }
        } else if ppb >= 14.0 {
            ticks = rect(ticks, x, RULER_H - 6.0, 1.0, 5.0, th.muted_fg.alpha(0.25));
        }
    }
    r = r.child(canvas(ticks).absolute(0.0, 0.0).size(vw, RULER_H).inert());
    // Loop handles (only while the loop is on).
    if loop_on && let Some(m) = doc.get_loop_markers() {
        let (sxp, exp) = (x_of(m.start_beat), x_of(m.end_beat));
        r = r
            .child(loop_handle("loop-start", sxp - 1.0, true))
            .child(loop_handle("loop-end", exp - 9.0, false));
    }
    // Marker flags.
    for m in doc.get_markers() {
        let beat = match &s.song.marker_drag {
            Some((id, b, true)) if *id == m.id => *b,
            _ => m.beat,
        };
        r = r.child(marker_flag(s, &m.id, &m.name, &m.color, x_of(beat)));
    }
    r
}

fn loop_handle(key: &str, x: f32, start: bool) -> El {
    let th = t();
    let tri = if start {
        vec![[0.0, 0.0], [8.0, 0.0], [0.0, 8.0]]
    } else {
        vec![[2.0, 0.0], [10.0, 0.0], [10.0, 8.0]]
    };
    el().key(key.to_owned())
        .absolute(x, 0.0)
        .size(10.0, RULER_H - 1.0)
        .cursor(rune::Cursor::ResizeColumn)
        .help(if start {
            "Drag loop start"
        } else {
            "Drag loop end"
        })
        .child(
            el().absolute(if start { 0.0 } else { 8.0 }, 0.0)
                .size(2.0, RULER_H - 1.0)
                .fill(th.primary),
        )
        .child(
            canvas(Shape::new().fill(tri, th.primary))
                .absolute(0.0, 0.0)
                .size(10.0, 8.0),
        )
        .on_drag(
            move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| match d.phase {
                Phase::Down => {
                    // Content x of the press, kept for the gesture.
                    d.data = x + d.x + s.song.scroll[0];
                    s.begin_loop_drag(if start {
                        crate::song::LoopEdge::Start
                    } else {
                        crate::song::LoopEdge::End
                    });
                }
                Phase::Move => s.move_loop_drag(d.data + d.dx),
                Phase::Up => s.end_loop_drag(),
            },
        )
}

fn marker_flag(s: &App, id: &str, name: &str, color: &str, x: f32) -> El {
    let c = track_color(color);
    let _ = s;
    let idm = id.to_owned();
    let idc = id.to_owned();
    // The line and the label share one hit area (the DOM hit-tests the label, positioned
    // outside the 1-point flag, too).
    row()
        .key(format!("marker-{id}"))
        .absolute(x - 0.5, 0.0)
        .h(RULER_H)
        .items_start()
        .cursor(rune::Cursor::Grab)
        .help(format!(
            "{name}: click to jump, drag to move, right-click to edit"
        ))
        .child(el().absolute(0.0, 0.0).size(1.0, RULER_H).fill(c))
        .child(
            row()
                .key(format!("marker-label-{id}"))
                .mt(2.0)
                .max_w(96.0)
                .pl(4.0)
                .pr(6.0)
                .py(2.0)
                .corners([0.0, 3.0, 3.0, 0.0])
                .fill(c)
                .child(
                    text(name.to_owned())
                        .font(ui(9.5, 700.0))
                        .color(Color::BLACK.alpha(0.85))
                        .truncate(),
                ),
        )
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            let len = s.svc.engine.doc().get_arrangement().length_beats;
            match d.phase {
                Phase::Down => {
                    d.data = x + d.x + s.song.scroll[0];
                    let b = s
                        .svc
                        .engine
                        .doc()
                        .get_markers()
                        .iter()
                        .find(|m| m.id == idm)
                        .map_or(0.0, |m| m.beat);
                    s.song.marker_drag = Some((idm.clone(), b, false));
                }
                Phase::Move => {
                    let beat =
                        snap_beat((d.data + d.dx) as f64 / s.song.ppb, s.song.snap).clamp(0.0, len);
                    s.song.marker_drag = Some((idm.clone(), beat, true));
                }
                Phase::Up => {
                    if let Some((mid, beat, moved)) = s.song.marker_drag.take() {
                        if moved && !d.cancelled() {
                            s.update_marker(
                                &mid,
                                MarkerUpdate {
                                    beat: Some(beat),
                                    ..Default::default()
                                },
                            );
                        } else if !moved {
                            s.seek_to_marker(&mid);
                        }
                    }
                }
            }
        })
        .on_secondary_press(move |s: &mut App, at: PointerPress, _tx: &mut Tx| {
            s.song.marker_rename = rune::TextEdit::new("");
            s.menu = Some(Menu::Song(SongMenu::Marker {
                x: at.x,
                y: at.y,
                marker: idc.clone(),
                renaming: false,
            }));
        })
}

// --- lanes -----------------------------------------------------------------------------------

/// Grid lines + alternate-bar shading over `[y0, y1)` of the viewport (`laneGridStyle`).
fn lane_grid(mut sh: Shape, s: &App, y0: f32, y1: f32, vw: f32) -> Shape {
    let ppb = s.song.ppb;
    let sx = s.song.scroll[0];
    let len = s.svc.engine.doc().get_arrangement().length_beats;
    let content_w = (len * ppb) as f32 - sx;
    let right = vw.min(content_w);
    if right <= 0.0 || y1 <= y0 {
        return sh;
    }
    let bar = ppb * 4.0;
    let first_bar = ((sx as f64 / bar).floor() as i64).max(0);
    let last_bar = ((sx + vw) as f64 / bar).ceil() as i64;
    for k in first_bar..=last_bar {
        let x = (k as f64 * bar) as f32 - sx;
        if k % 2 == 1 {
            let w = (bar as f32).min(right - x);
            sh = rect(
                sh,
                x.max(0.0),
                y0,
                w - (x.max(0.0) - x),
                y1 - y0,
                Color::WHITE.alpha(0.012),
            );
        }
        if x >= 0.0 && x < right {
            sh = rect(sh, x, y0, 1.0, y1 - y0, Color::WHITE.alpha(0.075));
        }
        if ppb >= 14.0 {
            for b in 1..4 {
                let bx = x + (b as f64 * ppb) as f32;
                if bx >= 0.0 && bx < right {
                    sh = rect(sh, bx, y0, 1.0, y1 - y0, Color::WHITE.alpha(0.03));
                }
            }
        }
    }
    sh
}

fn lanes(s: &App) -> El {
    let th = t();
    let doc = s.svc.engine.doc();
    let [vw, vh] = s.song.viewport.get();
    let [sx, sy] = s.song.scroll;
    let rows = s.song_rows();
    let mut bg = Shape::new();
    let mut l = el()
        .key("song-lanes")
        .role("listbox")
        .label("Regions")
        .grow(1.0)
        .w_full()
        .min_h(0.0)
        .clip_rounded(0.0)
        .on_wheel(|s: &mut App, e: WheelEvent, _tx: &mut Tx| timeline_wheel(s, e))
        .on_drag(|s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
            let at = [d.x + s.song.scroll[0], d.y + s.song.scroll[1]];
            match d.phase {
                Phase::Down => s.band_down(at),
                Phase::Move => s.band_move(at),
                Phase::Up => s.band_up(tx.mods().shift),
            }
        });
    let mut fg: Vec<El> = Vec::new();
    // Selected regions draw (and hit-test) above the others, as the TS `z-[2]`; each one's
    // ring is an inert overlay outside its box (the TS `box-shadow: 0 0 0 1.5px`), drawn last.
    let mut selected_fg: Vec<El> = Vec::new();
    let mut rings: Vec<El> = Vec::new();
    let selected = s.song.selected_set();
    let mut bottom = 0.0f32;
    let recording_track = s.svc.engine.get_recording_track_id().map(str::to_owned);
    for r in rows.iter() {
        match r {
            Row::Group { id, top } => {
                let y = top - sy;
                bottom = top + GROUP_H;
                if y > vh || y + GROUP_H < 0.0 {
                    continue;
                }
                let gc = doc
                    .get_groups()
                    .iter()
                    .find(|g| &g.id == id)
                    .map_or(th.primary, |g| track_color(&g.color));
                bg = rect(bg, 0.0, y, vw, GROUP_H - 1.0, cmix(gc, th.surface0, 0.06));
                bg = rect(bg, 0.0, y + GROUP_H - 1.0, vw, 1.0, th.border);
            }
            Row::Track { id, top, height } => {
                let y = top - sy;
                bottom = top + height;
                if y > vh || y + height < 0.0 {
                    continue;
                }
                let Some(track) = s.find_track(id) else {
                    continue;
                };
                bg = lane_grid(bg, s, y, y + TRACK_H - 1.0, vw);
                bg = rect(bg, 0.0, y + TRACK_H - 1.0, vw, 1.0, th.border);
                for region in &track.regions {
                    let x = (region.start_beat * s.song.ppb) as f32 - sx;
                    let w = (region.length_beats * s.song.ppb) as f32;
                    if x > vw || x + w.max(4.0) < 0.0 {
                        continue;
                    }
                    let sel = selected.contains(region.id.as_str());
                    let block = region_block(s, track, region, x, y + 3.0, w, sel);
                    if sel {
                        selected_fg.push(block);
                        rings.push(selection_ring(&region.id, x, y + 3.0, w.max(4.0)));
                    } else {
                        fg.push(block);
                    }
                }
                if recording_track.as_deref() == Some(id.as_str())
                    && let Some(live) = s.svc.engine.get_live_region()
                {
                    let x = (live.start_beat * s.song.ppb) as f32 - sx;
                    let w = (live.length_beats * s.song.ppb) as f32;
                    s.song
                        .live
                        .rec_rect
                        .snap([x, y + 3.0, w.max(4.0), TRACK_H - 6.0]);
                    fg.push(live_region(s, track, &live, vw));
                }
                for (i, lane) in crate::song::visible_lanes(track).into_iter().enumerate() {
                    let ly = y + TRACK_H + i as f32 * AUTO_H;
                    fg.push(automation_lane(s, track, lane, ly, vw));
                }
            }
        }
    }
    // The grid continues below the last track.
    let below = bottom - sy;
    if below < vh {
        bg = lane_grid(bg, s, below.max(0.0), vh, vw);
    }
    l = l.child(canvas(bg).absolute(0.0, 0.0).size(vw, vh).inert());
    for e in fg.into_iter().chain(selected_fg).chain(rings) {
        l = l.child(e);
    }
    if let Some(b) = s.song.band
        && b.active
    {
        let x = b.start[0].min(b.current[0]) - sx;
        let y = b.start[1].min(b.current[1]) - sy;
        l = l.child(
            el().key("rubber-band")
                .absolute(x, y)
                .size(
                    (b.current[0] - b.start[0]).abs(),
                    (b.current[1] - b.start[1]).abs(),
                )
                .radius(2.0)
                .border(1.0, th.select.alpha(0.8))
                .fill(th.select.alpha(0.1))
                .inert(),
        );
    }
    l
}

/// The region's colour desaturated like CSS `saturate(0.3)` (muted tracks).
fn desaturate(c: Color, amount: f32) -> Color {
    let [l, a, b, al] = c.to_oklab();
    Color::from_oklab([l, a * amount, b * amount, al])
}

/// `NoteMinimap` inside a region body `w × h`.
fn note_minimap(region: &Region, color: Color, w: f32, h: f32) -> Option<El> {
    let notes = &region.notes;
    if notes.is_empty() || region.length_beats <= 0.0 {
        return None;
    }
    let is_drum = notes.iter().any(|n| n.is_drum);
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for n in notes {
        lo = lo.min(n.note);
        hi = hi.max(n.note);
    }
    let pad = if is_drum { 0.0 } else { 2.0 };
    let lo = (lo - pad).max(0.0);
    let hi = (hi + pad).min(127.0);
    let range = (hi - lo + 1.0).max(if is_drum { 4.0 } else { 8.0 });
    let nc = cmix(color, Color::WHITE, 0.55);
    let (iw, ih) = ((w - 8.0).max(0.0), (h - 8.0).max(0.0));
    let mut sh = Shape::new();
    for n in notes {
        let left = (n.start_beat / region.length_beats) as f32 * iw;
        let width = ((n.duration_beats / region.length_beats) as f32 * iw)
            .max(iw * if is_drum { 0.006 } else { 0.008 });
        let bottom = ((n.note - lo) / range) as f32 * ih;
        let hh = (ih * (100.0 / range as f32).min(18.0) / 100.0).max(2.0);
        let v = if n.velocity > 1.0 {
            n.velocity / 127.0
        } else {
            n.velocity
        };
        let op = 0.55 + 0.45 * v.min(1.0) as f32;
        sh = rect(sh, left, ih - bottom - hh, width, hh, nc.alpha(op));
    }
    Some(
        canvas(sh)
            .absolute(4.0, 4.0)
            .size(iw, ih)
            .clip_rounded(0.0)
            .inert(),
    )
}

/// `SAMPLE_PROBLEM_TITLE`.
pub const SAMPLE_UNAVAILABLE: &str = "Sample file unavailable (its pack was removed): this clip plays the copy loaded earlier, but it won't load after reopening the project. Reinstall the pack to keep it.";
pub const SAMPLE_MISSING: &str = "Sample file not found: this clip is silent. Reinstall its pack or put the file back, then reopen the project.";

/// The selected region's ring: 1.5 points outside its box, over everything (inert).
fn selection_ring(id: &str, x: f32, y: f32, w: f32) -> El {
    el().key(format!("region-{id}-ring"))
        .absolute(x - 1.5, y - 1.5)
        .size(w + 3.0, TRACK_H - 6.0 + 3.0)
        .radius(6.5)
        .border(1.5, t().select)
        .inert()
}

fn region_block(
    s: &App,
    track: &Track,
    region: &Region,
    x: f32,
    y: f32,
    w: f32,
    selected: bool,
) -> El {
    let th = t();
    let dragging = s
        .song
        .region_drag
        .as_ref()
        .is_some_and(|d| d.region == region.id);
    let base = track_color(region.color.as_deref().unwrap_or(&track.color));
    let color = if track.muted {
        desaturate(base, 0.3)
    } else {
        base
    };
    let h = TRACK_H - 6.0;
    let dw = w.max(4.0);
    let show_header = w >= 18.0;
    // Sample status of audio regions: the Rust app has no sample manager yet, so an audio
    // clip's sample is never loaded ('missing') unless the pack store marks it unavailable.
    let problem = region.audio.as_ref().map(|a| {
        if s.svc.engine.is_sample_unavailable(&a.sample_id) {
            "unavailable"
        } else {
            "missing"
        }
    });
    // A selectable item of the lanes' list ("option" under the `listbox` lanes): selection reads
    // as `aria-selected`, which a button can't carry.
    let mut b = el()
        .key(format!("region-{}", region.id))
        .role("option")
        .label(region.name.clone().unwrap_or_else(|| {
            if region.audio.is_some() {
                "Audio"
            } else {
                "Clip"
            }
            .to_owned()
        }))
        // `aria-selected` (`data-selected` in the TS), not a checked state.
        .focus_selected(selected)
        .absolute(x, y)
        .size(dw, h)
        .radius(5.0)
        .clip_rounded(5.0)
        .fill(cmix(color, th.surface1, if selected { 0.34 } else { 0.24 }))
        .opacity(if dragging {
            0.75
        } else if track.muted {
            0.45
        } else {
            1.0
        })
        .cursor(if s.song.tool == ArrTool::Cut {
            rune::Cursor::Crosshair
        } else {
            rune::Cursor::Grab
        });
    b = if selected {
        // The ring is `selection_ring`, drawn over the region and its header strip.
        b.box_shadow(rune::Shadow {
            x: 0.0,
            y: 2.0,
            blur: 6.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.35),
            inset: false,
        })
    } else {
        b.border(1.0, color.alpha(0.55)).box_shadow(rune::Shadow {
            x: 0.0,
            y: 1.0,
            blur: 2.0,
            spread: 0.0,
            color: Color::BLACK.alpha(0.3),
            inset: false,
        })
    };
    if let Some(p) = problem {
        b = b.help(format!(
            "{}: {}",
            region.name.clone().unwrap_or_else(|| "Audio".into()),
            if p == "missing" {
                SAMPLE_MISSING
            } else {
                SAMPLE_UNAVAILABLE
            }
        ));
    } else if let Some(n) = &region.name {
        b = b.help(n.clone());
    }
    let mut body_top = 0.0;
    if show_header {
        let mut hdr = row()
            .absolute(0.0, 0.0)
            .size(dw, 15.0)
            .items_center()
            .px(6.0)
            .fill(cmix(color, th.surface1, if selected { 1.0 } else { 0.86 }))
            .inert()
            .child(
                text(region.name.clone().unwrap_or_else(|| {
                    if region.audio.is_some() {
                        "Audio"
                    } else {
                        "Clip"
                    }
                    .to_owned()
                }))
                .font(ui(10.0, 600.0))
                .color(Color::BLACK.alpha(0.8))
                .truncate(),
            );
        if let Some(p) = problem {
            hdr = hdr.child(rune::spacer()).child(badge(p, 12.0));
        }
        b = b.child(hdr);
        body_top = 15.0;
    } else if let Some(p) = problem {
        b = b.child(badge(p, 13.0).absolute(dw - 15.0, 2.0));
    }
    if region.audio.is_none()
        && let Some(m) = note_minimap(region, color, dw, h - body_top)
    {
        b = b.child(
            el().absolute(0.0, body_top)
                .size(dw, h - body_top)
                .inert()
                .child(m),
        );
    }
    // Edge zones (resize cursor) and their hover hints.
    if dw >= 12.0 && s.song.tool == ArrTool::Pointer {
        for (k, ex) in [("l", 0.0), ("r", dw - 6.0)] {
            b = b.child(
                el().key(format!("region-{}-edge-{k}", region.id))
                    .absolute(ex, 0.0)
                    .size(6.0, h)
                    .cursor(rune::Cursor::ResizeColumn),
            );
        }
    }
    let (tid, rid) = (track.id.clone(), region.id.clone());
    let (tid2, rid2) = (tid.clone(), rid.clone());
    let (tid3, rid3) = (tid.clone(), rid.clone());
    b.on_drag(
        move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| match d.phase {
            Phase::Down => {
                if s.song.tool == ArrTool::Cut {
                    s.cut_region_at(&tid, &rid, d.x);
                    d.data = -1.0;
                    return;
                }
                s.begin_region_drag(&tid, &rid, d.x, d.w, tx.mods().shift);
            }
            Phase::Move => {
                if d.data >= 0.0 {
                    s.move_region_drag(d.dx);
                }
            }
            Phase::Up => s.end_region_drag(),
        },
    )
    .on_double_press(move |s: &mut App| s.open_piano_roll(&tid2, &rid2))
    .on_secondary_press(move |s: &mut App, at: PointerPress, _tx: &mut Tx| {
        s.menu = Some(Menu::Song(SongMenu::Region {
            x: at.x,
            y: at.y,
            track: tid3.clone(),
            region: rid3.clone(),
        }));
    })
}

fn badge(state: &str, size: f32) -> El {
    let th = t();
    row()
        .key(format!("sample-badge-{state}"))
        .role("img")
        .label(if state == "missing" {
            "Sample file not found"
        } else {
            "Sample file unavailable"
        })
        .size(size, size)
        .center()
        .radius(3.0)
        .fill(th.warn)
        .child(icon(&icons::TRIANGLE_ALERT, 9.0).color(Color::BLACK))
}

/// `LiveRegionBlock`: the region being recorded. Its rect is `live.rec_rect`, moved every
/// frame by `App::song_poll` as the take grows (no rebuild); the children are laid out for the
/// whole lanes width and clipped by the growing box. The note preview is drawn in beat points
/// (`start × pxPerBeat`), which is what the TS percentages of the growing length come to.
fn live_region(s: &App, track: &Track, r: &Region, vw: f32) -> El {
    let th = t();
    let color = track_color(r.color.as_deref().unwrap_or(&track.color));
    let [x, y, _, _] = s.song.live.rec_rect.get();
    let dw = (vw - x).max(4.0);
    let h = TRACK_H - 6.0;
    let mut b = el()
        .key("live-region")
        .absolute(x, y)
        .size(dw, h)
        .live_rect(&s.song.live.rec_rect)
        .radius(5.0)
        .clip_rounded(5.0)
        .fill(cmix(th.rec, th.surface1, 0.16))
        .border(1.0, th.rec.alpha(0.7))
        .inert()
        .child(
            row()
                .absolute(0.0, 0.0)
                .size(dw, 15.0)
                .items_center()
                .gap(4.0)
                .px(6.0)
                .fill(th.rec)
                .child(el().size(6.0, 6.0).radius(3.0).fill(Color::WHITE))
                .child(
                    text("REC")
                        .font(ui(10.0, 700.0).tracking(0.6))
                        .color(Color::WHITE),
                ),
        );
    // The minimap of a region `len` beats long laid out over `len × ppb` points.
    let content_w = (r.length_beats * s.song.ppb) as f32;
    if let Some(m) = note_minimap(r, color, content_w.max(4.0), h - 15.0) {
        b = b.child(
            el().absolute(0.0, 15.0)
                .size(content_w.max(4.0), h - 15.0)
                .child(m),
        );
    }
    b
}

/// `AutomationLaneView`: guides, area + line, points (right-click deletes), click adds.
fn automation_lane(
    s: &App,
    track: &Track,
    lane: &mdaw_model::AutomationLane,
    y: f32,
    vw: f32,
) -> El {
    let th = t();
    let ppb = s.song.ppb;
    let sx = s.song.scroll[0];
    let tc = track_color(&track.color);
    let param = lane.parameter;
    let pkey = format!("{param:?}").to_lowercase();
    let mut sh = Shape::new();
    for f in [0.25f32, 0.5, 0.75] {
        sh = rect(sh, 0.0, AUTO_H * f, vw, 1.0, th.fg.alpha(0.06));
    }
    let pts: Vec<[f32; 2]> = lane
        .points
        .iter()
        .map(|p| [(p.beat * ppb) as f32 - sx, (1.0 - p.value as f32) * AUTO_H])
        .collect();
    if pts.len() > 1 {
        let mut area = pts.clone();
        area.push([pts[pts.len() - 1][0], AUTO_H]);
        area.push([pts[0][0], AUTO_H]);
        sh = sh.fill(area, tc.alpha(0.12)).stroke_with(
            pts.clone(),
            false,
            1.5,
            LineCap::Butt,
            tc.alpha(0.9),
        );
    }
    let tid = track.id.clone();
    let mut l = el()
        .key(format!("auto-{}-{pkey}", track.id))
        .absolute(0.0, y)
        .size(vw, AUTO_H)
        .fill(th.surface0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .cursor(rune::Cursor::Crosshair)
        .child(canvas(sh).absolute(0.0, 0.0).size(vw, AUTO_H).inert())
        .child(
            text(get_automation_parameter_label(param))
                .font(daw_label(9.5))
                .text_case(rune::TextCase::Uppercase)
                .color(cmix(tc, Color::WHITE, 0.7))
                .absolute(6.0 - sx, 4.0)
                .inert(),
        )
        .on_drag(move |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| {
            if d.phase == Phase::Up && !d.cancelled() && d.dx.abs() < 4.0 && d.dy.abs() < 4.0 {
                let x = d.x + s.song.scroll[0];
                let beat = snap_beat(x as f64 / s.song.ppb, s.song.snap);
                let v = (1.0 - d.y as f64 / AUTO_H as f64).clamp(0.0, 1.0);
                s.set_automation_point(&tid, param, beat, v);
            }
        });
    for (i, p) in pts.iter().enumerate() {
        let tid = track.id.clone();
        l = l.child(
            el().key(format!("auto-{}-{pkey}-pt-{i}", track.id))
                .absolute(p[0] - 4.25, p[1] - 4.25)
                .size(8.5, 8.5)
                .radius(4.25)
                .fill(tc)
                .border(1.5, th.surface0)
                .cursor(rune::Cursor::Pointer)
                .on_secondary_press(move |s: &mut App, _at: PointerPress, _tx: &mut Tx| {
                    s.delete_automation_point(&tid, param, i);
                }),
        );
    }
    l
}

/// The parameter of an automation-lane key (tests).
pub fn param_key(p: AutomationParameter) -> String {
    format!("{p:?}").to_lowercase()
}
