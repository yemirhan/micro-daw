//! The modal dialogs: `CommandPalette` (+ `config/commands.ts`), `ExportDialog`,
//! `TemplatePicker` and `ControllerSetupDialog`.

use mdaw_model::beat_helpers::pad_label;
use mdaw_model::controller::{
    CCMappingPatch, KNOB_TARGET_OPTIONS, LearnState, MidiActivity, knob_target_label,
    same_knob_target,
};
use mdaw_model::notes::midi_to_note_name;
use mdaw_model::types::{ControllerProfile, KnobTarget, PADS_PER_BANK};
use rune::{Align, Color, El, Icon, Tx, col, el, icon, row, spacer, text, text_input};
use std::collections::HashMap;

use crate::app::{App, AppMode, DawView};
use crate::icons;
use crate::icons_song;
use crate::icons_views;
use crate::project::ProjectOp;
use crate::theme::{R_LG, R_MD, R_SM, css_color, mono, t, ui};
use crate::ui::p9::kit::{dialog_layer, dialog_panel};

use crate::ui::widgets::{Button, Size, Variant, kbd, popover_frame, select_option, select_popup};
use crate::views::{Dialog, ExportKind, P9Menu, templates};

// ------------------------------------------------------------------------------- palette

/// One command of `STATIC_COMMANDS`.
pub struct Command {
    pub id: String,
    pub label: String,
    pub group: &'static str,
    pub icon: &'static Icon,
    pub shortcut: Option<&'static str>,
    pub keywords: String,
    pub daw_only: bool,
    /// Recent projects: the file path under the label (`subtitle`).
    pub subtitle: Option<String>,
}

fn cmd(
    id: &str,
    label: &str,
    group: &'static str,
    ic: &'static Icon,
    shortcut: Option<&'static str>,
    keywords: &str,
    daw_only: bool,
) -> Command {
    Command {
        id: id.to_owned(),
        label: label.to_owned(),
        group,
        icon: ic,
        shortcut,
        keywords: keywords.to_owned(),
        daw_only,
        subtitle: None,
    }
}

/// `STATIC_COMMANDS` in order (the preset commands generated from the bank).
pub fn commands() -> Vec<Command> {
    let v = vec![
        cmd(
            "new-project",
            "New Project",
            "Project",
            &icons_views::FILE_PLUS,
            Some("⌘N"),
            "create new",
            false,
        ),
        cmd(
            "open-project",
            "Open Project",
            "Project",
            &icons::FOLDER_OPEN,
            Some("⌘O"),
            "open load",
            false,
        ),
        cmd(
            "save",
            "Save",
            "Project",
            &icons_views::SAVE,
            Some("⌘S"),
            "",
            false,
        ),
        cmd(
            "save-as",
            "Save As",
            "Project",
            &icons_views::SAVE,
            Some("⌘⇧S"),
            "export save copy",
            false,
        ),
        cmd(
            "switch-daw",
            "Switch to DAW",
            "Navigation",
            &icons::MUSIC,
            Some("⌘1"),
            "mode arrange perform",
            false,
        ),
        cmd(
            "switch-samples",
            "Switch to Samples",
            "Navigation",
            &icons::AUDIO_LINES,
            Some("⌘2"),
            "mode samples audio library browse",
            false,
        ),
        cmd(
            "browse-packs",
            "Browse Packs",
            "Navigation",
            &icons::PACKAGE,
            Some("⌘3"),
            "packs kits store download 808 909 drum machine samples library get",
            false,
        ),
        cmd(
            "switch-dev",
            "Switch to Developer",
            "Navigation",
            &icons::TERMINAL,
            Some("⌘4"),
            "mode developer midi monitor debug raw",
            false,
        ),
        cmd(
            "toggle-view",
            "Switch Beat / Song View",
            "View",
            &icons_views::ARROW_LEFT_RIGHT,
            Some("Tab"),
            "beat song arrangement pads pattern toggle",
            true,
        ),
        cmd(
            "switch-beat",
            "Beat View",
            "View",
            &icons_views::LAYOUT_GRID,
            None,
            "pads pattern sequencer home",
            true,
        ),
        cmd(
            "switch-song",
            "Song View",
            "View",
            &icons_views::ROWS_3,
            None,
            "arrangement timeline tracks piano roll",
            true,
        ),
        cmd(
            "toggle-browser",
            "Toggle Sample Browser",
            "View",
            &icons::PANEL_LEFT,
            Some("B"),
            "files samples folder browse panel",
            true,
        ),
        cmd(
            "play-stop",
            "Play / Stop",
            "Transport",
            &icons::PLAY,
            Some("Space"),
            "playback start pause",
            true,
        ),
        cmd(
            "loop-record",
            "Record Loop",
            "Transport",
            &icons_views::CIRCLE_DOT,
            Some("R"),
            "rec pattern pads overdub beat count-in",
            true,
        ),
        cmd(
            "record",
            "Record to Track (Song)",
            "Transport",
            &icons_views::CIRCLE_DOT,
            None,
            "rec arm arrangement",
            true,
        ),
        cmd(
            "toggle-metronome",
            "Toggle Metronome",
            "Transport",
            &icons_views::TIMER,
            Some("M"),
            "click tempo beat",
            true,
        ),
        cmd(
            "toggle-note-repeat",
            "Toggle Note Repeat",
            "Transport",
            &icons::REPEAT,
            Some("N"),
            "nr retrigger roll pads",
            true,
        ),
        cmd(
            "clear-pattern",
            "Clear Pattern",
            "Editing",
            &icons::TRASH_2,
            Some("⇧⌫"),
            "delete erase beat steps",
            true,
        ),
        cmd(
            "controller-setup",
            "Controller Setup…",
            "MIDI",
            &icons::SLIDERS_HORIZONTAL,
            None,
            "midi mpk pads knobs map learn controller",
            false,
        ),
        cmd(
            "undo",
            "Undo",
            "Editing",
            &icons::UNDO_2,
            Some("⌘Z"),
            "",
            false,
        ),
        cmd(
            "redo",
            "Redo",
            "Editing",
            &icons::REDO_2,
            Some("⌘⇧Z"),
            "",
            false,
        ),
        cmd(
            "add-synth-track",
            "Add Synth Track",
            "Editing",
            &icons::PLUS,
            None,
            "new track instrument",
            true,
        ),
        cmd(
            "add-drum-track",
            "Add Drum Track",
            "Editing",
            &icons::PLUS,
            None,
            "new track percussion",
            true,
        ),
        cmd(
            "add-audio-track",
            "Add Audio Track",
            "Editing",
            &icons::FILE_AUDIO,
            None,
            "new track audio wav mp3",
            true,
        ),
        cmd(
            "import-audio",
            "Import Audio File",
            "Editing",
            &icons::FILE_AUDIO,
            Some("⌘I"),
            "import audio wav mp3 sample",
            true,
        ),
        cmd(
            "add-marker",
            "Add Marker",
            "Editing",
            &icons_song::FLAG,
            Some("⇧M"),
            "marker section flag",
            true,
        ),
        cmd(
            "next-marker",
            "Next Marker",
            "Editing",
            &icons_views::SKIP_FORWARD,
            Some("]"),
            "marker jump next",
            true,
        ),
        cmd(
            "prev-marker",
            "Previous Marker",
            "Editing",
            &icons_views::SKIP_BACK,
            Some("["),
            "marker jump previous",
            true,
        ),
        cmd(
            "create-group",
            "Group Tracks",
            "Editing",
            &icons::FOLDER_PLUS,
            Some("⌘G"),
            "group folder tracks",
            true,
        ),
        cmd(
            "new-from-template",
            "New from Template",
            "Project",
            &icons_views::LAYOUT_TEMPLATE,
            None,
            "template preset project",
            false,
        ),
        cmd(
            "export-wav",
            "Export as WAV",
            "Export",
            &icons::DOWNLOAD,
            Some("⌘E"),
            "audio bounce render",
            true,
        ),
        cmd(
            "export-midi",
            "Export as MIDI",
            "Export",
            &icons::DOWNLOAD,
            Some("⌘E"),
            "midi file",
            true,
        ),
        cmd(
            "toggle-fullscreen",
            "Toggle Fullscreen",
            "View",
            &icons::MAXIMIZE_2,
            None,
            "maximize screen",
            false,
        ),
        cmd(
            "open-settings",
            "Settings",
            "View",
            &icons::SETTINGS,
            Some("⌘,"),
            "preferences options config",
            false,
        ),
    ];
    // No "Instrument" preset commands: they picked the live sound, which left the UI (a
    // track's sound is picked in its header).
    v
}

