//! The changes panel: the checkout's changed files and, beside them, each file's diff on a
//! card of its own.
//!
//! The diffs are one virtualised list of rows, so only what's on screen is laid out however
//! long the diff. A live update replaces only the rows that differ, so the list stays where
//! it was scrolled to while an agent writes.

use std::collections::HashSet;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_git::{Change, Checkout, Content, Discarding, Status};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Context, Div, DragMoveEvent, Empty, Entity, FocusHandle, Font, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListOffset, ListScrollEvent,
    ListState, ParentElement as _, Pixels, Render, ScrollStrategy, SharedString,
    StatefulInteractiveElement as _, Styled, Subscription, UniformListScrollHandle, Window,
    actions, div, font, list, px, uniform_list,
};

use super::discard;
use super::highlight::Token;
use super::line::{CodeLine, Half, Look, runs};
use super::rows::{Layout, Row, Shown, rows};
use super::state::{Changes, FileView, GitAct, Key};
use super::tree::{self, entries};
use crate::modal::{self, spinner};
use crate::settings;
use crate::shell::{ellipsized, icon, icon_button, row};
use crate::sidebar::resize_handle;
use crate::theme::{self, Palette};

actions!(
    changes,
    [
        ToggleChangesWidth,
        SelectNext,
        SelectPrevious,
        NextFile,
        PreviousFile,
        OpenFolder,
        CloseFolder,
        ToggleFolder
    ]
);

/// The keys that move through the file list while it has focus.
pub fn init(cx: &mut gpui_kit::App) {
    let files = Some("ChangesFiles");
    cx.bind_keys([
        KeyBinding::new("down", SelectNext, files),
        KeyBinding::new("up", SelectPrevious, files),
        KeyBinding::new("j", NextFile, files),
        KeyBinding::new("k", PreviousFile, files),
        KeyBinding::new("right", OpenFolder, files),
        KeyBinding::new("left", CloseFolder, files),
        KeyBinding::new("enter", ToggleFolder, files),
        KeyBinding::new("space", ToggleFolder, files),
    ]);
}

/// The drag payload while the border between the file list and the diffs is being dragged.
#[derive(Clone)]
pub struct FilesResize;

impl Render for FilesResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The drag payload while the panel's left border is being dragged.
#[derive(Clone)]
pub struct ChangesResize;

impl Render for ChangesResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

/// The file list's groups, as Zed shows them grouped by staging: what's staged, then what
/// isn't. A file partly staged is in both.
struct Group {
    staged: bool,
    name: &'static str,
}

const GROUPS: [Group; 2] = [
    Group {
        staged: true,
        name: "Staged",
    },
    Group {
        staged: false,
        name: "Unstaged",
    },
];

/// What a file list row is to its actions, which show while it's hovered.
const ENTRY: &str = "changes-entry";

#[derive(Clone, Copy)]
enum Act {
    Stage,
    Unstage,
    Discard,
}

/// A row of the file list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    /// A folder of one group's files. The group itself is the folder at its top, whose path
    /// is empty.
    Folder {
        staged: bool,
        path: String,
        name: String,
        depth: usize,
    },
    /// The `index`th of the files.
    File { index: usize, depth: usize },
}

impl Entry {
    /// What a folder is opened and closed by.
    fn folder(&self) -> Option<Key> {
        match self {
            Entry::Folder { staged, path, .. } => Some((*staged, path.clone())),
            Entry::File { .. } => None,
        }
    }
}

/// The file list's rows: each group with files, holding them as a tree or flat, without what's
/// in the folders `closed`.
fn file_entries(files: &[Arc<FileView>], tree: bool, closed: &HashSet<Key>) -> Vec<Entry> {
    let mut list = Vec::new();
    for group in &GROUPS {
        let in_group: Vec<usize> = (0..files.len())
            .filter(|&index| files[index].diff.change.staged == group.staged)
            .collect();
        if in_group.is_empty() {
            continue;
        }
        list.push(Entry::Folder {
            staged: group.staged,
            path: String::new(),
            name: group.name.to_owned(),
            depth: 0,
        });
        if closed.contains(&(group.staged, String::new())) {
            continue;
        }
        let inside = if tree {
            let closed: HashSet<String> = (closed.iter())
                .filter(|(staged, _)| *staged == group.staged)
                .map(|(_, path)| path.clone())
                .collect();
            let paths = in_group
                .iter()
                .map(|&index| files[index].diff.change.path.as_str());
            entries(paths, &closed)
        } else {
            (0..in_group.len())
                .map(|index| tree::Entry::File { index, depth: 0 })
                .collect()
        };
        list.extend(inside.into_iter().map(|entry| match entry {
            tree::Entry::Folder { path, name, depth } => Entry::Folder {
                staged: group.staged,
                path,
                name,
                depth: depth + 1,
            },
            tree::Entry::File { index, depth } => Entry::File {
                index: in_group[index],
                depth: depth + 1,
            },
        }));
    }
    list
}

const LINE_HEIGHT: f32 = 20.;
const FILE_ROW_HEIGHT: f32 = 26.;
/// The width of a digit of a line number, in the code's font at its size.
const DIGIT_WIDTH: f32 = 7.3;
/// How long the diffs take to glide to a file picked in the list.
const GLIDE: Duration = Duration::from_millis(180);

/// The diffs on their way to a file's header.
struct Glide {
    file: usize,
    /// How far past the header the diffs were when they set off, up being negative. A file
    /// further away than a screen is jumped to within a screen of first.
    from: f32,
    start: Instant,
}

