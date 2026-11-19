//! The sample browser (`BrowserPanel.tsx`, `SampleBrowser.tsx`, `useSampleBrowser.ts`): user
//! root folders (persisted `micro-daw:browser-roots`), one level listed at a time (folders
//! first, audio files, the app's collation), breadcrumbs, a filter, keyboard navigation, click
//! = preview (auto-preview persisted), Enter / double-click = assign to the selected pad, rows
//! dragged onto a pad assign there, the panel resizes from its right edge (persisted width).

use mdaw_model::fs_entries::{EntryKind, FsDirEntry, list_entries};
use mdaw_model::paths::display_name;
use mdaw_model::settings::{self as store, SettingsStore};
use rune::{El, Key, KeyEvent, Phase, Tx, col, el, icon, row, spacer, text, text_input};

use crate::app::{App, SampleDrag};
use crate::icons;
use crate::theme::{R_SM, daw_label, mono, t, ui};
use crate::ui::widgets::{Button, Size, Variant, kbd};

/// The browser's state (`useSampleBrowser`).
pub struct BrowserState {
    pub roots: Vec<String>,
    /// None = the roots list.
    pub current: Option<String>,
    pub listing: Vec<FsDirEntry>,
    pub error: Option<String>,
    pub filter: rune::TextEdit,
    pub selected: Option<String>,
    pub preview: Option<String>,
    pub auto_preview: bool,
    pub width: f32,
    pub resize: Option<f32>,
    /// A listing runs on a worker thread (`fs:list-dir` is async in the TS).
    pub loading: bool,
    /// Bumped per listing request (`requestRef`): a slow result for an old folder is dropped.
    pub generation: u64,
    results: std::sync::mpsc::Receiver<Listing>,
    send: std::sync::mpsc::Sender<Listing>,
    /// Wakes the UI when a listing arrives (set once the window runs).
    pub waker: Option<rune::Waker>,
}

/// A finished listing: its request number, the row to select after it, the entries.
struct Listing {
    generation: u64,
    select_after: Option<String>,
    result: Result<Vec<FsDirEntry>, String>,
}

/// `filterEntries`: every whitespace-separated term must appear in the name (case-insensitive),
/// in any order (`1 kick` matches `Kick 1.wav`).
pub fn filter_entries(entries: Vec<FsDirEntry>, query: &str) -> Vec<FsDirEntry> {
    let q = query.trim().to_lowercase();
    let terms: Vec<&str> = q.split_whitespace().collect();
    if terms.is_empty() {
        return entries;
    }
    entries
        .into_iter()
        .filter(|e| {
            let name = e.name.to_lowercase();
            terms.iter().all(|t| name.contains(t))
        })
        .collect()
}

/// `moveIndex`.
pub fn move_index(current: Option<usize>, delta: i64, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    Some(match current {
        None if delta >= 0 => 0,
        None => len - 1,
        Some(c) => (c as i64 + delta).clamp(0, len as i64 - 1) as usize,
    })
}

const SEP: char = std::path::MAIN_SEPARATOR;

fn trim_sep(p: &str) -> String {
    let t = p.trim_end_matches(SEP);
    if t.is_empty() {
        p.to_owned()
    } else {
        t.to_owned()
    }
}

/// Lists a folder (`fs:list-dir`).
pub fn list_dir(dir: &str) -> Result<Vec<FsDirEntry>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let raw: Vec<(String, EntryKind)> = rd
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let kind = match e.file_type() {
                Ok(ft) if ft.is_dir() => EntryKind::Dir,
                Ok(ft) if ft.is_symlink() => {
                    if e.path().is_dir() {
                        EntryKind::Dir
                    } else {
                        EntryKind::File
                    }
                }
                _ => EntryKind::File,
            };
            (name, kind)
        })
        .collect();
    Ok(list_entries(dir, SEP, raw))
}