/// `buildRecentProjectCommands`: the first 10 recent projects (`recent-<i>`), searched by
/// `label subtitle keywords`.
pub fn recent_commands(recent: &[mdaw_model::types::ProjectMeta]) -> Vec<Command> {
    recent
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, p)| Command {
            id: format!("recent-{i}"),
            label: p.name.clone(),
            group: "Recent Projects",
            icon: &icons_views::CLOCK,
            shortcut: None,
            keywords: format!("{} recent project {}", p.file_path, p.name.to_lowercase()),
            daw_only: false,
            subtitle: Some(p.file_path.clone()),
        })
        .collect()
}

// cmdk 1.1.1's `command-score` (the default filter), ported as written: the abbreviation's
// characters are found in order; continuing a match scores 1, after a word gap (`\\/_+.#"@[({&`)
// 0.8, after a space or `-` 0.9, after other characters 0.17 (skipped characters cost 0.999
// each), a case mismatch 0.9999, an unfinished string 0.99; transpositions 0.1.
const SCORE_CONTINUE_MATCH: f64 = 1.0;
const SCORE_SPACE_WORD_JUMP: f64 = 0.9;
const SCORE_NON_SPACE_WORD_JUMP: f64 = 0.8;
const SCORE_CHARACTER_JUMP: f64 = 0.17;
const SCORE_TRANSPOSITION: f64 = 0.1;
const PENALTY_SKIPPED: f64 = 0.999;
const PENALTY_CASE_MISMATCH: f64 = 0.9999;
const PENALTY_NOT_COMPLETE: f64 = 0.99;

fn is_gap(c: Option<char>) -> bool {
    c.is_some_and(|c| "\\/_+.#\"@[({&".contains(c))
}

fn is_space(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_whitespace() || c == '-')
}

struct Score<'a> {
    string: &'a [char],
    abbr: &'a [char],
    lower: &'a [char],
    lower_abbr: &'a [char],
    memo: HashMap<(usize, usize), f64>,
}

impl Score<'_> {
    fn at(v: &[char], i: isize) -> Option<char> {
        (i >= 0).then(|| v.get(i as usize).copied()).flatten()
    }

    /// `lowerString.indexOf(char, from)` (in characters).
    fn index_of(&self, c: char, from: usize) -> Option<usize> {
        (from..self.lower.len()).find(|&i| self.lower[i] == c)
    }

    fn inner(&mut self, si: usize, ai: usize) -> f64 {
        if ai == self.abbr.len() {
            return if si == self.string.len() {
                SCORE_CONTINUE_MATCH
            } else {
                PENALTY_NOT_COMPLETE
            };
        }
        if ai > self.abbr.len() {
            return 0.0;
        }
        if let Some(v) = self.memo.get(&(si, ai)) {
            return *v;
        }
        let ac = self.lower_abbr[ai];
        let mut index = self.index_of(ac, si);
        let mut high = 0.0f64;
        while let Some(i) = index {
            let mut score = self.inner(i + 1, ai + 1);
            if score > high {
                if i == si {
                    score *= SCORE_CONTINUE_MATCH;
                } else if is_gap(Self::at(self.string, i as isize - 1)) {
                    score *= SCORE_NON_SPACE_WORD_JUMP;
                    let breaks = self.string[si..(i.saturating_sub(1)).max(si)]
                        .iter()
                        .filter(|c| is_gap(Some(**c)))
                        .count();
                    if breaks > 0 && si > 0 {
                        score *= PENALTY_SKIPPED.powi(breaks as i32);
                    }
                } else if is_space(Self::at(self.string, i as isize - 1)) {
                    score *= SCORE_SPACE_WORD_JUMP;
                    let breaks = self.string[si..(i.saturating_sub(1)).max(si)]
                        .iter()
                        .filter(|c| is_space(Some(**c)))
                        .count();
                    if breaks > 0 && si > 0 {
                        score *= PENALTY_SKIPPED.powi(breaks as i32);
                    }
                } else {
                    score *= SCORE_CHARACTER_JUMP;
                    if si > 0 {
                        score *= PENALTY_SKIPPED.powi((i - si) as i32);
                    }
                }
                if self.string.get(i) != self.abbr.get(ai) {
                    score *= PENALTY_CASE_MISMATCH;
                }
            }
            let next_abbr = Self::at(self.lower_abbr, ai as isize + 1);
            let prev_str = Self::at(self.lower, i as isize - 1);
            if (score < SCORE_TRANSPOSITION && prev_str == next_abbr)
                || (next_abbr == Some(ac) && prev_str != Some(ac))
            {
                let transposed = self.inner(i + 1, ai + 2);
                if transposed * SCORE_TRANSPOSITION > score {
                    score = transposed * SCORE_TRANSPOSITION;
                }
            }
            if score > high {
                high = score;
            }
            index = self.index_of(ac, i + 1);
        }
        self.memo.insert((si, ai), high);
        high
    }
}

/// cmdk's `defaultFilter(value, search)` = `commandScore(value, search)`: lower-cased, spaces and
/// dashes as spaces.
pub fn command_score(value: &str, search: &str) -> f64 {
    let fold = |x: &str| -> Vec<char> {
        x.to_lowercase()
            .chars()
            .map(|c| {
                if c.is_whitespace() || c == '-' {
                    ' '
                } else {
                    c
                }
            })
            .collect()
    };
    let string: Vec<char> = value.chars().collect();
    let abbr: Vec<char> = search.chars().collect();
    let (lower, lower_abbr) = (fold(value), fold(search));
    if lower.len() != string.len() || lower_abbr.len() != abbr.len() {
        // Lower-casing changed the length (rare characters): compare the folded strings.
        let mut sc = Score {
            string: &lower,
            abbr: &lower_abbr,
            lower: &lower,
            lower_abbr: &lower_abbr,
            memo: HashMap::new(),
        };
        return sc.inner(0, 0);
    }
    let mut sc = Score {
        string: &string,
        abbr: &abbr,
        lower: &lower,
        lower_abbr: &lower_abbr,
        memo: HashMap::new(),
    };
    sc.inner(0, 0)
}

/// The value cmdk filters on: `` `${label} ${keywords ?? ''}` ``.
fn cmdk_value(c: &Command) -> String {
    format!("{} {}", c.label, c.keywords)
}