pub struct ChangesPanel {
    changes: Entity<Changes>,
    /// What the rows were built from, so a file read again shows as changed rows.
    files: Vec<Arc<FileView>>,
    rows: Vec<Row>,
    /// Where each file's header is in `rows`.
    headers: Vec<usize>,
    /// The file list's rows.
    entries: Vec<Entry>,
    list: ListState,
    layout: Layout,
    wrap: bool,
    tree: bool,
    /// How wide the panel is beside the chat, or `None` if it has the card to itself.
    width: Option<f32>,
    shown: Shown,
    /// The file list's closed folders.
    closed: HashSet<Key>,
    /// The file the diffs are scrolled to.
    current: Option<usize>,
    glide: Option<Glide>,
    /// The folder the keyboard is on, if it's on one rather than on the current file.
    folder: Option<Key>,
    focus: FocusHandle,
    files_scroll: UniformListScrollHandle,
    _subscriptions: [Subscription; 3],
}

impl ChangesPanel {
    /// The panel showing `changes`, which it colours while it's open.
    pub fn new(changes: Entity<Changes>, cx: &mut Context<Self>) -> Self {
        let _subscriptions = [
            settings::observe(cx),
            cx.observe(&changes, |this, changes, cx| {
                // Colouring files says so too; the same files only need drawing again.
                let files = changes.read(cx).files();
                let same = files.len() == this.files.len()
                    && files
                        .iter()
                        .zip(&this.files)
                        .all(|(a, b)| Arc::ptr_eq(a, b));
                if same {
                    cx.notify();
                } else {
                    this.rebuild(cx);
                }
            }),
            // Its colours aren't needed while nothing shows them.
            cx.on_release(|this, cx| {
                (this.changes).update(cx, |changes, cx| changes.set_coloured(false, cx));
            }),
        ];
        changes.update(cx, |changes, cx| changes.set_coloured(true, cx));
        let list = ListState::new(0, ListAlignment::Top, px(400.));
        // Scrolling by hand takes over from a glide, and the keyboard follows to the file in
        // view. The list is mid-update when it calls this, so it's asked where it is after.
        list.set_scroll_handler(cx.listener(|this, _: &ListScrollEvent, window, cx| {
            this.glide = None;
            this.folder = None;
            cx.defer_in(window, |this, _, cx| this.follow_scroll(cx));
        }));
        let mut panel = ChangesPanel {
            changes,
            files: Vec::new(),
            rows: Vec::new(),
            headers: Vec::new(),
            entries: Vec::new(),
            list,
            layout: Layout::default(),
            wrap: true,
            tree: true,
            width: None,
            shown: Shown::default(),
            closed: HashSet::new(),
            current: None,
            glide: None,
            folder: None,
            focus: cx.focus_handle(),
            files_scroll: UniformListScrollHandle::new(),
            _subscriptions,
        };
        // What was read before the panel opened.
        panel.rebuild(cx);
        panel
    }

    pub fn changes(&self) -> &Entity<Changes> {
        &self.changes
    }

