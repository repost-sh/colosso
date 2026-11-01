//! The changes panel: the checkout's changed files, in a list; and each file's diff on a
//! card of its own, one under the other, in the workspace tab a file picked in the list
//! opens, scrolled to it ([`diffs`]).
//!
//! The diffs are one virtualised list of rows, so only what's on screen is laid out however
//! long the diff. A live update replaces only the rows that differ, so the list stays where
//! it was scrolled to while an agent writes. The panel holds both, so the list follows the
//! file the diffs are scrolled to.

mod diffs;

pub use diffs::ChangesTab;

use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colosso_git::{Change, Checkout as Git, Discarding, Status};
use gpui_kit::assets::IconName;
use gpui_kit::component::{Selectable as _, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, EventEmitter, FocusHandle, FontWeight,
    InteractiveElement as _, IntoElement, ListAlignment, ListOffset, ListScrollEvent, ListState,
    ParentElement as _, Render, ScrollStrategy, StatefulInteractiveElement as _, Styled,
    Subscription, UniformListScrollHandle, Window, div, px, uniform_list,
};

use super::discard;
use super::file_view::{FileView, Key};
use super::marks::Mark;
use super::rows::{Layout, Row, Shown, rows};
use super::tree::{self, entries};
use crate::checkout::{Checkout, GitAct};
use crate::modal::{self, spinner};
use crate::right_panel::{
    Activate, Collapse, Expand, NextFile, PageDown, PageUp, PreviousFile, SelectFirst, SelectLast,
    SelectNext, SelectPrevious, note, page, panel_header, title,
};
use crate::settings;
use crate::shell::{ellipsized, icon, icon_button, row};
use crate::theme::{self, Palette};

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

/// What the panel asks of the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChangesEvent {
    /// A file was picked in the list: show the diffs, at it.
    Picked,
}

pub struct ChangesPanel {
    checkout: Entity<Checkout>,
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
    _subscriptions: [Subscription; 2],
}

impl ChangesPanel {
    /// The panel showing `checkout`'s changes; the right panel has them coloured while it's shown.
    pub fn new(checkout: Entity<Checkout>, cx: &mut Context<Self>) -> Self {
        let _subscriptions = [
            settings::observe(cx),
            cx.observe(&checkout, |this, checkout, cx| {
                // Colouring files says so too; the same files only need drawing again.
                let files = checkout.read(cx).files();
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
        ];
        let list = ListState::new(0, ListAlignment::Top, px(400.));
        // Scrolling by hand takes over from a glide, and the keyboard follows to the file in
        // view. The list is mid-update when it calls this, so it's asked where it is after.
        list.set_scroll_handler(cx.listener(|this, _: &ListScrollEvent, window, cx| {
            this.glide = None;
            this.folder = None;
            cx.defer_in(window, |this, _, cx| this.follow_scroll(cx));
        }));
        let mut panel = ChangesPanel {
            checkout,
            files: Vec::new(),
            rows: Vec::new(),
            headers: Vec::new(),
            entries: Vec::new(),
            list,
            layout: Layout::default(),
            wrap: true,
            tree: true,
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

    /// The file list's, which the arrow keys move through.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Whose changes they are.
    #[cfg(test)]
    pub fn checkout(&self) -> &Entity<Checkout> {
        &self.checkout
    }

    /// Builds the rows again from the files, telling the list which rows differ.
    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let files = self.checkout.read(cx).files().to_vec();
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
        cx.emit(ChangesEvent::Picked);
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

    /// Moves the keyboard a page of entries down the file list, or up, stopping at its ends.
    fn page(&mut self, direction: isize, cx: &mut Context<Self>) {
        let by = direction * page(&self.files_scroll, FILE_ROW_HEIGHT);
        if let Some(at) = crate::list::stepped(self.cursor(), by, self.entries.len()) {
            self.select(at, cx);
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

impl EventEmitter<ChangesEvent> for ChangesPanel {}

impl Render for ChangesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let body = (self.state(p, cx)).unwrap_or_else(|| {
            let files = self.file_list(p, cx);
            files.flex_1().min_h_0().w_full().into_any_element()
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.header(p, cx))
            .child(body)
    }
}

impl ChangesPanel {
    /// What shows in place of the files while there are none to show: the first read under
    /// way, what kept it from reading them, or that there are no changes.
    fn state(&self, p: Palette, cx: &App) -> Option<AnyElement> {
        let checkout = self.checkout.read(cx);
        let state = if checkout.is_loading() {
            div().flex_1().relative().child(spinner(p.muted))
        } else if let Some(error) = checkout.error()
            && self.files.is_empty()
        {
            note(p).text_color(p.danger).child(error.to_owned())
        } else if self.files.is_empty() {
            (note(p).gap_2())
                .child(icon(IconName::CircleCheck, 14.))
                .child("No changes")
        } else {
            return None;
        };
        Some(state.into_any_element())
    }
}

impl ChangesPanel {
    fn header(&self, p: Palette, cx: &mut Context<Self>) -> Div {
        let toggle = |id: &'static str, glyph: IconName, tip: &'static str, on: bool| {
            icon_button(id, glyph, tip, cx).xsmall().selected(on)
        };
        // The title says what's here; the file list's top row, how big.
        panel_header(p)
            .child(div().flex_1().child(title("Changes", p)))
            .when_some(self.checkout.read(cx).unwatched(), |header, reason| {
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
            .on_action(cx.listener(|this, _: &SelectFirst, _, cx| this.select(0, cx)))
            .on_action(cx.listener(|this, _: &SelectLast, _, cx| {
                this.select(this.entries.len().saturating_sub(1), cx);
            }))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1, cx)))
            .on_action(cx.listener(|this, _: &Expand, _, cx| this.open_or_close(Some(true), cx)))
            .on_action(cx.listener(|this, _: &Collapse, _, cx| this.open_or_close(Some(false), cx)))
            .on_action(cx.listener(|this, _: &Activate, _, cx| this.open_or_close(None, cx)))
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
                    .child(ellipsized(name.to_owned()))
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
            Act::Stage => (Git::stage, "Couldn't stage"),
            Act::Unstage => (Git::unstage, "Couldn't unstage"),
            Act::Discard => return discard::confirm(self.checkout.clone(), list, window, cx),
        };
        let done = (self.checkout).update(cx, |checkout, cx| checkout.act(run, list, cx));
        cx.spawn_in(window, async move |_, cx| {
            if let Err(error) = done.await {
                cx.update(|window, cx| modal::alert(failed, error.message, window, cx))
                    .ok();
            }
        })
        .detach();
    }

}

/// A path's folder and file name.
pub(super) fn split_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn status_letter(status: &Status, p: Palette) -> Div {
    let mark = Mark::of(status);
    let (letter, colour) = (mark.letter(), mark.colour(p));
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

#[cfg(test)]
mod tests;