/// The commands available in `mode`, in `STATIC_COMMANDS` order, then the recent projects'
/// group (`CommandPalette` renders it last).
fn mode_commands(mode: AppMode, recent: &[mdaw_model::types::ProjectMeta]) -> Vec<Command> {
    commands()
        .into_iter()
        .chain(recent_commands(recent))
        .filter(|c| !c.daw_only || mode == AppMode::Daw)
        .collect()
}

/// cmdk's `sort()` after a search change: every item is re-appended to its group in score order
/// (a stable sort of the current DOM order); with an empty search nothing moves. cmdk 1.1.1 also
/// tries to re-append the groups by their best score, but it looks each group up by its id in
/// `data-value` (which holds the heading), finds none and moves nothing: the groups keep their
/// registration order (`p.group_order` is never changed). Updates `p.order`.
pub fn palette_sort(p: &mut crate::views::PaletteUi, mode: AppMode) {
    let q = p.query.text.clone();
    if q.is_empty() {
        return;
    }
    let cmds = mode_commands(mode, &p.recent);
    let score_of = |id: &str| -> f64 {
        cmds.iter()
            .find(|c| c.id == id)
            .filter(|c| !c.daw_only || mode == AppMode::Daw)
            .map_or(0.0, |c| command_score(&cmdk_value(c), &q))
    };
    let mut scored: Vec<(f64, String)> = p
        .order
        .iter()
        .map(|id| (score_of(id), id.clone()))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    p.order = scored.into_iter().map(|(_, id)| id).collect();
}

/// The rows as cmdk shows them for the current query: the groups in their DOM order, each with
/// its matching items in their DOM order (every item when the query is empty).
pub fn palette_rows(p: &crate::views::PaletteUi, mode: AppMode) -> Vec<Command> {
    let q = &p.query.text;
    let mut cmds: Vec<Option<Command>> = mode_commands(mode, &p.recent)
        .into_iter()
        .map(Some)
        .collect();
    let mut out = Vec::new();
    for g in &p.group_order {
        for id in &p.order {
            let Some(slot) = cmds
                .iter_mut()
                .find(|c| c.as_ref().is_some_and(|c| &c.id == id && c.group == *g))
            else {
                continue;
            };
            let c = slot.take().expect("present");
            if q.is_empty() || command_score(&cmdk_value(&c), q) > 0.0 {
                out.push(c);
            }
        }
    }
    out
}

/// The query changed since the last look: cmdk re-sorts and selects the first row again.
pub fn palette_sync(s: &mut App) -> bool {
    if s.p9.palette.query.text == s.p9.palette.seen_query {
        return false;
    }
    s.p9.palette.seen_query = s.p9.palette.query.text.clone();
    let mode = s.mode;
    palette_sort(&mut s.p9.palette, mode);
    s.p9.palette.sel = 0;
    true
}

/// The commands shown in `mode` for `query` on a freshly opened palette (tests).
pub fn palette_items(mode: AppMode, query: &str) -> Vec<Command> {
    palette_items_with(mode, query, &[])
}

/// [`palette_items`] with the recent projects' group (after the commands).
pub fn palette_items_with(
    mode: AppMode,
    query: &str,
    recent: &[mdaw_model::types::ProjectMeta],
) -> Vec<Command> {
    let mut p = crate::views::PaletteUi::with_recent(recent);
    p.query = rune::TextEdit::new(query);
    palette_sort(&mut p, mode);
    palette_rows(&p, mode)
}

impl App {
    /// Runs a palette command (`actions[id]()`), then closes the palette.
    pub fn run_command(&mut self, id: &str, tx: &mut Tx) {
        let _ = self.leave_dialog();
        match id {
            "switch-daw" => self.switch_mode(AppMode::Daw),
            "switch-samples" => self.switch_mode(AppMode::Samples),
            "browse-packs" => self.switch_mode(AppMode::Packs),
            "switch-dev" => self.switch_mode(AppMode::Dev),
            "toggle-view" => {
                self.switch_mode(AppMode::Daw);
                self.view = if self.view == DawView::Beat {
                    DawView::Song
                } else {
                    DawView::Beat
                };
            }
            "switch-beat" => {
                self.switch_mode(AppMode::Daw);
                self.view = DawView::Beat;
            }
            "switch-song" => {
                self.switch_mode(AppMode::Daw);
                self.view = DawView::Song;
            }
            "toggle-browser" => {
                self.switch_mode(AppMode::Daw);
                self.toggle_browser();
            }
            "play-stop" => self.palette_toggle_play(),
            "loop-record" => {
                self.view = DawView::Beat;
                self.toggle_record();
            }
            "record" => self.palette_song_record(),
            "toggle-metronome" => self.palette_toggle_metronome(),
            "toggle-note-repeat" => self.toggle_note_repeat(),
            "clear-pattern" => self.clear_pattern(),
            "controller-setup" => self.open_dialog(Dialog::Controller),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "add-synth-track" => {
                self.svc
                    .engine
                    .edit(|d| d.add_track(mdaw_model::types::InstrumentType::Synth, 0));
            }
            "add-drum-track" => {
                self.svc
                    .engine
                    .edit(|d| d.add_track(mdaw_model::types::InstrumentType::Drums, 0));
            }
            "add-audio-track" => {
                self.svc
                    .engine
                    .edit(|d| d.add_track(mdaw_model::types::InstrumentType::Audio, 0));
            }
            "import-audio" => self.import_audio_pick(tx),
            "add-marker" => self.add_marker_at_playhead(),
            "next-marker" => self.palette_marker(true),
            "prev-marker" => self.palette_marker(false),
            "create-group" => self.group_all_tracks(),
            "new-from-template" => self.open_dialog(Dialog::Templates),
            "export-wav" | "export-midi" => self.open_dialog(Dialog::Export),
            "toggle-fullscreen" => tx.toggle_full_screen(),
            "open-settings" => self.switch_mode(AppMode::Settings),
            "new-project" => self.request_project_op(ProjectOp::New, tx),
            "open-project" => self.request_project_op(ProjectOp::Open, tx),
            "save" => self.save_project(tx),
            "save-as" => self.save_project_as(tx),
            other => {
                if let Some(i) = other.strip_prefix("preset-").and_then(|n| n.parse().ok()) {
                    self.change_preset(i);
                }
                // Recent projects (`recent-<i>`: `onOpenRecent(subtitle)`).
                if let Some(i) = other
                    .strip_prefix("recent-")
                    .and_then(|n| n.parse::<usize>().ok())
                    && let Some(p) = self.project.recent.get(i).map(|p| p.file_path.clone())
                {
                    self.request_project_op(ProjectOp::OpenPath(p), tx);
                }
            }
        }
    }
}

/// The rows as drawn (kept for the tests: [`palette_rows`] already returns them grouped).
pub fn shown_order(items: Vec<Command>) -> Vec<Command> {
    items
}