    /// The file list's, which the arrow keys move through.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    pub fn set_width(&mut self, width: Option<f32>, cx: &mut Context<Self>) {
        if self.width != width {
            self.width = width;
            cx.notify();
        }
    }

    /// The widest the file list can be beside the diffs and leave them room.
    fn widest_files(&self) -> f32 {
        self.width
            .map_or(f32::MAX, |width| width - theme::DIFFS_MIN_WIDTH)
    }

    /// The file list's width beside the diffs: as set, unless the panel is too narrow for it.
    fn files_width(&self, cx: &gpui_kit::App) -> f32 {
        settings::get(cx)
            .changes_files_width()
            .min(self.widest_files())
            .max(theme::CHANGES_FILES_MIN_WIDTH)
    }

    /// Builds the rows again from the files, telling the list which rows differ.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let files = self.changes.read(cx).files().to_vec();
        let rows = rows(
            files.iter().map(|file| &file.diff),
            self.layout,
            &self.shown,
        );
        let id = |files: &[Arc<FileView>], row: &Row| Arc::as_ptr(&files[row.file()]);
        let same = |old: &Row, new: &Row| *old == *new && id(&self.files, old) == id(&files, new);
        let prefix = self
            .rows
            .iter()
            .zip(&rows)
            .take_while(|(old, new)| same(old, new))
            .count();
        let suffix = self.rows[prefix..]
            .iter()
            .rev()
            .zip(rows[prefix..].iter().rev())
            .take_while(|(old, new)| same(old, new))
            .count();
        // A file at the top whose rows all changed would leave the list at the top of the
        // change; keep it on that file instead.
        let top = self.list.logical_scroll_top();
        let gliding = (self.glide.as_ref()).map(|glide| self.files[glide.file].key());
        let kept = (self.rows.get(top.item_ix))
            .filter(|_| top.item_ix >= prefix && top.item_ix < self.rows.len() - suffix)
            .map(|row| self.files[row.file()].key());
        self.list.splice(
            prefix..self.rows.len() - suffix,
            rows.len() - prefix - suffix,
        );
        self.headers = (rows.iter().enumerate())
            .filter_map(|(index, row)| matches!(row, Row::File(_)).then_some(index))
            .collect();
        let find = |files: &[Arc<FileView>], (staged, path): &Key| {
            (files.iter()).position(|file| {
                file.diff.change.staged == *staged && file.diff.change.path == *path
            })
        };
        if let Some(kept) = kept
            && let Some(file) = find(&files, &kept)
        {
            self.scroll_to_row(self.headers[file]);
        }
        self.rows = rows;
        self.files = files;
        self.rebuild_entries(cx);
        // A glide carries on to its file wherever that is now.
        let gliding = gliding.and_then(|gliding| find(&self.files, &gliding));
        if let (Some(glide), Some(file)) = (&mut self.glide, gliding) {
            glide.file = file;
        } else {
            self.glide = None;
            self.follow_scroll(cx);
        }
        cx.notify();
    }

    /// Builds the file list's rows again, from the files as they are.
    fn rebuild_entries(&mut self, cx: &mut Context<Self>) {
        self.entries = file_entries(&self.files, self.tree, &self.closed);
        cx.notify();
    }

    fn scroll_to_row(&self, index: usize) {
        self.list.scroll_to(ListOffset {
            item_ix: index,
            offset_in_item: px(0.),
        });
    }

    /// How far the diffs are scrolled, in pixels from their top.
    fn scrolled(&self) -> f32 {
        -self.list.scroll_px_offset_for_scrollbar().y.as_f32()
    }

    /// Notes which file the diffs are scrolled to, keeping it in view in the file list.
    fn follow_scroll(&mut self, cx: &mut Context<Self>) {
        let top = self.list.logical_scroll_top().item_ix;
        let current = self.rows.get(top).map(|row| row.file());
        if current != self.current {
            self.current = current;
            if let Some(entry) = self.cursor() {
                self.files_scroll
                    .scroll_to_item(entry, ScrollStrategy::Nearest);
            }
            cx.notify();
        }
    }

    /// The scroll offset of the `index`th row's top, by the heights the list knows.
    fn offset_of(&self, index: usize) -> f32 {
        let now = self.list.logical_scroll_top();
        self.scroll_to_row(index);
        let offset = self.scrolled();
        self.list.scroll_to(now);
        offset
    }

    /// Moves the diffs along their glide to where they are by now, asking for another frame
    /// until they're there.
    fn glide(&mut self, window: &mut Window) {
        let Some(glide) = &self.glide else {
            return;
        };
        let done = (glide.start.elapsed().as_secs_f32() / GLIDE.as_secs_f32()).min(1.);
        // Eased out, so it sets off fast and settles on the file.
        let left = glide.from * (1. - done).powi(3);
        self.scroll_to_row(self.headers[glide.file]);
        self.list.scroll_by(px(left));
        if done < 1. {
            window.request_animation_frame();
        } else {
            self.glide = None;
        }
    }

    fn show(&mut self, file: usize, cx: &mut Context<Self>) {
        // A collapsed file opens when it's picked from the list.
        if self.shown.collapsed.remove(&self.files[file].key()) {
            self.rebuild(cx);
        }
        let header = self.headers[file];
        if cx.reduce_motion() {
            self.scroll_to_row(header);
        } else {
            let screen = self.list.viewport_bounds().size.height.as_f32();
            let from = (self.scrolled() - self.offset_of(header)).clamp(-screen, screen);
            self.glide = Some(Glide {
                file,
                from,
                start: Instant::now(),
            });
        }
        self.current = Some(file);
        cx.notify();
    }

    fn toggle_collapsed(&mut self, file: usize, cx: &mut Context<Self>) {
        let key = self.files[file].key();
        if !self.shown.collapsed.remove(&key) {
            self.shown.collapsed.insert(key);
        }
        self.rebuild(cx);
    }

    /// The file list entry the keyboard is on: a folder it moved to, or the current file.
    fn cursor(&self) -> Option<usize> {
        if let Some(folder) = &self.folder {
            (self.entries.iter()).position(|entry| entry.folder().as_ref() == Some(folder))
        } else {
            let current = self.current?;
            (self.entries.iter())
                .position(|entry| matches!(entry, Entry::File { index, .. } if *index == current))
        }
    }

    /// Moves the keyboard to the file list's `index`th entry, showing it if it's a file.
    fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.entries.get(index) else {
            return;
        };
        self.files_scroll
            .scroll_to_item(index, ScrollStrategy::Nearest);
        match *entry {
            Entry::Folder { .. } => {
                self.folder = entry.folder();
                cx.notify();
            }
            Entry::File { index: file, .. } => {
                self.folder = None;
                self.show(file, cx);
            }
        }
    }

    /// Moves the keyboard `step` entries down the file list, or up if it's negative, stopping
    /// at its ends. With `files_only`, folders are passed over.
    fn step(&mut self, step: isize, files_only: bool, cx: &mut Context<Self>) {
        let len = self.entries.len() as isize;
        // With nothing picked, the first step lands on the list's first or last entry.
        let mut index = match self.cursor() {
            Some(index) => index as isize,
            None if step > 0 => -1,
            None => len,
        };
        loop {
            index += step;
            if !(0..len).contains(&index) {
                return;
            }
            let index = index as usize;
            if !files_only || matches!(self.entries[index], Entry::File { .. }) {
                return self.select(index, cx);
            }
        }
    }

    /// Opens the folder the keyboard is on, or closes it, or with `open` set moves into an open
    /// one; with `open` unset, moves from a file or a closed folder up to the folder holding it.
    fn open_or_close(&mut self, open: Option<bool>, cx: &mut Context<Self>) {
        let Some(index) = self.cursor() else {
            return;
        };
        let entry = &self.entries[index];
        let (Entry::Folder { depth, .. } | Entry::File { depth, .. }) = *entry;
        let folder = entry.folder();
        let is_open = folder
            .as_ref()
            .is_some_and(|folder| !self.closed.contains(folder));
        match (folder, open) {
            (Some(folder), None) => self.toggle_folder(folder, cx),
            (Some(folder), Some(true)) if !is_open => self.toggle_folder(folder, cx),
            (Some(_), Some(true)) => self.step(1, false, cx),
            (Some(folder), Some(false)) if is_open => self.toggle_folder(folder, cx),
            (None, Some(true)) | (_, None) => {}
            (_, Some(false)) => {
                let parent = (self.entries[..index].iter()).rposition(
                    |entry| matches!(entry, Entry::Folder { depth: d, .. } if *d < depth),
                );
                if let Some(parent) = parent {
                    self.select(parent, cx);
                }
            }
        }
    }

    fn toggle_folder(&mut self, folder: Key, cx: &mut Context<Self>) {
        if !self.closed.remove(&folder) {
            self.closed.insert(folder);
        }
        self.rebuild_entries(cx);
    }

    fn unfold(&mut self, file: usize, new: u32, cx: &mut Context<Self>) {
        let key = self.files[file].key();
        self.shown.unfolded.entry(key).or_default().insert(new);
        self.rebuild(cx);
    }
}