impl BrowserState {
    pub fn new(s: &SettingsStore) -> Self {
        let roots = store::read_browser_roots(s);
        let current = store::read_browser_last_path(s, &roots);
        let (send, results) = std::sync::mpsc::channel();
        let mut b = Self {
            roots,
            current: None,
            listing: Vec::new(),
            error: None,
            filter: rune::TextEdit::new(""),
            selected: None,
            preview: None,
            auto_preview: store::read_browser_auto_preview(s),
            width: store::read_browser_width(s) as f32,
            resize: None,
            loading: false,
            generation: 0,
            results,
            send,
            waker: None,
        };
        // The restored folder lists in the background like any other.
        b.navigate_to(current, None);
        b
    }

    /// `navigate`: the folder changes at once (filter cleared); its listing loads on a worker.
    fn navigate_to(&mut self, path: Option<String>, select_after: Option<String>) {
        self.current = path;
        self.clear_filter();
        self.load(select_after);
    }

    /// `setFilter('')`: empties the filter; a focused filter field keeps its focus (a fresh
    /// `TextEdit` would blur it).
    pub fn clear_filter(&mut self) {
        let focused = self.filter.focused;
        self.filter = rune::TextEdit::new("");
        self.filter.focused = focused;
    }

    /// `load(path, selectAfter)`: lists the current folder on a worker thread. A newer request
    /// makes this one's result stale (dropped in [`Self::poll`]).
    pub fn load(&mut self, select_after: Option<String>) {
        self.generation += 1;
        let generation = self.generation;
        self.error = None;
        let Some(path) = self.current.clone() else {
            self.listing.clear();
            self.loading = false;
            self.selected = select_after;
            return;
        };
        self.loading = true;
        let send = self.send.clone();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let result = list_dir(&path);
            let _ = send.send(Listing {
                generation,
                select_after,
                result,
            });
            if let Some(w) = waker {
                w();
            }
        });
    }

    /// Applies finished listings (the latest request only). True when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(l) = self.results.try_recv() {
            if l.generation != self.generation {
                continue;
            }
            self.loading = false;
            changed = true;
            match l.result {
                Ok(entries) => {
                    self.selected = l
                        .select_after
                        .or_else(|| entries.first().map(|e| e.path.clone()));
                    self.listing = entries;
                }
                Err(e) => {
                    self.listing.clear();
                    self.selected = None;
                    self.error = Some(if e.is_empty() {
                        "Could not read folder".to_owned()
                    } else {
                        e
                    });
                }
            }
        }
        changed
    }

    /// Waits (tests) until the current listing arrived, at most `timeout`.
    pub fn settle(&mut self, timeout: std::time::Duration) -> bool {
        let end = std::time::Instant::now() + timeout;
        while self.loading && std::time::Instant::now() < end {
            if !self.poll() {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
        !self.loading
    }

    /// The rows shown: the roots (as folders) or the folder's entries, filtered by name.
    pub fn entries(&self) -> Vec<FsDirEntry> {
        let all: Vec<FsDirEntry> = match &self.current {
            None => self
                .roots
                .iter()
                .map(|r| FsDirEntry {
                    name: std::path::Path::new(r)
                        .file_name()
                        .map_or_else(|| r.clone(), |n| n.to_string_lossy().into_owned()),
                    path: r.clone(),
                    is_dir: true,
                    ext: String::new(),
                })
                .collect(),
            Some(_) => self.listing.clone(),
        };
        filter_entries(all, &self.filter.text)
    }

    /// `selectedIndex`: the selected row, else while filtering the first match (Enter / Space
    /// act on it), else none.
    pub fn selected_index(&self, entries: &[FsDirEntry]) -> Option<usize> {
        let found = self
            .selected
            .as_ref()
            .and_then(|p| entries.iter().position(|e| &e.path == p));
        found.or_else(|| (!self.filter.text.trim().is_empty() && !entries.is_empty()).then_some(0))
    }

    pub fn total(&self) -> usize {
        match &self.current {
            None => self.roots.len(),
            Some(_) => self.listing.len(),
        }
    }

    fn crumbs(&self) -> Vec<(String, Option<String>)> {
        let mut out = vec![("Folders".to_owned(), None)];
        let Some(cur) = &self.current else { return out };
        let Some(root) = store::find_root_for(&self.roots, cur).map(str::to_owned) else {
            out.push((cur.clone(), Some(cur.clone())));
            return out;
        };
        let root_name = std::path::Path::new(&root)
            .file_name()
            .map_or_else(|| root.clone(), |n| n.to_string_lossy().into_owned());
        out.push((root_name, Some(root.clone())));
        let rest = cur
            .strip_prefix(&root)
            .unwrap_or("")
            .trim_start_matches(SEP);
        let mut acc = root.clone();
        for part in rest.split(SEP).filter(|p| !p.is_empty()) {
            acc = format!("{acc}{SEP}{part}");
            out.push((part.to_owned(), Some(acc.clone())));
        }
        out
    }
}

impl App {
    fn browser_persist_path(&mut self) {
        let cur = self.browser.current.clone();
        self.with_settings_mut(|s| {
            let _ = store::write_browser_last_path(s, cur.as_deref());
        });
    }

    pub fn browser_navigate(&mut self, path: Option<String>) {
        self.browser_navigate_selecting(path, None);
    }

    /// `navigate(path, selectAfter)`.
    pub fn browser_navigate_selecting(
        &mut self,
        path: Option<String>,
        select_after: Option<String>,
    ) {
        self.browser.navigate_to(path, select_after);
        self.browser_persist_path();
    }

    pub fn browser_add_root(&mut self, path: &str) {
        let p = trim_sep(path);
        if !self.browser.roots.contains(&p) {
            self.browser.roots.push(p.clone());
        }
        let roots = self.browser.roots.clone();
        self.with_settings_mut(|s| {
            let _ = store::write_browser_roots(s, &roots);
        });
        self.browser_navigate(Some(p));
    }

    pub fn browser_remove_root(&mut self, path: &str) {
        self.browser.roots.retain(|r| r != path);
        let roots = self.browser.roots.clone();
        self.with_settings_mut(|s| {
            let _ = store::write_browser_roots(s, &roots);
        });
        if self
            .browser
            .current
            .as_deref()
            .is_some_and(|c| store::is_within(path, c))
        {
            self.browser_navigate(None);
        } else if self.browser.current.is_none() && self.browser.selected.as_deref() == Some(path) {
            self.browser.selected = None;
        }
    }

    pub fn browser_go_up(&mut self) {
        let Some(cur) = self.browser.current.clone() else {
            return;
        };
        let root = store::find_root_for(&self.browser.roots, &cur).map(str::to_owned);
        let at_root = root
            .as_deref()
            .is_none_or(|r| trim_sep(r) == trim_sep(&cur));
        let parent = if at_root {
            None
        } else {
            std::path::Path::new(&cur)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
        };
        // Re-select the folder just left (its root row when leaving a root).
        let reselect = if at_root { root.unwrap_or(cur) } else { cur };
        self.browser_navigate_selecting(parent, Some(reselect));
    }

    /// Click on a row: folder → open, file → select (+ preview).
    pub fn browser_click(&mut self, e: &FsDirEntry) {
        if e.is_dir {
            self.browser_navigate(Some(e.path.clone()));
        } else {
            self.browser.selected = Some(e.path.clone());
            self.browser_preview(&e.path.clone());
        }
    }

    /// Enter / double-click: folder → open, file → assign to the selected pad.
    pub fn browser_activate(&mut self, e: &FsDirEntry) {
        if e.is_dir {
            self.browser_navigate(Some(e.path.clone()));
        } else {
            let pad = self.selected_pad();
            self.assign_file(pad, &e.path, &display_name(&e.name));
        }
    }

    pub fn browser_preview(&mut self, path: &str) {
        self.browser.preview = Some(path.to_owned());
        self.svc
            .packs
            .preview_file(path, path, &mut self.svc.engine);
    }

    fn browser_select(&mut self, index: usize, preview: bool) {
        let entries = self.browser.entries();
        let Some(e) = entries.get(index).cloned() else {
            return;
        };
        self.browser.selected = Some(e.path.clone());
        if preview && !e.is_dir {
            self.browser_preview(&e.path);
        }
    }

    fn browser_move(&mut self, delta: i64) {
        let entries = self.browser.entries();
        let cur = self.browser.selected_index(&entries);
        if let Some(next) = move_index(cur, delta, entries.len()) {
            let preview = self.browser.auto_preview;
            self.browser_select(next, preview);
        }
    }

    /// The list's keys (`handleNavKey`).
    pub fn browser_key(&mut self, ev: &KeyEvent, from_input: bool) -> bool {
        let entries = self.browser.entries();
        let sel = self
            .browser
            .selected_index(&entries)
            .and_then(|i| entries.get(i))
            .cloned();
        match &ev.key {
            Key::Down => self.browser_move(1),
            Key::Up => self.browser_move(-1),
            Key::PageDown => self.browser_move(10),
            Key::PageUp => self.browser_move(-10),
            Key::Enter => {
                if let Some(e) = sel {
                    self.browser_activate(&e);
                }
            }
            Key::Escape => {
                if self.browser.filter.text.is_empty() {
                    self.svc.packs.stop_preview(&mut self.svc.engine);
                    self.browser.preview = None;
                } else {
                    self.browser.clear_filter();
                }
            }
            _ if from_input => return false,
            Key::Home => {
                let preview = self.browser.auto_preview;
                self.browser_select(0, preview);
            }
            Key::End => {
                let preview = self.browser.auto_preview;
                self.browser_select(entries.len().saturating_sub(1), preview);
            }
            Key::Right => {
                if let Some(e) = sel.filter(|e| e.is_dir) {
                    self.browser_navigate(Some(e.path));
                }
            }
            Key::Left | Key::Backspace => self.browser_go_up(),
            Key::Text(t) if t == " " => {
                if let Some(e) = sel.filter(|e| !e.is_dir) {
                    self.browser_preview(&e.path);
                }
            }
            _ => return false,
        }
        true
    }

    /// A row dragged out of the list: the pad under the pointer takes the sample on release.
    pub fn browser_row_drag(&mut self, e: &FsDirEntry, d: &rune::Drag, tx: &mut Tx) {
        if e.is_dir {
            return;
        }
        match d.phase {
            Phase::Down => {}
            Phase::Move => {
                if (d.dx.abs() + d.dy.abs()) < 4.0 && self.sample_drag.is_none() {
                    return;
                }
                let at = d.window_point();
                let over = self.sample_drag.as_ref().and_then(|d| d.over);
                self.sample_drag = Some(SampleDrag {
                    path: e.path.clone(),
                    name: display_name(&e.name),
                    over,
                    at,
                });
                tx.element_at(
                    at[0],
                    at[1],
                    |s: &mut App, picked: Option<rune::Picked>, _tx: &mut Tx| {
                        let over = picked.and_then(|p| pad_in_path(p.path()));
                        if let Some(d) = &mut s.sample_drag {
                            d.over = over;
                        }
                    },
                );
            }
            Phase::Up => {
                if let Some(drag) = self.sample_drag.take()
                    && !d.cancelled()
                    && let Some(p) = drag.over
                {
                    self.assign_file(p, &drag.path, &drag.name);
                    self.select_pad(p);
                }
            }
        }
    }
}

fn header(s: &App) -> El {
    let th = t();
    let b = &s.browser;
    let mut r = row()
        .h(32.0)
        .w_full()
        .items_center()
        .gap(2.0)
        .pl(12.0)
        .pr(4.0)
        .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
        .child(
            text("Browser")
                .font(daw_label(11.0))
                .text_case(rune::TextCase::Uppercase)
                .color(th.muted_fg),
        )
        .child(spacer())
        .child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(if b.auto_preview {
                    &icons::VOLUME_2
                } else {
                    &icons::VOLUME_X
                })
                .a11y("Auto-preview while navigating")
                .tip(
                    if b.auto_preview {
                        "Auto-preview on"
                    } else {
                        "Auto-preview off"
                    },
                    None,
                )
                .pressed(b.auto_preview)
                .key("browser-auto-preview")
                .build(|s: &mut App, _| {
                    s.browser.auto_preview = !s.browser.auto_preview;
                    let on = s.browser.auto_preview;
                    s.with_settings_mut(|st| {
                        let _ = store::write_browser_auto_preview(st, on);
                    });
                }),
        );
    if b.current.is_some() {
        r = r.child(
            Button::new(Variant::Ghost, Size::IconXs)
                .icon(&icons::REFRESH_CW)
                .a11y("Refresh folder")
                .tip("Refresh", None)
                .build(|s: &mut App, _| {
                    // `refresh`: the same folder, selection kept, filter kept.
                    let sel = s.browser.selected.clone();
                    s.browser.load(sel);
                }),
        );
    }
    r.child(
        Button::new(Variant::Ghost, Size::IconXs)
            .icon(&icons::FOLDER_PLUS)
            .a11y("Add folder")
            .tip("Add folder", None)
            .key("browser-add-folder")
            .build(|s: &mut App, tx: &mut Tx| s.pick_browser_root(tx)),
    )
}