/// `CommandPalette` (`CommandDialog` at 18 % from the top, `sm:max-w-xl`).
pub fn palette(s: &App, win: [f32; 2]) -> El {
    let th = t();
    let items = palette_rows(&s.p9.palette, s.mode);
    let sel = s.p9.palette.sel.min(items.len().saturating_sub(1));
    let mut list = col().w_full();
    let mut groups: Vec<&str> = Vec::new();
    for c in &items {
        if !groups.contains(&c.group) {
            groups.push(c.group);
        }
    }
    // cmdk groups in order of first appearance, each a `p-1` block.
    let mut idx = 0usize;
    for g in &groups {
        let mut group = col().w_full().padding(4.0).child(
            text((*g).to_owned())
                .font(crate::theme::daw_label(10.5))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg)
                .px(8.0)
                .pt(8.0)
                .pb(4.0),
        );
        for c in items.iter().filter(|c| c.group == *g) {
            let on = idx == sel;
            let id = c.id.clone();
            let mut r = row()
                .key(format!("cmd-{}", c.id))
                .role("option")
                .label(c.label.clone())
                .focus_selected(on)
                .w_full()
                .items_center()
                .gap(8.0)
                .px(8.0)
                .py(6.0)
                .radius(R_MD)
                .child(icon(c.icon, 16.0).color(if on { th.fg } else { th.muted_fg }))
                .child(match &c.subtitle {
                    None => text(c.label.clone()).font(ui(13.0, 400.0)).color(th.fg),
                    Some(sub) => col()
                        .min_w(0.0)
                        .child(text(c.label.clone()).font(ui(13.0, 400.0)).color(th.fg))
                        .child(
                            text(sub.clone())
                                .font(mono(11.0, 400.0))
                                .color(th.muted_fg)
                                .truncate()
                                .max_w(420.0),
                        ),
                })
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.run_command(&id, tx));
            if let Some(k) = c.shortcut {
                r = r.child(spacer()).child(
                    text(k)
                        .font(mono(11.0, 400.0).tracking(0.5))
                        .color(th.muted_fg),
                );
            }
            if on {
                r = r.fill(th.surface3);
            } else {
                r = r.hover(th.surface3);
            }
            group = group.child(r);
            idx += 1;
        }
        list = list.child(group);
    }
    if items.is_empty() {
        list = list.child(
            text("No results found.")
                .key("palette-empty")
                .font(ui(14.0, 400.0))
                .color(th.muted_fg)
                .py(24.0)
                .self_center(),
        );
    }
    let hint = |k: Vec<&str>, l: &str| {
        let mut r = row().items_center().gap(6.0);
        for x in k {
            r = r.child(kbd(x));
        }
        r.child(text(l.to_owned()).font(ui(11.0, 400.0)).color(th.muted_fg))
    };
    let w = 576.0f32.min(win[0] - 32.0);
    let panel = col()
        .w(w)
        .radius(12.0)
        .border(1.0, th.border)
        .fill(th.popover)
        .clip_rounded(12.0)
        .box_shadow(rune::Shadow {
            x: 0.0,
            y: 24.0,
            blur: 64.0,
            spread: -12.0,
            color: Color::BLACK.alpha(0.7),
            inset: false,
        })
        .child(
            row()
                .w_full()
                .h(48.0)
                .items_center()
                .px(14.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(icon(&icons::SEARCH, 16.0).color(th.muted_fg).mr(10.0))
                .child(
                    text_input(&s.p9.palette.query, ui(14.0, 400.0))
                        .key("palette-input")
                        .label("Search commands, sounds, projects…")
                        .single_line()
                        .autofocus()
                        .placeholder("Search commands, sounds, projects…", th.muted_fg)
                        .color(th.fg)
                        .grow(1.0)
                        // The selection resets when the query text changes (`palette_sync`),
                        // not on every key the field sees.
                        .on_edit(|s: &mut App| &mut s.p9.palette.query)
                        .on_input_key(|s: &mut App, ev: &rune::KeyEvent, tx: &mut Tx| {
                            palette_key(s, ev, tx)
                        }),
                ),
        )
        .child(
            col()
                .key("palette-list")
                .role("listbox")
                .w_full()
                .max_h(300.0)
                .child(list)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        )
        .child(
            row()
                .w_full()
                .h(36.0)
                .items_center()
                .gap(16.0)
                .px(14.0)
                .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
                .child(hint(vec!["↑", "↓"], "navigate"))
                .child(hint(vec!["↵"], "run"))
                .child(hint(vec!["esc"], "close"))
                .child(spacer())
                .child(hint(vec!["⌘K"], "toggle")),
        );
    dialog_layer(
        "palette",
        "Command Palette",
        panel,
        Some(win[1] * 0.18),
        |s: &mut App| s.close_dialog(),
    )
}

/// Up / Down move, Return runs, Escape closes.
pub fn palette_key(s: &mut App, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    let _ = palette_sync(s);
    let shown = palette_rows(&s.p9.palette, s.mode);
    let n = shown.len();
    let reveal = |s: &App, tx: &mut Tx| {
        // cmdk's `scrollSelectedIntoView` (block: nearest).
        if let Some(c) = shown.get(s.p9.palette.sel.min(n.saturating_sub(1))) {
            tx.scroll_into_view_in("palette-list", format!("cmd-{}", c.id));
        }
    };
    match ev.key {
        rune::Key::Down => {
            if n > 0 {
                s.p9.palette.sel = (s.p9.palette.sel + 1).min(n - 1);
            }
            reveal(s, tx);
            true
        }
        rune::Key::Up => {
            s.p9.palette.sel = s.p9.palette.sel.saturating_sub(1);
            reveal(s, tx);
            true
        }
        rune::Key::Enter => {
            if let Some(c) = shown.get(s.p9.palette.sel.min(n.saturating_sub(1))) {
                let id = c.id.clone();
                s.run_command(&id, tx);
            }
            true
        }
        rune::Key::Escape => {
            s.close_dialog();
            true
        }
        _ => false,
    }
}

// ------------------------------------------------------------------------------- export

/// `ExportDialog`.
pub fn export(s: &App) -> El {
    let th = t();
    let tracks = s.svc.engine.doc().get_tracks();
    let n = tracks.len();
    let audible = tracks.iter().filter(|t| !t.muted).count();
    let running = s.p9.export.running;
    let sub = if n == 0 {
        "Nothing to export yet. Add a track first.".to_owned()
    } else {
        format!(
            "{audible} of {n} track{} will be included (muted tracks are skipped).",
            if n == 1 { "" } else { "s" }
        )
    };
    let option = |kind: ExportKind, ic: &'static Icon, title: &str, sub: &str| {
        let on = running == Some(kind);
        let disabled = running.is_some() || n == 0;
        let key = match kind {
            ExportKind::Wav => "export-wav",
            ExportKind::Midi => "export-midi",
        };
        let mut b = row()
            .key(key)
            .role("button")
            .label(title.to_owned())
            .w_full()
            .h(56.0)
            .items_center()
            .gap(12.0)
            .px(12.0)
            .radius(R_LG)
            .border(1.0, th.border)
            .fill(th.surface2)
            .child(
                row()
                    .size(32.0, 32.0)
                    .center()
                    .radius(R_MD)
                    .fill(th.primary.alpha(0.12))
                    .child(
                        icon(if on { &icons::LOADER_CIRCLE } else { ic }, 16.0).color(th.primary),
                    ),
            )
            .child(
                col()
                    .grow(1.0)
                    .child(
                        text(if on {
                            "Exporting…".to_owned()
                        } else {
                            title.to_owned()
                        })
                        .font(ui(13.0, 600.0))
                        .color(th.fg),
                    )
                    .child(
                        text(sub.to_owned())
                            .font(ui(11.0, 400.0))
                            .color(th.muted_fg),
                    ),
            )
            .child(icon(&icons::CHEVRON_RIGHT, 16.0).color(th.muted_fg));
        if disabled {
            b = b.opacity(0.45).disabled(true);
        } else {
            b = b
                .hover(th.surface3)
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.export_pick(kind, tx));
        }
        b
    };
    let mut panel = dialog_panel(360.0, 20.0)
        .child(
            text("Export")
                .font(ui(15.0, 600.0).tracking(-0.4))
                .color(th.fg),
        )
        .child(
            text(sub)
                .font(ui(12.0, 400.0))
                .color(th.muted_fg)
                .wrap()
                .mt(4.0)
                .mb(16.0),
        )
        .child(
            col()
                .w_full()
                .gap(8.0)
                .child(option(
                    ExportKind::Wav,
                    &icons::FILE_AUDIO,
                    "WAV audio",
                    "16-bit PCM stereo, rendered offline",
                ))
                .child(option(
                    ExportKind::Midi,
                    &icons::MUSIC,
                    "MIDI file",
                    "Standard MIDI; drums on channel 10",
                )),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::X)
                .a11y("Close")
                .disabled(running.is_some())
                .key("export-close")
                .build(|s: &mut App, _| s.close_dialog())
                .absolute(360.0 - 12.0 - 28.0, 12.0),
        );
    if let Some(e) = &s.p9.export.error {
        panel = panel.child(
            text(e.clone())
                .role("alert")
                .font(ui(12.0, 400.0))
                .color(th.rec)
                .wrap()
                .mt(12.0)
                .px(10.0)
                .py(6.0)
                .radius(R_MD)
                .border(1.0, th.rec.alpha(0.4))
                .fill(th.rec.alpha(0.1)),
        );
    }
    dialog_layer("export", "Export", panel, None, |s: &mut App| {
        s.close_dialog()
    })
}