impl Render for ChangesPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        self.glide(window);
        let changes = self.changes.read(cx);
        let body = if changes.is_loading() {
            div()
                .flex_1()
                .relative()
                .child(spinner(p.muted))
                .into_any_element()
        } else if let Some(error) = changes.error()
            && self.files.is_empty()
        {
            note(p)
                .text_color(p.danger)
                .child(error.to_owned())
                .into_any_element()
        } else if self.files.is_empty() {
            note(p)
                .gap_2()
                .child(icon(IconName::CircleCheck, 14.))
                .child("No changes")
                .into_any_element()
        } else {
            let files = self.file_list(p, cx);
            // How the diff lines look this frame, shared by them all; the code's size is what
            // `text_xs` gives.
            let look = Rc::new(look(self.wrap, p, window.rem_size() * 0.75));
            let diffs = div().flex_1().min_w_0().min_h_0().child(
                list(
                    self.list.clone(),
                    cx.processor(move |this, index, _, cx| this.row(index, &look, cx)),
                )
                .size_full(),
            );
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .on_drag_move(
                    cx.listener(|this, event: &DragMoveEvent<FilesResize>, _, cx| {
                        let width = event.event.position.x - event.bounds.left();
                        let width = width.as_f32().min(this.widest_files());
                        settings::update(cx, |settings| settings.changes_files_width = width);
                    }),
                )
                .child(
                    div()
                        .relative()
                        .flex_shrink_0()
                        .w(px(self.files_width(cx)))
                        .h_full()
                        .child(files.size_full().border_r_1())
                        .child(resize_handle("changes-files-resize", FilesResize).right_0()),
                )
                .child(diffs)
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(p, cx))
            .child(body)
    }
}

