//! ⌃⇥ lists the files open in the Files panel, the latest shown first, in a palette where
//! ⌘P's opens; each ⌃⇥ moves the pick on and ⌃⇧⇥ back, round the ends, as the arrows do,
//! and letting go of ⌃ shows the file picked, as Enter or a click does.

use std::ops::Range;
use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Context, Entity, EntityId, FocusHandle,
    InteractiveElement as _, IntoElement, ModifiersChangedEvent, ParentElement as _, Render,
    ScrollStrategy, SharedString, StatefulInteractiveElement as _, Styled, UniformListScrollHandle,
    Window, div,
};

use super::panel::FilesPanel;
use super::tabs::{name, unsaved_dot};
use super::{ROW, SwitchTab, SwitchTabBack, file_icon, file_row};
use crate::list::{self, Activate, PageDown, PageUp, SelectNext, SelectPrevious};
use crate::modal;
use crate::theme::{self, Palette};

/// Opens the switcher over `files`\' tabs with the pick moved `by` rows from the file shown,
/// or moves the open switcher's pick `by` rows. With one file open there's nothing to switch
/// to.
pub(super) fn switch(files: Entity<FilesPanel>, by: isize, window: &mut Window, cx: &mut App) {
    if let Some(open) = modal::current::<Switcher>(cx) {
        open.update(cx, |switcher, cx| switcher.step(by, cx));
        return;
    }
    let rows = rows(files.read(cx), cx);
    if rows.len() < 2 {
        return;
    }
    let behind = window.window_handle();
    modal::open_list(rows.len(), window, cx, |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut switcher = Switcher {
            files,
            status: format!("{} files open, the latest shown first", rows.len()).into(),
            rows,
            selected: 0,
            behind,
            focus,
            scroll: UniformListScrollHandle::new(),
        };
        switcher.step(by, cx);
        switcher
    });
}

/// ⌃ let go in `window`, the one behind the switcher, before the switcher's own window had
/// the keyboard: shows the file picked and closes the switcher. It's done from here because
/// a window handling an event can't be reached from another.
pub(super) fn released(window: &mut Window, cx: &mut App) {
    let Some(switcher) = modal::current::<Switcher>(cx) else {
        return;
    };
    let (files, picked) = {
        let switcher = switcher.read(cx);
        (switcher.files.clone(), switcher.picked())
    };
    modal::close(cx);
    if let Some(key) = picked {
        files.update(cx, |files, cx| files.switch_to(key, window, cx));
    }
}

/// A tab as its row shows it.
struct Row {
    key: EntityId,
    icon: IconName,
    name: SharedString,
    folder: SharedString,
    pinned: bool,
    dirty: bool,
}

/// `files`\' tabs as rows, the latest shown first.
fn rows(files: &FilesPanel, cx: &App) -> Vec<Row> {
    let mut tabs: Vec<_> = files.tabs.iter().collect();
    tabs.sort_by_key(|tab| std::cmp::Reverse(tab.shown_at));
    (tabs.into_iter())
        .map(|tab| {
            let name = name(&tab.path);
            // The path as the tree names it, or whole outside the checkout: its folder.
            let shown = tab.relative.as_deref().map_or(tab.path.as_path(), Path::new);
            let folder = (shown.parent()).map_or(String::new(), |folder| folder.display().to_string());
            Row {
                key: tab.key,
                icon: file_icon(&name),
                name: name.into(),
                folder: folder.into(),
                pinned: tab.pinned,
                dirty: tab.document.read(cx).is_dirty(),
            }
        })
        .collect()
}

pub(super) struct Switcher {
    files: Entity<FilesPanel>,
    rows: Vec<Row>,
    selected: usize,
    /// How many files are open, under the list.
    status: SharedString,
    /// The window the switcher sits over, where the file picked is shown.
    behind: AnyWindowHandle,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl Switcher {
    /// The key of the tab picked.
    fn picked(&self) -> Option<EntityId> {
        self.rows.get(self.selected).map(|row| row.key)
    }

    /// Moves the pick `by` rows, round the ends.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let len = self.rows.len() as isize;
        self.select((self.selected as isize + by).rem_euclid(len) as usize, cx);
    }

    /// Moves the pick `by` pages, stopping at the ends.
    fn page(&mut self, by: isize, cx: &mut Context<Self>) {
        let by = by * list::page(&self.scroll, ROW);
        if let Some(at) = list::stepped(Some(self.selected), by, self.rows.len()) {
            self.select(at, cx);
        }
    }

    fn select(&mut self, at: usize, cx: &mut Context<Self>) {
        self.selected = at;
        self.scroll.scroll_to_item(at, ScrollStrategy::Nearest);
        cx.notify();
    }

    /// Shows the file picked in the window behind, with the keyboard, and closes the switcher.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.picked() {
            let files = &self.files;
            (self.behind.update(cx, |_, window, cx| {
                files.update(cx, |files, cx| files.switch_to(key, window, cx));
            }))
            .ok();
        }
        modal::dismiss(window, cx);
    }

    fn row(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        let name = div().when(!row.pinned, Styled::italic).child(row.name.clone());
        file_row(("tab-switcher-row", index), index == self.selected, row.icon, name, row.folder.clone(), p)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.selected = index;
                this.pick(window, cx);
            }))
            .when(row.dirty, |row| row.child(unsaved_dot(p)))
            .into_any_element()
    }
}

impl Render for Switcher {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let rows = cx.processor(move |this: &mut Self, range: Range<usize>, _, cx| {
            range.map(|index| this.row(index, p, cx)).collect()
        });
        let list = list::scrolling("tab-switcher", self.rows.len(), &self.scroll, rows);
        modal::palette(None, list, self.status.clone(), p)
            .track_focus(&self.focus)
            .key_context("TabSwitcher")
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1, cx)))
            .on_action(cx.listener(|this, _: &SwitchTab, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SwitchTabBack, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Activate, window, cx| this.pick(window, cx)))
            // Letting go of ⌃ shows the file picked.
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                if !event.modifiers.control {
                    this.pick(window, cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests;