// ------------------------------------------------------------------------------- templates

const CATEGORIES: [(&str, &str, &str); 6] = [
    ("starter", "Starter", "oklch(0.65 0.16 195)"),
    ("rock", "Rock", "oklch(0.62 0.22 25)"),
    ("hip-hop", "Hip-Hop", "oklch(0.62 0.22 310)"),
    ("jazz", "Jazz", "oklch(0.65 0.20 250)"),
    ("edm", "EDM", "oklch(0.75 0.17 90)"),
    ("ambient", "Ambient", "oklch(0.65 0.20 155)"),
];

/// `TemplatePicker`.
pub fn template_picker(s: &App, win: [f32; 2]) -> El {
    let th = t();
    let w = 720.0f32.min(win[0] - 32.0);
    let filter = s.p9.template_filter;
    let mut tabs = row()
        .role("tablist")
        .w_full()
        .gap(4.0)
        .px(20.0)
        .py(8.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border);
    let tab = |key: String, label: &str, on: bool, v: Option<&'static str>| {
        let mut b = row()
            .key(key)
            .role("tab")
            .label(label.to_owned())
            .focus_selected(on)
            .h(28.0)
            .px(10.0)
            .items_center()
            .radius(R_MD)
            .child(text(label.to_owned()).font(ui(12.0, 600.0)).color(if on {
                th.fg
            } else {
                th.muted_fg
            }))
            .on_press(move |s: &mut App| s.p9.template_filter = v);
        b = if on {
            b.fill(th.surface3)
        } else {
            b.hover(th.surface2)
        };
        b
    };
    tabs = tabs.child(tab(
        "template-tab-all".into(),
        "All",
        filter.is_none(),
        None,
    ));
    for (id, label, _) in CATEGORIES {
        tabs = tabs.child(tab(
            format!("template-tab-{id}"),
            label,
            filter == Some(id),
            Some(id),
        ));
    }
    let shown: Vec<(usize, &crate::views::Template)> = templates()
        .iter()
        .enumerate()
        .filter(|(_, t)| filter.is_none_or(|f| t.category == f))
        .collect();
    let card_w = (w - 40.0 - 12.0) / 2.0;
    let mut grid = col().w_full().gap(12.0).padding(20.0);
    for pair in shown.chunks(2) {
        let mut r = row().w_full().gap(12.0);
        for (i, tp) in pair {
            let (_, clabel, ccol) = CATEGORIES
                .iter()
                .find(|c| c.0 == tp.category)
                .copied()
                .unwrap_or(("", "", "oklch(0.65 0.16 195)"));
            let cc = css_color(ccol, th.primary);
            let mut bars = col().w_full().gap(3.0).mt(12.0);
            for (k, c) in tp.colors.iter().enumerate() {
                bars = bars.child(
                    el().h(5.0)
                        .w_frac((55 + (k * 37) % 45) as f32 / 100.0)
                        .radius(2.5)
                        .fill(css_color(c, th.primary).alpha(0.85)),
                );
            }
            let i = *i;
            let card = col()
                .key(format!("template-{}", tp.id))
                .role("button")
                .label(tp.name.clone())
                .w(card_w)
                .padding(16.0)
                .radius(R_LG)
                .border(1.0, th.border)
                .fill(th.surface2)
                .hover(th.surface3)
                .child(
                    row()
                        .w_full()
                        .items_start()
                        .justify_between()
                        .gap(8.0)
                        .child(text(tp.name.clone()).font(ui(14.0, 600.0)).color(th.fg))
                        .child(
                            row()
                                .px(6.0)
                                .py(2.0)
                                .radius(R_SM - 2.0)
                                .fill(crate::ui::song::cmix(cc, Color::TRANSPARENT, 0.18))
                                .child(
                                    text(clabel)
                                        .font(ui(10.0, 700.0).tracking(0.4))
                                        .color(crate::ui::song::cmix(cc, th.white, 0.85)),
                                ),
                        ),
                )
                .child(
                    text(tp.description.clone())
                        .font(ui(12.0, 400.0).lh(19.5))
                        .color(th.muted_fg)
                        .wrap()
                        .mt(4.0),
                )
                .child(bars)
                .child(
                    row()
                        .mt(12.0)
                        .items_center()
                        .gap(12.0)
                        .child(
                            text(format!("{} BPM", mdaw_model::js::number_to_string(tp.bpm)))
                                .font(mono(11.0, 400.0))
                                .color(th.muted_fg),
                        )
                        .child(el().size(4.0, 4.0).radius(2.0).fill(th.border))
                        .child(
                            text(format!("{} tracks", tp.track_count))
                                .font(mono(11.0, 400.0))
                                .color(th.muted_fg),
                        ),
                )
                .on_press_tx(move |s: &mut App, tx: &mut Tx| s.new_from_template(i, tx));
            r = r.child(card);
        }
        grid = grid.child(r);
    }
    let panel = dialog_panel(w, 0.0)
        .max_h(win[1] * 0.8)
        .clip_rounded(12.0)
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .px(20.0)
                .py(14.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
                .child(
                    col()
                        .child(
                            row()
                                .items_center()
                                .gap(8.0)
                                .child(icon(&icons_views::LAYOUT_TEMPLATE, 16.0).color(th.primary))
                                .child(
                                    text("New from template")
                                        .font(ui(15.0, 600.0).tracking(-0.4))
                                        .color(th.fg),
                                ),
                        )
                        .child(
                            text("Start with tracks, sounds and a groove already set up.")
                                .font(ui(12.0, 400.0))
                                .color(th.muted_fg)
                                .mt(2.0),
                        ),
                )
                .child(
                    Button::new(Variant::Ghost, Size::IconSm)
                        .icon(&icons::X)
                        .a11y("Close")
                        .key("templates-close")
                        .build(|s: &mut App, _| s.close_dialog()),
                ),
        )
        .child(tabs)
        .child(
            col()
                .w_full()
                // `max-h-[80vh]` less the header (70) and the tabs (45).
                .max_h(win[1] * 0.8 - 115.0)
                .child(grid)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    dialog_layer(
        "templates",
        "New from template",
        panel,
        None,
        |s: &mut App| s.close_dialog(),
    )
}

