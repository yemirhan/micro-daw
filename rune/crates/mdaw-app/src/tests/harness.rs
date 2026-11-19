use rune::{Env, Key, KeyEvent, Mods, PhysicalInput, Runtime};

use crate::app::App;
use crate::{keys, scenes, ui};

pub const DT: f64 = 1.0 / 60.0;

fn poll(s: &mut App, t: f64, tx: &mut rune::Tx) -> bool {
    s.poll(t, tx)
}

/// A started app (start screen gone, browser closed) in a headless runtime.
pub fn rt() -> Runtime<App> {
    let mut app = App::headless();
    scenes::started(&mut app);
    runtime(app)
}

/// The production text system: the app's embedded Manrope / JetBrains Mono / Syne under the
/// sans / mono slots (`AppSpec::fonts` / `families`), so layout and wrapping match the app.
pub fn text_system() -> rune::text::TextSystem {
    rune::text::TextSystem::new(crate::theme::FONTS, crate::theme::FAMILIES)
}

pub fn runtime(app: App) -> Runtime<App> {
    let mut rt = Runtime::with_text(
        app,
        ui::view,
        (1440.0, 900.0),
        Env::new(true, 1.0),
        text_system(),
    );
    rt.poll = Some(poll);
    rt.on_key = Some(keys::on_key);
    rt.frame(0.0);
    frames(&mut rt, 3);
    rt
}

pub fn frames(rt: &mut Runtime<App>, n: usize) {
    for _ in 0..n {
        rt.frame(DT);
    }
}

/// Runs `seconds` of virtual time (the offline engine follows the clock).
pub fn run(rt: &mut Runtime<App>, seconds: f64) {
    frames(rt, (seconds / DT).ceil() as usize);
}

pub fn at(rt: &Runtime<App>, key: &str) -> (f32, f32) {
    rt.find(key).unwrap_or_else(|| panic!("no element {key}"))
}

pub fn rect(rt: &Runtime<App>, key: &str) -> [f32; 4] {
    rt.presented(key)
        .unwrap_or_else(|| panic!("no element {key}"))
        .0
}

pub fn has(rt: &Runtime<App>, key: &str) -> bool {
    rt.find(key).is_some()
}

pub fn click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = at(rt, key);
    click_at(rt, x, y);
}

pub fn click_at(rt: &mut Runtime<App>, x: f32, y: f32) {
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.frame(DT);
    rt.pointer_up(x, y);
    frames(rt, 2);
}

pub fn double_click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = at(rt, key);
    rt.pointer_move(x, y);
    for _ in 0..2 {
        rt.pointer_down(x, y);
        rt.frame(0.02);
        rt.pointer_up(x, y);
        rt.frame(0.02);
    }
    frames(rt, 2);
}

/// Press at `(x, y)`, move by `(dx, dy)` in steps, release.
pub fn drag(rt: &mut Runtime<App>, x: f32, y: f32, dx: f32, dy: f32) {
    rt.pointer_move(x, y);
    rt.pointer_down(x, y);
    rt.frame(DT);
    for i in 1..=8 {
        let f = i as f32 / 8.0;
        rt.pointer_move(x + dx * f, y + dy * f);
        rt.frame(DT);
    }
    rt.pointer_up(x + dx, y + dy);
    frames(rt, 2);
}

pub fn right_click(rt: &mut Runtime<App>, key: &str) {
    let (x, y) = at(rt, key);
    rt.pointer_move(x, y);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    frames(rt, 2);
}

pub fn ev(key: Key, mods: Mods) -> KeyEvent {
    KeyEvent { key, mods }
}

pub fn text_key(c: &str) -> KeyEvent {
    ev(Key::Text(c.into()), Mods::default())
}

pub fn shift() -> Mods {
    Mods {
        shift: true,
        ..Mods::default()
    }
}

pub fn cmd() -> Mods {
    Mods {
        cmd: true,
        logo: cfg!(target_os = "macos"),
        ctrl: !cfg!(target_os = "macos"),
        ..Mods::default()
    }
}

pub fn press(rt: &mut Runtime<App>, e: KeyEvent) {
    rt.key(e);
    frames(rt, 2);
}

pub fn release(rt: &mut Runtime<App>, e: KeyEvent) {
    rt.key_up(e);
    frames(rt, 2);
}

/// Holds modifier keys (pointer handlers read them from `Tx::mods`).
pub fn hold_mods(rt: &mut Runtime<App>, m: Mods) {
    rt.physical_input(PhysicalInput::Modifiers(m));
}