pub fn view(s: &App) -> El {
    let th = t();
    let b = &s.browser;
    let width = b.width;
    let mut panel = col()
        .key("browser")
        .role("region")
        .label("Sample browser")
        .w(width)
        .h_full()
        .fill(th.surface1)
        .border_sides([0.0, 1.0, 0.0, 0.0], th.border)
        .child(header(s));
    if b.roots.is_empty() {
        panel = panel.child(
            col()
                .grow(1.0)
                .items_center()
                .px(16.0)
                .pt(40.0)
                .gap(8.0)
                .child(
                    row().size(40.0, 40.0).center().radius(8.0).fill(th.surface2).border(1.0, th.border)
                        .child(icon(&icons::FOLDER_OPEN, 20.0).color(th.muted_fg)),
                )
                .child(text("Add a sample folder").font(ui(14.0, 500.0)).color(th.fg).mt(8.0))
                .child(
                    text("Point the browser at folders with your drums and one-shots. Click to preview, drag onto a pad or press Enter to assign.")
                        .font(ui(12.0, 400.0).lh(18.0))
                        .color(th.muted_fg)
                        .align(rune::Align::Center)
                        .wrap(),
                )
                .child(
                    Button::new(Variant::Outline, Size::Sm)
                        .icon(&icons::FOLDER_PLUS)
                        .icon_size(14.0)
                        .label("Choose folder…")
                        .key("browser-choose")
                        .build(|s: &mut App, tx: &mut Tx| s.pick_browser_root(tx))
                        .mt(4.0),
                ),
        );
        return panel;
    }
    // Breadcrumbs
    let mut crumbs = row().h(28.0).px(8.0).items_center().gap(2.0).min_w(0.0);
    let all = b.crumbs();
    let n = all.len();
    for (i, (name, path)) in all.into_iter().enumerate() {
        if i > 0 {
            crumbs = crumbs.child(icon(&icons::CHEVRON_RIGHT, 12.0).color(th.muted_fg));
        }
        crumbs = crumbs.child(
            text(name)
                .font(ui(11.5, if i + 1 == n { 600.0 } else { 400.0 }))
                .color(if i + 1 == n { th.fg } else { th.muted_fg })
                .truncate()
                .px(2.0)
                .on_press(move |s: &mut App| s.browser_navigate(path.clone())),
        );
    }
    panel = panel.child(crumbs);
    // Filter
    panel = panel.child(
        row()
            .w_full()
            .padding(6.0)
            .border_sides([0.0, 0.0, 1.0, 0.0], th.border)
            .child(
                row()
                    .h(24.0)
                    .w_full()
                    .items_center()
                    .gap(4.0)
                    .px(6.0)
                    .radius(R_SM)
                    .border(1.0, th.input)
                    .fill(th.surface0)
                    .child(icon(&icons::SEARCH, 12.0).color(th.muted_fg))
                    .child(
                        text_input(&b.filter, ui(12.0, 400.0))
                            .key("browser-filter")
                            .label("Filter")
                            .single_line()
                            .placeholder(
                                if b.current.is_none() {
                                    "Filter folders"
                                } else {
                                    "Filter this folder"
                                },
                                th.muted_fg,
                            )
                            .color(th.fg)
                            .grow(1.0)
                            .on_edit(|s: &mut App| &mut s.browser.filter)
                            .on_input_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| {
                                s.browser_key(ev, true)
                            }),
                    ),
            ),
    );
    // Rows
    let entries = b.entries();
    let at_roots = b.current.is_none();
    let mut list = col()
        .key("browser-list")
        .role("listbox")
        .label(if at_roots {
            "Sample folders".to_owned()
        } else {
            "Folder contents".to_owned()
        })
        .focusable(true)
        .padding(4.0)
        .on_key(|s: &mut App, ev: &KeyEvent, _tx: &mut Tx| s.browser_key(ev, false));
    if let Some(err) = &b.error {
        list = list.child(
            text(err.clone())
                .font(ui(11.5, 400.0))
                .color(th.destructive)
                .px(8.0)
                .py(12.0),
        );
    } else if b.loading && entries.is_empty() {
        list = list.child(
            row()
                .key("browser-loading")
                .items_center()
                .gap(6.0)
                .px(8.0)
                .py(12.0)
                .child(icon(&icons::LOADER_CIRCLE, 12.0).color(th.muted_fg))
                .child(text("Loading…").font(ui(11.5, 400.0)).color(th.muted_fg)),
        );
    } else if entries.is_empty() {
        let f = b.filter.text.clone();
        list = list.child(
            text(if f.is_empty() {
                "No folders or audio files here".to_owned()
            } else {
                format!("Nothing matches \"{f}\"")
            })
            .font(ui(11.5, 400.0))
            .color(th.muted_fg)
            .px(8.0)
            .py(12.0),
        );
    }
    let sel_index = b.selected_index(&entries);
    for (i, e) in entries.iter().enumerate() {
        let sel = sel_index == Some(i);
        let previewing = b.preview.as_ref() == Some(&e.path);
        let (ec, ea, ed, er) = (e.clone(), e.clone(), e.clone(), e.path.clone());
        let mut r = row()
            .key(format!("browser-row-{i}"))
            .role("option")
            .focus_selected(sel)
            .label(e.name.clone())
            .h(26.0)
            .w_full()
            .items_center()
            .gap(6.0)
            .px(8.0)
            .radius(R_SM)
            .fill(if sel {
                th.surface3
            } else {
                rune::Color::TRANSPARENT
            })
            .hover(th.surface2)
            .child(
                icon(
                    if e.is_dir {
                        &icons::FOLDER
                    } else {
                        &icons::FILE_AUDIO
                    },
                    14.0,
                )
                .color(if e.is_dir { th.muted_fg } else { th.select }),
            )
            .child(
                text(if e.is_dir {
                    e.name.clone()
                } else {
                    display_name(&e.name)
                })
                .font(ui(12.0, 400.0))
                .color(th.fg)
                .truncate()
                .grow(1.0)
                .min_w(0.0),
            );
        if previewing {
            r = r.child(icon(&icons::VOLUME_2, 12.0).color(th.primary));
        }
        if at_roots {
            r = r.child(
                row()
                    .role("button")
                    .label("Remove folder")
                    .padding(2.0)
                    .radius(R_SM)
                    .child(icon(&icons::X, 12.0).color(th.muted_fg))
                    .on_press(move |s: &mut App| s.browser_remove_root(&er)),
            );
        }
        r = r
            .on_press(move |s: &mut App| s.browser_click(&ec))
            .on_double_press(move |s: &mut App| {
                if !ea.is_dir {
                    s.browser_activate(&ea);
                }
            })
            .on_drag(move |s: &mut App, d: &mut rune::Drag, tx: &mut Tx| {
                s.browser_row_drag(&ed, d, tx)
            });
        list = list.child(r);
    }
    panel = panel.child(
        list.scroll(rune::ScrollSpec {
            stick_bottom: false,
        })
        .grow(1.0)
        .min_h(0.0),
    );
    let total = b.total();
    let count = if b.filter.text.is_empty() {
        format!("{total} {}", if total == 1 { "item" } else { "items" })
    } else {
        format!(
            "{}/{total} {}",
            entries.len(),
            if total == 1 { "item" } else { "items" }
        )
    };
    let mut footer = row()
        .h(24.0)
        .w_full()
        .items_center()
        .gap(8.0)
        .px(8.0)
        .border_sides([1.0, 0.0, 0.0, 0.0], th.border)
        .child(text(count).font(mono(11.0, 400.0)).color(th.muted_fg))
        .child(spacer());
    if width >= 256.0 {
        footer = footer
            .child(kbd("↵"))
            .child(text("assign").font(mono(11.0, 400.0)).color(th.muted_fg))
            .child(kbd("␣"))
            .child(text("play").font(mono(11.0, 400.0)).color(th.muted_fg));
    }
    panel = panel.child(footer);
    // Resize handle on the right edge.
    let handle = el()
        .key("browser-resize")
        .role("separator")
        .label("Resize sample browser")
        .pin(None, Some(0.0), Some(-4.0), Some(0.0))
        .w(8.0)
        .cursor(rune::Cursor::ResizeColumn)
        .on_drag(
            |s: &mut App, d: &mut rune::Drag, _tx: &mut Tx| match d.phase {
                Phase::Down => s.browser.resize = Some(s.browser.width),
                Phase::Move => {
                    if let Some(start) = s.browser.resize {
                        s.browser.width = store::clamp_browser_width((start + d.dx) as f64) as f32;
                    }
                }
                Phase::Up => {
                    s.browser.resize = None;
                    let w = s.browser.width as f64;
                    s.with_settings_mut(|st| {
                        let _ = store::write_browser_width(st, w);
                    });
                }
            },
        )
        .on_double_press(|s: &mut App| {
            s.browser.width = store::BROWSER_DEFAULT_WIDTH as f32;
            s.with_settings_mut(|st| {
                let _ = store::write_browser_width(st, store::BROWSER_DEFAULT_WIDTH);
            });
        });
    panel.child(handle)
}

impl App {
    /// "Add folder" / "Choose folder…" (`fs:pick-folder`).
    pub fn pick_browser_root(&mut self, tx: &mut Tx) {
        let options = rune::FileDialogOptions {
            title: "Choose a sample folder".into(),
            directory: None,
            file_name: None,
            filters: Vec::new(),
        };
        tx.open_folder(
            options,
            |s: &mut App, r: rune::FileDialogResult, _tx: &mut Tx| {
                if let Ok(Some(dir)) = r {
                    s.browser_add_root(&dir.to_string_lossy());
                }
            },
        );
    }
}

/// The pad (`pad-N` key) an element path is inside, if any.
pub fn pad_in_path(path: &str) -> Option<usize> {
    path.split('/').find_map(|seg| {
        seg.strip_prefix("pad-")
            .and_then(|n| n.parse::<usize>().ok())
    })
}
