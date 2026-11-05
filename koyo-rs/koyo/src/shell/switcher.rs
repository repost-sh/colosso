//! ⌃⇥ lists the selected checkout's tabs, the latest shown first, in a palette where ⌘P's
//! opens: chats, terminals, pages and files alike, a split as one row naming its panes.
//! Each ⌃⇥ moves the pick on and ⌃⇧⇥ back, round the ends, as the arrows do, and letting
//! go of ⌃ shows the tab picked, as Enter or a click does.

use std::ops::Range;
use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyBinding, ModifiersChangedEvent, ParentElement as _, Render, ScrollStrategy,
    SharedString, StatefulInteractiveElement as _, Styled, UniformListScrollHandle, Window, div,
};

use super::tab_face::Face;
use super::{Shell, SwitchTab, SwitchTabBack};
use crate::files::{ROW, file_row};
use crate::list::{self, Activate, PageDown, PageUp, SelectNext, SelectPrevious};
use crate::model::Tab;
use crate::modal;
use crate::pane::unsaved_dot;
use crate::theme::{self, Palette};

/// The keys that move the switcher's pick, and Enter to show the tab picked.
pub(super) fn init(cx: &mut App) {
    list::bind_keys("TabSwitcher", cx);
    cx.bind_keys([KeyBinding::new("enter", Activate, Some("TabSwitcher"))]);
}

/// Opens the switcher over the selected checkout's tabs with the pick moved `by` rows from
/// the tab shown, or moves the open switcher's pick `by` rows. With one tab open there's
/// nothing to switch to.
pub(super) fn switch(
    shell: &Shell,
    entity: Entity<Shell>,
    by: isize,
    window: &mut Window,
    cx: &mut App,
) {
    if let Some(open) = modal::current::<Switcher>(cx) {
        open.update(cx, |switcher, cx| switcher.step(by, cx));
        return;
    }
    let rows = rows(shell, cx);
    if rows.len() < 2 {
        return;
    }
    let behind = window.window_handle();
    modal::open_list(rows.len(), window, cx, |window, cx| {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let mut switcher = Switcher {
            shell: entity,
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
/// the keyboard: shows the tab picked and closes the switcher. It's done from here because
/// a window handling an event can't be reached from another.
pub(super) fn released(window: &mut Window, cx: &mut App) {
    let Some(switcher) = modal::current::<Switcher>(cx) else {
        return;
    };
    let (shell, picked) = {
        let switcher = switcher.read(cx);
        (switcher.shell.clone(), switcher.picked())
    };
    modal::close(cx);
    if let Some(key) = picked {
        shell.update(cx, |shell, cx| shell.switch_to(key, window, cx));
    }
}

/// A tab as its row shows it.
struct Row {
    /// Its first pane's id, which finds it wherever it's moved.
    key: u64,
    icon: IconName,
    name: SharedString,
    /// For a file alone in its tab, the folder it's in.
    folder: SharedString,
    preview: bool,
    dirty: bool,
}

/// The selected checkout's tabs as rows, the latest shown first.
fn rows(shell: &Shell, cx: &App) -> Vec<Row> {
    let Some(unit) = shell.workspace.branch() else {
        return Vec::new();
    };
    let mut tabs: Vec<&Tab> = unit.tabs.iter().collect();
    tabs.sort_by_key(|tab| std::cmp::Reverse(tab.shown_at()));
    (tabs.into_iter())
        .map(|tab| {
            let faces = Face::all(tab, cx);
            let titles: Vec<&str> = faces.iter().map(|face| face.title.as_ref()).collect();
            let folder = match (tab, tab.files().as_slice()) {
                (Tab::Single(_), [file]) => {
                    let file = file.read(cx);
                    // The path as the tree names it, or whole outside the checkout: its folder.
                    let shown = file.relative.as_deref().map_or(file.path.as_path(), Path::new);
                    (shown.parent()).map_or(String::new(), |folder| folder.display().to_string())
                }
                _ => String::new(),
            };
            Row {
                key: tab.first().id,
                icon: faces.first().map_or(IconName::File, |face| face.icon),
                name: titles.join("  ·  ").into(),
                folder: folder.into(),
                preview: faces.len() == 1 && faces.iter().all(|face| face.preview),
                dirty: faces.iter().any(|face| face.dirty),
            }
        })
        .collect()
}

impl Shell {
    /// Shows the tab holding pane `key` on the selected checkout, if it's still open, and
    /// gives the keyboard to its file, or else to the window.
    fn switch_to(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(unit) = self.workspace.branch_mut() else {
            return;
        };
        let Some(at) = unit.tab_with(key) else {
            return;
        };
        unit.active_tab = at;
        match unit.tabs[at].files().first() {
            Some(file) => file.update(cx, |file, cx| file.focus(window, cx)),
            None => window.focus(&self.focus, cx),
        }
        cx.notify();
    }
}

pub(super) struct Switcher {
    shell: Entity<Shell>,
    rows: Vec<Row>,
    selected: usize,
    /// The window the switcher sits over, where the tab picked is shown.
    behind: AnyWindowHandle,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
}

impl Switcher {
    /// The key of the tab picked.
    fn picked(&self) -> Option<u64> {
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

    /// Shows the tab picked in the window behind, and closes the switcher.
    fn pick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(key) = self.picked() {
            let shell = &self.shell;
            (self.behind.update(cx, |_, window, cx| {
                shell.update(cx, |shell, cx| shell.switch_to(key, window, cx));
            }))
            .ok();
        }
        modal::dismiss(window, cx);
    }

    fn row(&self, index: usize, p: Palette, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.rows.get(index) else {
            return div().into_any_element();
        };
        let name = div().when(row.preview, Styled::italic).child(row.name.clone());
        let selected = index == self.selected;
        file_row(("tab-switcher-row", index), selected, row.icon, name, row.folder.clone(), p)
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
        modal::palette(None, list, None, p)
            .track_focus(&self.focus)
            .key_context("TabSwitcher")
            .on_action(cx.listener(|this, _: &SelectPrevious, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &SelectNext, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &PageUp, _, cx| this.page(-1, cx)))
            .on_action(cx.listener(|this, _: &PageDown, _, cx| this.page(1, cx)))
            .on_action(cx.listener(|this, _: &SwitchTab, _, cx| this.step(1, cx)))
            .on_action(cx.listener(|this, _: &SwitchTabBack, _, cx| this.step(-1, cx)))
            .on_action(cx.listener(|this, _: &Activate, window, cx| this.pick(window, cx)))
            // Letting go of ⌃ shows the tab picked.
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, window, cx| {
                if !event.modifiers.control {
                    this.pick(window, cx);
                }
            }))
    }
}

#[cfg(test)]
mod tests;