pub fn undo_label(rt: &Runtime<App>) -> Option<String> {
    rt.state
        .svc
        .engine
        .doc()
        .undo_manager()
        .undo_label()
        .map(str::to_owned)
}

/// The active drum track's pattern notes as `(pad, beat, velocity)`.
pub fn notes(rt: &Runtime<App>) -> Vec<(usize, f64, f64)> {
    let doc = rt.state.svc.engine.doc();
    let Some(id) = doc.get_active_drum_track_id() else {
        return vec![];
    };
    let Some(r) = doc.get_pattern_region(&id) else {
        return vec![];
    };
    let mut v: Vec<(usize, f64, f64)> = r
        .notes
        .iter()
        .map(|n| {
            (
                (n.note - 36.0) as usize,
                r.start_beat + n.start_beat,
                n.velocity,
            )
        })
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

pub fn pad(rt: &Runtime<App>, p: usize) -> mdaw_model::types::PadSettings {
    rt.state.pad_settings(p).unwrap()
}

/// One notched mouse-wheel step at a window point (`dy > 0` = wheel up, the DOM's `deltaY < 0`).
pub fn wheel(rt: &mut Runtime<App>, x: f32, y: f32, dy: f32) {
    rt.pointer_move(x, y);
    rt.wheel_event(
        rune::WheelEvent::new(0.0, dy)
            .at(x, y)
            .notch(true)
            .precise(false),
    );
    frames(rt, 2);
}

/// Texts of the elements on screen (exiting ones left out).
pub fn texts(rt: &Runtime<App>) -> Vec<String> {
    rt.elements()
        .into_iter()
        .filter(|e| !e.exiting)
        .filter_map(|e| e.text)
        .collect()
}

/// The rock-song template in the Song view (`scenes_song::song`) in a headless runtime.
pub fn song_rt() -> Runtime<App> {
    let mut app = App::headless();
    crate::scenes_song::song(&mut app);
    let mut rt = runtime(app);
    run(&mut rt, 0.3);
    rt
}

pub fn tid(rt: &Runtime<App>, name: &str) -> String {
    crate::scenes_song::track_id(&rt.state, name)
}

pub fn rid(rt: &Runtime<App>, track: &str, i: usize) -> String {
    rt.state.find_track(track).unwrap().regions[i].id.clone()
}

pub fn region(rt: &Runtime<App>, track: &str, id: &str) -> mdaw_model::Region {
    rt.state
        .find_track(track)
        .unwrap()
        .regions
        .iter()
        .find(|r| r.id == id)
        .unwrap()
        .clone()
}

pub fn undo_depth(rt: &Runtime<App>) -> usize {
    rt.state.svc.engine.doc().undo_manager().undo_len()
}

/// Press at a window point, move by (dx, dy) in steps with modifiers held, release.
pub fn drag_mods(rt: &mut Runtime<App>, x: f32, y: f32, dx: f32, dy: f32, m: Mods) {
    hold_mods(rt, m);
    drag(rt, x, y, dx, dy);
    hold_mods(rt, Mods::default());
}

pub fn click_mods(rt: &mut Runtime<App>, x: f32, y: f32, m: Mods) {
    hold_mods(rt, m);
    click_at(rt, x, y);
    hold_mods(rt, Mods::default());
}

pub fn alt() -> Mods {
    Mods {
        alt: true,
        ..Mods::default()
    }
}

pub fn right_click_at(rt: &mut Runtime<App>, x: f32, y: f32) {
    rt.pointer_move(x, y);
    rt.secondary_down(x, y);
    rt.secondary_up(x, y);
    // Let the menu's layout springs land before anything measures it.
    run(rt, 0.6);
}

/// Click then let layout springs settle (menus / popovers opening).
pub fn click_settle(rt: &mut Runtime<App>, key: &str) {
    click(rt, key);
    run(rt, 0.6);
}

/// A wheel step with modifiers (`dy > 0` = wheel up).
pub fn wheel_mods(rt: &mut Runtime<App>, x: f32, y: f32, dx: f32, dy: f32, m: Mods) {
    rt.pointer_move(x, y);
    rt.wheel_event(
        rune::WheelEvent::new(dx, dy)
            .at(x, y)
            .mods(m)
            .notch(true)
            .precise(false),
    );
    frames(rt, 2);
}