impl ChangesPanel {
    fn header(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let toggle = |id: &'static str, glyph: IconName, tip: &'static str, on: bool| {
            icon_button(id, glyph, tip, cx).xsmall().selected(on)
        };
        div()
            .h(px(theme::TAB_STRIP_HEIGHT))
            .flex_shrink_0()
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(p.hairline)
            // The rail's button and the file list's top row already say what's here and how big.
            .child(div().flex_1())
            .when_some(self.changes.read(cx).unwatched(), |header, reason| {
                let tip = format!("Not following changes as they happen: {reason}");
                header.child(
                    icon_button("changes-unwatched", IconName::TriangleAlert, tip, cx).xsmall(),
                )
            })
            .child(
                toggle(
                    "changes-tree",
                    IconName::ListTree,
                    "Show folders",
                    self.tree,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.tree = !this.tree;
                    this.rebuild_entries(cx);
                })),
            )
            .child(
                toggle(
                    "changes-split",
                    IconName::Columns2,
                    "Side by side",
                    self.layout == Layout::Split,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.layout = match this.layout {
                        Layout::Unified => Layout::Split,
                        Layout::Split => Layout::Unified,
                    };
                    this.rebuild(cx);
                })),
            )
            .child(
                toggle("changes-wrap", IconName::TextWrap, "Wrap lines", self.wrap).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.wrap = !this.wrap;
                        this.list.remeasure();
                        cx.notify();
                    }),
                ),
            )
            .child({
                let (glyph, tip) = match self.width {
                    None => (IconName::Minimize2, "Back beside the chat"),
                    Some(_) => (IconName::Maximize2, "Use the whole card"),
                };
                icon_button("changes-width", glyph, tip, cx)
                    .xsmall()
                    .on_click(|_, window, cx| {
                        window.dispatch_action(Box::new(ToggleChangesWidth), cx);
                    })
            })
    }

    fn file_list(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        div()
            .flex_shrink_0()
            .border_color(p.hairline)
            .py_1()
            .track_focus(&self.focus)
            .key_context("ChangesFiles")
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, false, cx)))
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, false, cx)))
            .on_action(cx.listener(|this, _: &NextFile, _, cx| this.step(1, true, cx)))
            .on_action(cx.listener(|this, _: &PreviousFile, _, cx| this.step(-1, true, cx)))
            .on_action(
                cx.listener(|this, _: &OpenFolder, _, cx| this.open_or_close(Some(true), cx)),
            )
            .on_action(
                cx.listener(|this, _: &CloseFolder, _, cx| this.open_or_close(Some(false), cx)),
            )
            .on_action(cx.listener(|this, _: &ToggleFolder, _, cx| this.open_or_close(None, cx)))
            .child(
                uniform_list(
                    "changes-files",
                    self.entries.len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        let p = theme::palette(cx);
                        range
                            .map(|index| this.entry(index, p, cx))
                            .collect::<Vec<_>>()
                    }),
                )
                .track_scroll(&self.files_scroll)
                .size_full()
                .px_1(),
            )
    }

    fn entry(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let indent = |depth: usize| px(8. + depth as f32 * 12.);
        let row = |id, selected| {
            row(id, selected, p)
                .group(ENTRY)
                .relative()
                .w_full()
                .h(px(FILE_ROW_HEIGHT))
                .gap_1p5()
                .rounded(px(5.))
        };
        match &self.entries[index] {
            Entry::Folder {
                staged,
                path,
                name,
                depth,
            } => {
                let folder = (*staged, path.clone());
                let open = !self.closed.contains(&folder);
                let group = path.is_empty();
                row(("folder", index), self.folder.as_ref() == Some(&folder))
                    .pl(indent(*depth))
                    .text_color(if group { p.text } else { p.muted })
                    .when(group, |row| row.font_weight(FontWeight::MEDIUM))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.focus, cx);
                        this.folder = Some(folder.clone());
                        this.toggle_folder(folder.clone(), cx);
                    }))
                    .child(icon(
                        if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        },
                        12.,
                    ))
                    .child(ellipsized(name.clone()))
                    .child(div().flex_1())
                    .map(|row| {
                        if group {
                            // A group's actions sit beside its count.
                            row.child(self.entry_actions(index, cx))
                                .child(badge(self.entry_files(index).len(), p))
                        } else {
                            row.child(over_the_end(self.entry_actions(index, cx)))
                        }
                    })
                    .into_any_element()
            }
            Entry::File { index: file, depth } => {
                let file = *file;
                let change = &self.files[file].diff.change;
                let (dir, name) = split_path(&change.path);
                let (added, removed) = self.files[file].diff.content.line_counts();
                let current = self.folder.is_none() && self.current == Some(file);
                row(("file", file), current)
                    .pl(indent(*depth) + px(if self.tree { 14. } else { 0. }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        window.focus(&this.focus, cx);
                        this.folder = None;
                        this.show(file, cx);
                    }))
                    .child(status_letter(&change.status, p))
                    .child(div().flex_shrink_0().child(name.to_owned()))
                    .when(!self.tree && !dir.is_empty(), |row| {
                        row.child(ellipsized(dir.to_owned()).text_xs().text_color(p.muted))
                    })
                    .child(div().flex_1())
                    // The actions take the counts' place while the row is hovered.
                    .child(counts(added, removed, p).group_hover(ENTRY, Styled::invisible))
                    .child(over_the_end(self.entry_actions(index, cx)))
                    .into_any_element()
            }
        }
    }

    /// Discarding, and staging or unstaging, what the file list's `index`th entry holds: shown
    /// while the row is hovered.
    fn entry_actions(&self, index: usize, cx: &mut Context<Self>) -> Div {
        let staged = self.entry_is_staged(index);
        let (stage, discard) = match &self.entries[index] {
            Entry::Folder { path, .. } if path.is_empty() => {
                let stage = if staged { "Unstage All" } else { "Stage All" };
                (stage, "Discard All")
            }
            Entry::Folder { .. } => {
                let stage = if staged {
                    "Unstage Folder"
                } else {
                    "Stage Folder"
                };
                (stage, "Discard Changes")
            }
            Entry::File { index: file, .. } => {
                let stage = if staged { "Unstage File" } else { "Stage File" };
                let discard = match self.files[*file].diff.change.discarding() {
                    Discarding::Trashed => "Trash File",
                    Discarding::Restored => "Restore File",
                    Discarding::Reverted | Discarding::Kept => "Discard Changes",
                };
                (stage, discard)
            }
        };
        let button = |id: &'static str, glyph: IconName, tip: &'static str, act: Act| {
            icon_button((id, index), glyph, tip, cx)
                .xsmall()
                .on_click(cx.listener(move |this, _, window, cx| {
                    // Not the row's own click, which shows the file or opens the folder.
                    cx.stop_propagation();
                    this.act(act, index, window, cx);
                }))
        };
        let (glyph, act) = if staged {
            (IconName::Minus, Act::Unstage)
        } else {
            (IconName::Plus, Act::Stage)
        };
        // Hidden by visibility, not display: a row laid out without them and then hovered
        // would paint them unlaid.
        div()
            .invisible()
            .group_hover(ENTRY, Styled::visible)
            .flex()
            .items_center()
            .child(button("discard", IconName::Undo2, discard, Act::Discard))
            .child(button("stage", glyph, stage, act))
    }

    /// Whether the file list's `index`th entry is in the staged group.
    fn entry_is_staged(&self, index: usize) -> bool {
        match &self.entries[index] {
            Entry::Folder { staged, .. } => *staged,
            Entry::File { index, .. } => self.files[*index].diff.change.staged,
        }
    }

    /// The files the file list's `index`th entry holds: itself, or a folder's or group's.
    fn entry_files(&self, index: usize) -> Vec<usize> {
        let (staged, folder) = match &self.entries[index] {
            Entry::Folder { staged, path, .. } => (*staged, path.as_str()),
            Entry::File { index, .. } => return vec![*index],
        };
        let in_folder = |path: &str| {
            folder.is_empty()
                || (path.strip_prefix(folder)).is_some_and(|rest| rest.starts_with('/'))
        };
        (0..self.files.len())
            .filter(|&file| {
                let change = &self.files[file].diff.change;
                change.staged == staged && in_folder(&change.path)
            })
            .collect()
    }

    /// Does `act` to what the file list's `index`th entry holds. Discarding asks first.
    fn act(&mut self, act: Act, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let list: Vec<Change> = (self.entry_files(index).into_iter())
            .map(|file| self.files[file].diff.change.clone())
            .collect();
        let (run, failed): (GitAct, _) = match act {
            Act::Stage => (Checkout::stage, "Couldn't stage"),
            Act::Unstage => (Checkout::unstage, "Couldn't unstage"),
            Act::Discard => return discard::confirm(self.changes.clone(), list, window, cx),
        };
        let done = (self.changes).update(cx, |changes, cx| changes.act(run, list, cx));
        cx.spawn_in(window, async move |_, cx| {
            if let Err(error) = done.await {
                cx.update(|window, cx| modal::alert(failed, error.message, window, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// The `index`th row of the diffs, framed as part of its file's card.
    fn row(&self, index: usize, look: &Rc<Look>, cx: &mut Context<Self>) -> AnyElement {
        let p = theme::palette(cx);
        let row = self.rows[index];
        let file = &self.files[row.file()];
        // Each row draws its stretch of the card's sides, so the list can lay them out apart.
        let card = || {
            div()
                .mx_3()
                .border_x_1()
                .border_color(p.hairline)
                .bg(p.sunken)
                .overflow_hidden()
        };
        match row {
            Row::File(index) => {
                let collapsed = self.shown.collapsed.contains(&file.key());
                div()
                    .pt_3()
                    .child(
                        card()
                            .border_t_1()
                            .rounded_t(px(8.))
                            .when(collapsed, |card| card.border_b_1().rounded_b(px(8.)))
                            .child(self.file_header(index, collapsed, p, cx)),
                    )
                    .into_any_element()
            }
            Row::End(_) => div()
                .when(index + 1 == self.rows.len(), Styled::pb_3)
                .child(card().h(px(4.)).border_b_1().rounded_b(px(8.)))
                .into_any_element(),
            Row::Note(_) => card()
                .border_t_1()
                .border_color(p.hairline)
                .px_3()
                .py_2()
                .text_xs()
                .text_color(p.muted)
                .child(note_for(file))
                .into_any_element(),
            Row::Fold { file, new, len, .. } => card()
                .child(
                    div()
                        .id(("fold", index))
                        .h(px(26.))
                        .px_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_xs()
                        .text_color(p.muted)
                        .bg(p.hover)
                        .cursor_pointer()
                        .hover(|style| style.bg(p.selected).text_color(p.text))
                        .on_click(cx.listener(move |this, _, _, cx| this.unfold(file, new, cx)))
                        .child(icon(IconName::UnfoldVertical, 12.))
                        .child(match len {
                            1 => "1 unmodified line".to_owned(),
                            len => format!("{len} unmodified lines"),
                        }),
                )
                .into_any_element(),
            Row::Line { old, new, .. } => {
                let (side, line, tint) = match (old, new) {
                    (Some(old), None) => (Side::Old, old, Some(p.removed)),
                    (None, Some(new)) => (Side::New, new, Some(p.added)),
                    (_, Some(new)) => (Side::New, new, None),
                    // Every line is on one side at least.
                    (None, None) => return Empty.into_any_element(),
                };
                let half = half(file, side, line, tint, vec![old, new], &look.font, p);
                CodeLine::new(vec![half], look.clone()).into_any_element()
            }
            Row::Pair {
                old, new, changed, ..
            } => {
                let side = |side, line: Option<u32>, tint| match line {
                    Some(line) => {
                        let tint = changed.then_some(tint);
                        half(file, side, line, tint, vec![Some(line)], &look.font, p)
                    }
                    None => Half::missing(px(number_width(file))),
                };
                let halves = vec![
                    side(Side::Old, old, p.removed),
                    side(Side::New, new, p.added),
                ];
                CodeLine::new(halves, look.clone()).into_any_element()
            }
        }
    }

    fn file_header(
        &self,
        file: usize,
        collapsed: bool,
        p: Palette,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let diff = &self.files[file].diff;
        let (dir, name) = split_path(&diff.change.path);
        let (added, removed) = diff.content.line_counts();
        div()
            .id(("header", file))
            .h(px(36.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .cursor_pointer()
            .hover(|style| style.bg(p.hover))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_collapsed(file, cx)))
            .child(
                icon(
                    if collapsed {
                        IconName::ChevronRight
                    } else {
                        IconName::ChevronDown
                    },
                    14.,
                )
                .text_color(p.muted),
            )
            .child(status_letter(&diff.change.status, p))
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap_1p5()
                    .child(
                        div()
                            .flex_shrink_0()
                            .font_weight(FontWeight::MEDIUM)
                            .child(name.to_owned()),
                    )
                    .when_some(renamed_from(&diff.change.status), |path, from| {
                        path.child(
                            ellipsized(format!("from {from}"))
                                .text_xs()
                                .text_color(p.muted),
                        )
                    })
                    .when(!dir.is_empty(), |path| {
                        path.child(ellipsized(dir.to_owned()).text_xs().text_color(p.muted))
                    }),
            )
            // A file partly staged has a card for each part.
            .when(diff.change.staged, |header| {
                header.child(
                    div()
                        .flex_shrink_0()
                        .px_1p5()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(p.hairline)
                        .text_xs()
                        .text_color(p.muted)
                        .child("Staged"),
                )
            })
            .child(div().flex_1())
            .child(counts(added, removed, p))
            .child(stats_bar(added, removed, p))
    }
}

#[derive(Clone, Copy)]
enum Side {
    Old,
    New,
}

fn note(p: Palette) -> Div {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(p.muted)
}

/// What a file's card says in place of lines it has none to show.
fn note_for(file: &FileView) -> String {
    match (&file.diff.content, &file.diff.change.status) {
        (Content::Binary, _) => "Binary file".to_owned(),
        (Content::TooLarge { bytes }, _) => {
            format!(
                "Too large to show ({:.1} MB)",
                *bytes as f64 / f64::from(1 << 20)
            )
        }
        (Content::Unreadable { reason }, _) => format!("Couldn't be read: {reason}"),
        (_, Status::Renamed { .. }) => "Moved, unchanged".to_owned(),
        (_, Status::Added | Status::Untracked) => "Empty file".to_owned(),
        (_, Status::Deleted) => "Deleted empty file".to_owned(),
        _ => "Only its file mode changed".to_owned(),
    }
}

/// A path's folder and file name.
pub(super) fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn renamed_from(status: &Status) -> Option<&str> {
    match status {
        Status::Renamed { from } => Some(from),
        _ => None,
    }
}

fn status_letter(status: &Status, p: Palette) -> Div {
    let (letter, colour) = match status {
        Status::Added => ("A", p.added),
        Status::Deleted => ("D", p.removed),
        Status::Modified => ("M", p.modified),
        Status::Untracked => ("U", p.added),
        Status::Renamed { .. } => ("R", p.accent),
        Status::Conflicted => ("!", p.danger),
    };
    div()
        .w(px(12.))
        .flex_shrink_0()
        .text_xs()
        .font_family(theme::MONO)
        .font_weight(FontWeight::BOLD)
        .text_color(colour)
        .child(letter)
}

/// `+added −removed`, leaving out a side with none.
pub fn counts(added: u32, removed: u32, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .flex()
        .gap_1()
        .text_xs()
        .font_family(theme::MONO)
        .when(added > 0, |counts| {
            counts.child(div().text_color(p.added).child(format!("+{added}")))
        })
        .when(removed > 0, |counts| {
            counts.child(div().text_color(p.removed).child(format!("−{removed}")))
        })
}

/// `actions` over the end of a file list row, so they take no room from its name.
fn over_the_end(actions: Div) -> Div {
    actions.absolute().top_0().bottom_0().right_1()
}

/// A count in a pill: how many files a group holds.
fn badge(count: usize, p: Palette) -> Div {
    div()
        .flex_shrink_0()
        .min_w(px(18.))
        .h(px(18.))
        .px_1p5()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(p.accent)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(gpui_kit::white())
        .child(count.to_string())
}

/// Five blocks shared between added and removed lines, as a glance at the size of a change.
fn stats_bar(added: u32, removed: u32, p: Palette) -> Div {
    const BLOCKS: u32 = 5;
    let total = added + removed;
    // Rounded to the nearest block; a change with no lines fills none.
    let green = (added * BLOCKS + total / 2).checked_div(total);
    let red = green.map_or(0, |green| BLOCKS - green);
    let green = green.unwrap_or(0);
    div()
        .flex_shrink_0()
        .flex()
        .gap(px(2.))
        .children((0..BLOCKS).map(|block| {
            let colour = if block < green {
                p.added
            } else if block < green + red {
                p.removed
            } else {
                p.selected
            };
            div().size(px(7.)).rounded(px(1.5)).bg(colour)
        }))
}

/// The width of a line-number column wide enough for every line of `file`.
fn number_width(file: &FileView) -> f32 {
    let lines = match &file.diff.content {
        Content::Text { old, new, .. } => old.len().max(new.len()),
        _ => 0,
    };
    let digits = lines.max(1).ilog10() + 1;
    digits.max(2) as f32 * DIGIT_WIDTH + 12.
}

fn token_colour(token: Token, p: Palette) -> Hsla {
    match token {
        Token::Comment => p.code.comment,
        Token::Keyword => p.code.keyword,
        Token::String => p.code.string,
        Token::Constant => p.code.number,
        Token::Function => p.code.function,
        Token::Type => p.code.kind,
        Token::Tag => p.code.tag,
        Token::Attribute => p.code.attribute,
    }
}

/// How the diff's lines look in palette `p`, wrapped or not, with code `font_size` big.
/// Line `line` of `side` of `view`, numbered with `numbers`, coloured by its tokens and,
/// if it's `tint`ed as added or removed, with the words it changed marked.
fn half(
    view: &FileView,
    side: Side,
    line: u32,
    tint: Option<Hsla>,
    numbers: Vec<Option<u32>>,
    font: &Font,
    p: Palette,
) -> Half {
    let number_width = px(number_width(view));
    let Content::Text { old, new, .. } = &view.diff.content else {
        return Half::missing(number_width);
    };
    let colours = view.colours();
    let highlights = colours.and_then(|colours| colours.highlights.as_ref());
    let (lines, tokens, words) = match side {
        Side::Old => (
            old,
            highlights.map(|highlights| &highlights.old),
            colours.map(|colours| &colours.old_words),
        ),
        Side::New => (
            new,
            highlights.map(|highlights| &highlights.new),
            colours.map(|colours| &colours.new_words),
        ),
    };
    let text = lines.line(line as usize);
    let tokens =
        (tokens.and_then(|tokens| tokens.get(line as usize))).map_or(&[][..], Vec::as_slice);
    let words = (tint.and(words).and_then(|words| words.get(&line))).map_or(&[][..], Vec::as_slice);
    let colour = |kind: Option<Token>| kind.map_or(p.text, |kind| token_colour(kind, p));
    let marked = tint.unwrap_or(p.text).alpha(0.3);
    Half {
        numbers,
        number_width,
        runs: runs(text.len(), tokens, words, colour, marked, font),
        text: SharedString::from(text.to_owned()),
        tint,
        missing: false,
    }
}

fn look(wrap: bool, p: Palette, font_size: Pixels) -> Look {
    Look {
        font: font(theme::MONO),
        font_size,
        line_height: px(LINE_HEIGHT),
        inset: px(12.),
        card: p.sunken,
        hairline: p.hairline,
        numbers: p.muted,
        missing: p.hover,
        wrap,
    }
}

#[cfg(test)]
mod tests {
    use colosso_git::{Change, FileDiff, Hunk, Lines};
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Point, TestAppContext, WindowBounds,
        WindowOptions, size,
    };

    use super::*;

    /// A Rust file of `len` lines with every seventh replaced, as an agent's edit might leave it.
    fn edited(path: &str, len: u32) -> FileDiff {
        let line = |n: u32, new: bool| {
            let name = if new { "updated" } else { "original" };
            format!("    let {name}_{n} = compute(input, {n}); // keeps the total in range\n")
        };
        let old: String = (0..len).map(|n| line(n, false)).collect();
        let new: String = (0..len).map(|n| line(n, n % 7 == 3)).collect();
        let hunks = (0..len)
            .filter(|n| n % 7 == 3)
            .map(|n| Hunk {
                old: n..n + 1,
                new: n..n + 1,
            })
            .collect();
        FileDiff {
            change: Change {
                path: path.into(),
                status: Status::Modified,
                staged: false,
            },
            content: Content::Text {
                old: Lines::new(old),
                new: Lines::new(new),
                hunks,
            },
        }
    }

    #[test]
    fn staged_and_unstaged_files_are_grouped_apart() {
        let file = |path: &str, staged| {
            let mut diff = edited(path, 10);
            diff.change.staged = staged;
            Arc::new(FileView::new(diff))
        };
        let files = [
            file("src/a.rs", true),
            file("src/a.rs", false),
            file("b.rs", false),
        ];
        // Closing a folder in one group leaves the same folder in the other open.
        let closed = HashSet::from([(false, "src".to_owned())]);
        let rows: Vec<String> = (file_entries(&files, true, &closed).iter())
            .map(|entry| match entry {
                Entry::Folder { name, depth, .. } => format!("{depth} {name}/"),
                Entry::File { index, depth } => format!("{depth} #{index}"),
            })
            .collect();
        assert_eq!(
            rows,
            [
                "0 Staged/",
                "1 src/",
                "2 #0",
                "0 Unstaged/",
                "1 src/",
                "1 #2"
            ]
        );
    }

    /// A panel in a window of its own, showing `count` edited files.
    fn open(count: usize, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<ChangesPanel>) {
        let files = (0..count)
            .map(|n| edited(&format!("crates/app/src/module_{n:03}.rs"), 400))
            .collect();
        open_with(files, cx)
    }

    /// A panel in a window of its own, showing `files`.
    fn open_with(
        files: Vec<FileDiff>,
        cx: &mut TestAppContext,
    ) -> (AnyWindowHandle, Entity<ChangesPanel>) {
        cx.update(gpui_kit::init);
        cx.update(settings::init);
        cx.update(|cx| {
            let bounds = Bounds {
                origin: Point::default(),
                size: size(px(960.), px(1100.)),
            };
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..WindowOptions::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| {
                let changes = cx.new(|cx| Changes::read_as(files, window, cx));
                cx.new(|cx| ChangesPanel::new(changes, cx))
            })
            .unwrap()
        })
    }

    #[gpui_kit::test]
    fn a_folder_or_group_acts_on_the_files_under_it(cx: &mut TestAppContext) {
        let file = |path: &str, staged| {
            let mut diff = edited(path, 10);
            diff.change.staged = staged;
            diff
        };
        let files = vec![
            file("src/a.rs", true),
            file("src/a.rs", false),
            file("src/b.rs", false),
            file("c.rs", false),
        ];
        let (_, panel) = open_with(files, cx);
        panel.update(cx, |panel, _| {
            // Staged/, src/, a.rs, Unstaged/, src/, a.rs, b.rs, c.rs
            assert_eq!(panel.entry_files(0), [0]);
            assert_eq!(panel.entry_files(1), [0]);
            assert_eq!(panel.entry_files(3), [1, 2, 3]);
            assert_eq!(panel.entry_files(4), [1, 2]);
            assert_eq!(panel.entry_files(7), [3]);
            assert!(panel.entry_is_staged(1) && !panel.entry_is_staged(4));
        });
    }

    #[gpui_kit::test]
    fn hovering_the_file_list_shows_its_actions_without_breaking_a_frame(cx: &mut TestAppContext) {
        let (handle, _) = open(8, cx);
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };
        draw(cx);
        // Down the file list, row by row, onto each row and off it again.
        for y in (40..400).step_by(9) {
            cx.update_window(handle, |_, window, cx| {
                window.simulate_mouse_move(gpui_kit::point(px(120.), px(y as f32)), cx);
            })
            .unwrap();
            draw(cx);
        }
    }

    #[gpui_kit::test]
    fn a_file_picked_in_the_list_is_glided_to_and_stays_picked(cx: &mut TestAppContext) {
        let (handle, panel) = open(20, cx);
        let draw = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
                .unwrap();
        };
        draw(cx);
        panel.update(cx, |panel, cx| panel.show(12, cx));
        // Partway there, the file it's going to stays the one picked.
        draw(cx);
        panel.update(cx, |panel, _| {
            assert!(panel.glide.is_some());
            assert_ne!(panel.list.logical_scroll_top().item_ix, panel.headers[12]);
            assert_eq!(panel.current, Some(12));
        });
        panel.update(cx, |panel, _| {
            let glide = panel.glide.as_mut().unwrap();
            glide.start -= GLIDE;
        });
        draw(cx);
        panel.update(cx, |panel, _| {
            assert!(panel.glide.is_none());
            let top = panel.list.logical_scroll_top();
            assert_eq!(top.item_ix, panel.headers[12]);
            assert_eq!(top.offset_in_item, px(0.));
            assert_eq!(panel.current, Some(12));
        });
    }

    /// How long a frame takes to draw the diffs after jumping from file to file of a large
    /// diff, by arrow key, one way and back. Run with
    /// `cargo test --release -p colosso jumping -- --ignored --nocapture`.
    #[gpui_kit::test]
    #[ignore = "a measurement, not a check"]
    fn jumping_between_files(cx: &mut TestAppContext) {
        let (handle, panel) = open(200, cx);
        let mut frame = |step: &mut dyn FnMut(&mut ChangesPanel, &mut Context<ChangesPanel>)| {
            cx.update_window(handle, |_, window, cx| {
                panel.update(cx, |panel, cx| step(panel, cx));
                let started = Instant::now();
                window.draw(cx).clear(cx);
                started.elapsed()
            })
            .unwrap()
        };
        frame(&mut |_, _| {});
        let mut times: Vec<Duration> = Vec::new();
        for step in [1, -1] {
            for _ in 0..199 {
                times.push(frame(&mut |panel, cx| panel.step(step, true, cx)));
            }
        }
        times.sort();
        let at = |share: f64| times[((times.len() - 1) as f64 * share) as usize];
        eprintln!(
            "jump frames: median {:?}, 90th {:?}, worst {:?}, over {} jumps",
            at(0.5),
            at(0.9),
            at(1.),
            times.len()
        );
    }
}