// ------------------------------------------------------------------------------- controller

fn describe(a: &MidiActivity) -> (String, String) {
    match a {
        MidiActivity::Note {
            note,
            velocity,
            channel,
            pad,
            ..
        } => (
            format!("Note {} · {note}", midi_to_note_name(*note as f64)),
            format!(
                "ch {channel} · vel {} → {}",
                (velocity * 127.0).round(),
                pad.map_or("keys".to_owned(), |p| format!("Pad {}", p + 1))
            ),
        ),
        MidiActivity::Cc {
            cc,
            value,
            channel,
            targets,
            ..
        } => (
            format!("CC {cc} = {}", mdaw_model::js::number_to_string(*value)),
            format!(
                "ch {channel} → {}",
                if targets.is_empty() {
                    "unmapped".to_owned()
                } else {
                    targets
                        .iter()
                        .map(knob_target_label)
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            ),
        ),
    }
}

/// The pad channel list.
pub fn pad_channel_menu(s: &App) -> El {
    let ch = s
        .svc
        .with_router(|r, _| r.mapping.get_profile().pad_map.channel);
    let mut list = popover_frame(Some(128.0), 4.0);
    list = list.child(
        select_option(
            "pad-ch-any".into(),
            row().child(text("Any channel").font(ui(12.0, 400.0)).color(t().fg)),
            ch.is_none(),
        )
        .on_press(|s: &mut App| {
            s.svc.with_router(|r, _| r.mapping.set_pad_channel(None));
            s.menu = None;
        }),
    );
    for c in 1..=16i64 {
        list = list.child(
            select_option(
                format!("pad-ch-{c}"),
                row().child(
                    text(format!("Channel {c}"))
                        .font(ui(12.0, 400.0))
                        .color(t().fg),
                ),
                ch == Some(c),
            )
            .on_press(move |s: &mut App| {
                s.svc.with_router(|r, _| r.mapping.set_pad_channel(Some(c)));
                s.menu = None;
            }),
        );
    }
    select_popup(
        "pad-channel",
        Align::End,
        list.max_h(300.0).scroll(rune::ScrollSpec {
            stick_bottom: false,
        }),
    )
}

/// A knob row's target list.
pub fn knob_target_menu(s: &App, index: usize) -> El {
    let cur = s.svc.with_router(|r, _| {
        r.mapping
            .get_profile()
            .cc_mappings
            .get(index)
            .map(|m| m.target)
    });
    let mut list = popover_frame(Some(170.0), 4.0);
    for o in KNOB_TARGET_OPTIONS {
        let target = o.target;
        list = list.child(
            select_option(
                format!("knob-target-{index}-{}", o.key),
                row().child(text(o.label).font(ui(12.0, 400.0)).color(t().fg)),
                cur.is_some_and(|c| same_knob_target(&c, &target)),
            )
            .on_press(move |s: &mut App| {
                s.svc.with_router(|r, _| {
                    r.mapping.set_cc_mapping(
                        index as i64,
                        CCMappingPatch {
                            target: Some(target),
                            ..Default::default()
                        },
                    )
                });
                s.menu = None;
            }),
        );
    }
    select_popup(&format!("knob-target-{index}"), Align::Start, list)
}

fn pad_bank(
    s: &App,
    profile: &ControllerProfile,
    learn: &LearnState,
    activity: Option<&MidiActivity>,
    bank: usize,
    w: f32,
) -> El {
    let th = t();
    let base = bank * PADS_PER_BANK;
    let half = PADS_PER_BANK / 2;
    let rows = [
        (0..half).map(|i| base + half + i).collect::<Vec<_>>(),
        (0..half).map(|i| base + i).collect::<Vec<_>>(),
    ];
    let cell_w = (w - 16.0 - 18.0) / 4.0;
    let mut b = col()
        .w(w)
        .gap(6.0)
        .padding(8.0)
        .radius(R_LG)
        .border(1.0, th.border)
        .fill(th.surface0)
        .child(
            text(format!("Bank {}", if bank == 0 { "A" } else { "B" }))
                .font(ui(11.0, 600.0).tracking(0.3))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        );
    for rw in rows {
        let mut r = row().gap(6.0);
        for pad in rw {
            let note = profile.pad_map.notes.get(pad).copied().flatten();
            let learning = matches!(learn, LearnState::Pad { pad: p, .. } if *p == pad);
            let hit =
                matches!(activity, Some(MidiActivity::Note { pad: Some(p), .. }) if *p == pad);
            let value: El = if learning {
                text("hit…").font(mono(11.0, 400.0)).color(th.fg)
            } else if let Some(n) = note {
                row()
                    .items_end()
                    .gap(6.0)
                    .child(
                        text(midi_to_note_name(n as f64))
                            .font(mono(11.0, 400.0))
                            .color(th.fg),
                    )
                    .child(
                        text(n.to_string())
                            .font(mono(11.0, 400.0))
                            .color(th.muted_fg),
                    )
            } else {
                text("—").font(mono(11.0, 400.0)).color(th.muted_fg)
            };
            let mut c = col()
                .key(format!("ctl-pad-{pad}"))
                .role("button")
                .label(format!(
                    "Pad {}: {}. Click to learn.",
                    pad + 1,
                    note.map_or("unassigned".to_owned(), |n| format!("note {n}"))
                ))
                .checked(learning)
                .w(cell_w)
                .h(56.0)
                .justify_between()
                .px(8.0)
                .py(6.0)
                .radius(R_MD)
                .border(1.0, if learning { th.primary } else { th.border })
                .fill(if hit {
                    th.primary.alpha(0.35)
                } else if learning {
                    th.primary.alpha(0.15)
                } else {
                    th.surface2
                })
                .hover(th.surface3)
                .child(
                    text(pad_label(pad))
                        .font(mono(11.0, 600.0))
                        .color(th.muted_fg),
                )
                .child(value)
                .on_press(move |s: &mut App| s.controller_learn_pad(pad))
                .on_hover_change(move |s: &mut App, on: bool, _tx: &mut Tx| {
                    if on {
                        s.p9.ctl_hover = Some(pad);
                    } else if s.p9.ctl_hover == Some(pad) {
                        s.p9.ctl_hover = None;
                    }
                });
            // `hidden group-hover:flex`: the clear button shows while the pointer is on the pad.
            if note.is_some() && !learning && s.p9.ctl_hover == Some(pad) {
                c = c.child(
                    row()
                        .key(format!("ctl-pad-clear-{pad}"))
                        .role("button")
                        .label(format!("Clear pad {} mapping", pad + 1))
                        .help("Clear mapping")
                        .absolute(cell_w - 20.0, 4.0)
                        .size(16.0, 16.0)
                        .center()
                        .radius(R_SM - 2.0)
                        .hover(th.surface4)
                        .child(icon(&icons::X, 12.0).color(th.muted_fg))
                        .on_press(move |s: &mut App| {
                            s.svc.with_router(|r, _| r.mapping.clear_pad(pad as i64))
                        }),
                );
            }
            r = r.child(c);
        }
        b = b.child(r);
    }
    b
}

/// The CC column (the table's auto layout gives it about 70 points).
const CC_W: f32 = 70.0;

/// `ControllerSetupDialog`.
pub fn controller(s: &App, win: [f32; 2]) -> El {
    let th = t();
    let (profile, learn, activity) = s.svc.with_router(|r, _| {
        (
            r.mapping.get_profile().clone(),
            r.mapping.get_learn_state(),
            r.mapping.get_last_activity().cloned(),
        )
    });
    let host = &s.svc.midi.host;
    let device = host
        .active_device_id()
        .and_then(|id| host.devices().iter().find(|d| d.id == id))
        .map(|d| d.name.clone());
    let w = 640.0f32.min(win[0] - 32.0);
    let mut desc = row().flex_wrap().child(
        text("Map your controller's pads and knobs. ")
            .font(ui(13.0, 400.0))
            .color(th.muted_fg),
    );
    desc = match &device {
        Some(n) => desc
            .child(
                text("Listening to ")
                    .font(ui(13.0, 400.0))
                    .color(th.muted_fg),
            )
            .child(text(n.clone()).font(ui(13.0, 400.0)).color(th.fg))
            .child(text(".").font(ui(13.0, 400.0)).color(th.muted_fg)),
        None => desc.child(
            text("No MIDI input selected.")
                .font(ui(13.0, 400.0))
                .color(th.muted_fg),
        ),
    };
    let act = {
        let mut r = row()
            .key("midi-activity")
            .w_full()
            .items_center()
            .gap(10.0)
            .px(12.0)
            .py(8.0)
            .radius(R_LG)
            .border(1.0, th.border)
            .fill(th.surface0)
            .child(
                el().size(10.0, 10.0)
                    .radius(5.0)
                    .fill(if activity.is_some() {
                        th.primary
                    } else {
                        th.surface4
                    }),
            );
        r = match &activity {
            Some(a) => {
                let (p, sec) = describe(a);
                r.child(text(p).font(mono(12.0, 600.0)).color(th.fg)).child(
                    text(sec)
                        .font(ui(12.0, 400.0))
                        .color(th.muted_fg)
                        .truncate(),
                )
            }
            None => r.child(
                text("Waiting for MIDI… hit a pad or turn a knob.")
                    .font(ui(12.0, 400.0))
                    .color(th.muted_fg),
            ),
        };
        r
    };
    let mut body = col().w_full().gap(16.0).child(act);
    if learn != LearnState::Idle {
        let (title, hint, seq, pad) = match learn {
            LearnState::Pad {
                pad,
                sequence: true,
            } => (
                format!("Hit pad {}", pad + 1),
                format!(
                    "{} of 16 · bank {}{}",
                    pad + 1,
                    if pad < PADS_PER_BANK { "A" } else { "B" },
                    if pad == PADS_PER_BANK {
                        " (switch your controller to bank B)"
                    } else {
                        ""
                    }
                ),
                true,
                pad,
            ),
            LearnState::Pad { pad, .. } => (
                format!("Hit the hardware pad for Pad {}", pad + 1),
                "The next note you play is assigned to this pad.".to_owned(),
                false,
                pad,
            ),
            LearnState::Knob { target, .. } => (
                format!("Turn a knob for {}", knob_target_label(&target)),
                "The next knob (CC) you move is bound to this target.".to_owned(),
                false,
                0,
            ),
            LearnState::Idle => unreachable!(),
        };
        let mut banner = row()
            .key("learn-banner")
            .role("status")
            .w_full()
            .items_center()
            .gap(12.0)
            .px(12.0)
            .py(10.0)
            .radius(R_LG)
            .border(1.0, th.primary.alpha(0.4))
            .fill(th.primary.alpha(0.1))
            .child(el().size(8.0, 8.0).radius(4.0).fill(th.primary))
            .child(
                col()
                    .min_w(0.0)
                    .grow(1.0)
                    .child(
                        text(title)
                            .font(ui(14.0, 600.0).tracking(-0.3))
                            .color(th.fg),
                    )
                    .child(text(hint).font(ui(11.0, 400.0)).color(th.muted_fg)),
            );
        if seq {
            banner = banner
                .child(
                    Button::new(Variant::Ghost, Size::Xs)
                        .icon(&icons_views::DELETE)
                        .label("Back")
                        .tip("Previous pad (Backspace)", None)
                        .disabled(pad == 0)
                        .key("learn-back")
                        .build(|s: &mut App, _| s.svc.with_router(|r, _| r.mapping.previous_pad())),
                )
                .child(
                    Button::new(Variant::Ghost, Size::Xs)
                        .icon(&icons_views::SKIP_FORWARD)
                        .label("Skip")
                        .tip("Leave this pad as it is", None)
                        .key("learn-skip")
                        .build(|s: &mut App, _| s.svc.with_router(|r, _| r.mapping.skip_pad())),
                );
        }
        banner = banner.child(
            row()
                .key("learn-cancel")
                .role("button")
                .label("Cancel")
                .help("Cancel (Esc)")
                .h(24.0)
                .px(8.0)
                .items_center()
                .gap(4.0)
                .radius(R_SM)
                .border(1.0, th.border)
                .fill(th.surface2)
                .hover(th.surface3)
                .child(text("Cancel").font(ui(11.0, 500.0)).color(th.fg))
                .child(kbd("Esc"))
                .on_press(|s: &mut App| s.svc.with_router(|r, _| r.mapping.cancel_learn())),
        );
        body = body.child(banner);
    }
    let ch_label = profile
        .pad_map
        .channel
        .map_or("Any channel".to_owned(), |c| format!("Channel {c}"));
    let seq_on = matches!(learn, LearnState::Pad { sequence: true, .. });
    let bank_w = (w - 40.0 - 12.0) / 2.0;
    let pads = col()
        .key("ctl-pads")
        .label("Pads")
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    text("Pads")
                        .font(ui(12.0, 600.0).tracking(0.3))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    row()
                        .items_center()
                        .gap(8.0)
                        .child(
                            row()
                                .key("pad-channel")
                                .role("combobox")
                                .label("Pad MIDI channel")
                                .focusable(true)
                                .h(28.0)
                                .w(128.0)
                                .px(10.0)
                                .items_center()
                                .justify_between()
                                .radius(R_MD)
                                .border(1.0, th.border)
                                .fill(th.surface2)
                                .hover(th.surface3)
                                .child(text(ch_label).font(ui(12.0, 400.0)).color(th.fg))
                                .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
                                .on_press(|s: &mut App| s.toggle_p9_menu(P9Menu::PadChannel)),
                        )
                        .child(
                            Button::new(
                                if seq_on {
                                    Variant::Secondary
                                } else {
                                    Variant::Primary
                                },
                                Size::Sm,
                            )
                            .icon(&icons_views::WAND_SPARKLES)
                            .label("Learn pads (hit 1–16)")
                            .key("learn-pads")
                            .build(|s: &mut App, _| s.controller_learn_pads()),
                        ),
                ),
        )
        .child(
            row()
                .w_full()
                .gap(12.0)
                .child(pad_bank(s, &profile, &learn, activity.as_ref(), 0, bank_w))
                .child(pad_bank(s, &profile, &learn, activity.as_ref(), 1, bank_w)),
        )
        .child(
            text("Click a pad to learn just that one. A note already used by another pad moves to the new pad.")
                .font(ui(11.0, 400.0))
                .color(th.muted_fg)
                .wrap(),
        );
    // Knobs
    let mappings = profile.cc_mappings.clone();
    let learning_index = match learn {
        LearnState::Knob { index, .. } => index,
        _ => None,
    };
    let mut table = col().w_full().child(
        row()
            .w_full()
            .h(28.0)
            .items_center()
            .px(12.0)
            .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
            .child(
                text("CC")
                    .font(ui(11.0, 600.0).tracking(0.3))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg)
                    .w(CC_W),
            )
            .child(
                text("Target")
                    .font(ui(11.0, 600.0).tracking(0.3))
                    .text_case(rune::TextCase::Uppercase)
                    .color(th.muted_fg),
            ),
    );
    for (i, m) in mappings.iter().enumerate() {
        let row_learning = learning_index == Some(i as i64);
        let live = matches!(&activity, Some(MidiActivity::Cc { cc, .. }) if *cc == m.cc);
        let mut cc_cell = row().w(CC_W).items_center().gap(8.0);
        cc_cell = if row_learning {
            cc_cell.child(text("turn…").font(mono(12.0, 400.0)).color(th.primary))
        } else {
            cc_cell.child(text(m.cc.to_string()).font(mono(12.0, 400.0)).color(th.fg))
        };
        if let Some(c) = m.channel {
            cc_cell = cc_cell.child(
                text(format!("ch {c}"))
                    .font(mono(12.0, 400.0))
                    .color(th.muted_fg),
            );
        }
        if live
            && !row_learning
            && let Some(MidiActivity::Cc { value, .. }) = &activity
        {
            cc_cell = cc_cell.child(
                row()
                    .h(4.0)
                    .w(40.0)
                    .radius(2.0)
                    .fill(th.surface3)
                    .child(el().h(4.0).w_frac((*value / 127.0) as f32).fill(th.primary)),
            );
        }
        let target = m.target;
        table = table.child(
            row()
                .w_full()
                .h(37.0)
                .items_center()
                .px(12.0)
                .border_sides([0.0, 0.0, 1.0, 0.0], th.border.alpha(0.6))
                .child(cc_cell)
                .child(
                    row()
                        .key(format!("knob-target-{i}"))
                        .role("combobox")
                        .label(format!("Target for CC {}", m.cc))
                        .focusable(true)
                        .h(28.0)
                        .w(170.0)
                        .px(10.0)
                        .items_center()
                        .justify_between()
                        .radius(R_MD)
                        .border(1.0, th.border)
                        .fill(th.surface2)
                        .hover(th.surface3)
                        .child(
                            text(knob_target_label(&m.target))
                                .font(ui(12.0, 400.0))
                                .color(th.fg),
                        )
                        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(th.muted_fg.alpha(0.5)))
                        .on_press(move |s: &mut App| s.toggle_p9_menu(P9Menu::KnobTarget(i))),
                )
                .child(spacer())
                .child(
                    Button::new(
                        if row_learning {
                            Variant::Secondary
                        } else {
                            Variant::Ghost
                        },
                        Size::Xs,
                    )
                    .icon(&icons_views::CORNER_DOWN_LEFT)
                    .label(if row_learning { "Cancel" } else { "Learn" })
                    .tip("Move a knob to bind it to this target", None)
                    .key(format!("knob-learn-{i}"))
                    .build(move |s: &mut App, _| {
                        if row_learning {
                            s.svc.with_router(|r, _| r.mapping.cancel_learn());
                        } else {
                            s.controller_learn_knob(target, Some(i as i64));
                        }
                    }),
                )
                .child(
                    Button::new(Variant::Ghost, Size::IconXs)
                        .icon(&icons::TRASH_2)
                        .a11y(format!("Remove CC {} mapping", m.cc))
                        .key(format!("knob-remove-{i}"))
                        .build(move |s: &mut App, _| {
                            s.svc
                                .with_router(|r, _| r.mapping.remove_cc_mapping(i as i64))
                        }),
                ),
        );
    }
    if let LearnState::Knob {
        target,
        index: Some(ix),
    } = learn
        && ix as usize >= mappings.len()
    {
        table = table.child(
            row()
                .w_full()
                .px(12.0)
                .py(8.0)
                .child(
                    text("turn…")
                        .font(mono(12.0, 400.0))
                        .color(th.primary)
                        .w(CC_W),
                )
                .child(
                    text(knob_target_label(&target))
                        .font(ui(12.0, 400.0))
                        .color(th.fg),
                ),
        );
    }
    if mappings.is_empty() && !matches!(learn, LearnState::Knob { .. }) {
        table = table.child(
            text("No knobs mapped.")
                .font(ui(12.0, 400.0))
                .color(th.muted_fg)
                .self_center()
                .py(12.0),
        );
    }
    let n_map = mappings.len();
    let knobs = col()
        .key("ctl-knobs")
        .label("Knobs")
        .w_full()
        .gap(8.0)
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    text("Knobs")
                        .font(ui(12.0, 600.0).tracking(0.3))
                        .text_case(rune::TextCase::Uppercase)
                        .color(th.muted_fg),
                )
                .child(
                    Button::new(Variant::Outline, Size::Sm)
                        .icon(&icons::PLUS)
                        .label("Add knob")
                        .key("add-knob")
                        .build(move |s: &mut App, _| {
                            let used: Vec<KnobTarget> = s.svc.with_router(|r, _| {
                                r.mapping
                                    .get_profile()
                                    .cc_mappings
                                    .iter()
                                    .map(|m| m.target)
                                    .collect()
                            });
                            let t = KNOB_TARGET_OPTIONS
                                .iter()
                                .find(|o| !used.iter().any(|u| same_knob_target(u, &o.target)))
                                .unwrap_or(&KNOB_TARGET_OPTIONS[0])
                                .target;
                            s.controller_learn_knob(t, Some(n_map as i64));
                        }),
                ),
        )
        .child(
            col()
                .w_full()
                .max_h(220.0)
                .radius(R_LG)
                .border(1.0, th.border)
                .fill(th.surface0)
                .clip_rounded(R_LG)
                .child(table)
                .scroll(rune::ScrollSpec {
                    stick_bottom: false,
                }),
        );
    body = body.child(pads).child(knobs);
    let panel = dialog_panel(w, 20.0)
        .gap(16.0)
        .child(
            col()
                .w_full()
                .gap(8.0)
                .pr(32.0)
                .child(
                    text("Controller setup")
                        .font(ui(15.0, 600.0).lh(15.0).tracking(-0.4))
                        .color(th.fg),
                )
                .child(desc),
        )
        .child(body)
        .child(
            row()
                .w_full()
                .items_center()
                .justify_between()
                .child(
                    Button::new(Variant::Ghost, Size::Sm)
                        .icon(&icons_views::ROTATE_CCW)
                        .label("Reset to default")
                        .key("ctl-reset")
                        .build(|s: &mut App, _| {
                            s.svc.with_router(|r, _| r.mapping.reset_to_default())
                        }),
                )
                .child(
                    Button::new(Variant::Primary, Size::Sm)
                        .label("Done")
                        .key("ctl-done")
                        .build(|s: &mut App, _| s.close_dialog()),
                ),
        )
        .child(
            Button::new(Variant::Ghost, Size::IconSm)
                .icon(&icons::X)
                .a11y("Close")
                .key("ctl-close")
                .build(|s: &mut App, _| s.close_dialog())
                .absolute(w - 14.0 - 28.0, 14.0),
        )
        .on_key(|s: &mut App, ev: &rune::KeyEvent, _tx: &mut Tx| s.controller_key(ev));
    dialog_layer(
        "controller",
        "Controller setup",
        panel,
        None,
        |s: &mut App| s.close_dialog(),
    )
}
