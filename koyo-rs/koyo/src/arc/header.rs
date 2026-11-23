//! A worktree's header in the Arc sidebar (`docs/arc-sidebar.md` 5), a label over its tabs: the
//! worktree's name, from where their icons start, and, while the pointer's on it, a pill under the
//! row and its changes', its files', a `…` and a `+` at its end, fading in and out, the name
//! making room for them only then; the changes' or the files' stays while its panel shows.
//! Clicked, it opens or collapses the worktree's rows, which slide as they go. Its changes' and
//! files' show the worktree's panel on them
//! (`right_panel::buttons`), its `…`, or a right-click, opens the worktree's menu, and its `+` a
//! new tab in it; dragged, it takes the worktree, rows and all, to another place among the
//! project's; and a tab from another worktree let go on it moves there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, FontWeight, InteractiveElement as _, MouseButton,
    MouseDownEvent, ParentElement as _, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::Look;
use super::actions::ClearWorktree;
use super::worktrees::{ROW_END, arc_row, takes_tabs};
use crate::launcher::NewTab;

use crate::model::BranchUnit;
use crate::pane::ROW_GLYPH;
use crate::projects::{DeleteWorktree, path_items};
use crate::settings;
use crate::shell::{InHand, List, Shell, ellipsized, icon, row_button};
use crate::theme;

/// How long the pill and the buttons take to fade in or out as the pointer comes onto the
/// header or leaves, as the sidebar's other hovers do.
const HOVER_FADE: Duration = Duration::from_millis(150);

/// The header of the worktree `unit`, the `branch`th of the project at `project`, known by
/// `key` as it moves.
pub(super) fn header(
    shell: &Shell,
    (project, branch, key): (usize, usize, u64),
    unit: &BranchUnit,
    look: Look,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Stateful<Div> {
    let name = unit.title(shell.own_prefix(project, cx));
    let (shown, hover) = theme::hover::eased(("arc-header", key), HOVER_FADE, window, cx);
    let lit = if shell.row_hand.holding() == Some(key) {
        1.
    } else {
        shown
    };
    let at = (project, branch);
    let hovered = shown > 0.;
    let panel = (shell.panel_buttons(at, (ROW_GLYPH, look.pill), hovered, cx).into_iter())
        .map(|(button, lit)| button.opacity(if lit { 1. } else { shown }));
    let ends = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(2.))
        .children(panel)
        .when(hovered, |ends| {
            ends.child(more_button(at, look, cx).opacity(shown))
                .child(plus_button(at, look, cx).opacity(shown))
        });
    let hand = shell.row_hand.clone();
    let header = arc_row(("arc-header", branch))
        .debug_selector(move || format!("arc-header-{branch}"))
        .pr(px(ROW_END))
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(look.label)
        .bg(look.row_hover.opacity(lit))
        .on_hover(hover)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_worktree(at, cx)))
        .on_mouse_down(MouseButton::Right, opens_menu(at, cx))
        .on_drag(InHand, move |_, grab, _, cx| {
            hand.take(List::Worktrees(project), key, branch, grab.y);
            cx.new(|_| InHand)
        })
        .child(ellipsized(name).flex_1())
        .child(ends);
    #[cfg(feature = "visual-check")]
    let header = {
        use gpui_kit::base::ElementExt as _;
        header.on_prepaint(crate::visual::noting(format!("arc-header-{branch}")))
    };
    takes_tabs(header, at, look, cx)
}

/// The header's `…`, which opens the worktree's menu, for the worktree at `at`.
fn more_button(at: (usize, usize), look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let branch = at.1;
    row_button(("arc-header-more", branch), look.p)
        .debug_selector(move || format!("arc-header-more-{branch}"))
        .child(icon(IconName::Ellipsis, ROW_GLYPH))
        .on_mouse_down(MouseButton::Left, opens_menu(at, cx))
}

/// The header's `+`, which selects the worktree at `at` and opens a new tab on it.
fn plus_button(at: (usize, usize), look: Look, cx: &mut Context<Shell>) -> Stateful<Div> {
    let branch = at.1;
    row_button(("arc-header-new-tab", branch), look.p)
        .debug_selector(move || format!("arc-header-new-tab-{branch}"))
        .child(icon(IconName::Plus, ROW_GLYPH))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.open_worktree(at, cx);
            }),
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTab), cx))
}

/// Opens the menu of the worktree at `at`, as `(project, branch)`, where the pointer pressed,
/// rather than any menu of what it's in: the header's `…` and a right-click on it open the one
/// menu alike.
fn opens_menu(
    (project, branch): (usize, usize),
    cx: &mut Context<Shell>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static {
    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
        cx.stop_propagation();
        let unit = (this.workspace.projects.get(project)).and_then(|it| it.branches.get(branch));
        if let Some(unit) = unit {
            Items::of(unit).menu().show(event.position, window, cx);
        }
    })
}

/// What a worktree's menu offers (`docs/arc-sidebar.md` 5): its own items, then its folder's.
pub(super) struct Items {
    path: PathBuf,
    tabs: usize,
    is_main: bool,
}

impl Items {
    pub(super) fn of(unit: &BranchUnit) -> Items {
        Items {
            path: unit.worktree.path.clone(),
            tabs: unit.tabs.len(),
            is_main: unit.is_main(),
        }
    }

    /// The worktree's own items, in order: what each says, whether it can be picked, and what it
    /// does. Clearing needs tabs to close, and git can't delete a main checkout.
    pub(super) fn own(&self) -> [(&'static str, bool, Box<dyn Action>); 2] {
        let clear = Box::new(ClearWorktree(self.path.clone()));
        let delete = Box::new(DeleteWorktree(self.path.clone()));
        [
            ("Clear Tabs…", self.tabs > 0, clear),
            ("Delete Worktree…", !self.is_main, delete),
        ]
    }

    /// The menu: the worktree's own items, then Reveal in Finder and Copy Path, as any folder's
    /// menu has them.
    fn menu(&self) -> NativeMenu {
        let own = (self.own().into_iter()).fold(NativeMenu::new(), |menu, (label, can, action)| {
            menu.menu_with_disabled(label, !can, action)
        });
        path_items(own.separator(), &self.path)
    }
}

impl Shell {
    /// The folders of the project at `project` and of its worktree at `branch`, if it has one.
    fn folders_at(&self, (project, branch): (usize, usize)) -> Option<(&Path, &Path)> {
        let folder = self.workspace.projects.get(project)?;
        Some((&folder.path, &folder.branches.get(branch)?.worktree.path))
    }

    /// Collapses the worktree at `at`, as `(project, branch)`, or opens it, as its header's
    /// clicked; kept for the next launch.
    fn toggle_worktree(&mut self, at: (usize, usize), cx: &mut Context<Self>) {
        if let Some((project, worktree)) = self.folders_at(at) {
            settings::update(cx, |settings| settings.toggle_collapsed(project, worktree));
        }
    }

    /// Selects the worktree at `at`, as `(project, branch)`, for a new tab, which shows: opened,
    /// if it was collapsed.
    fn open_worktree(&mut self, at: (usize, usize), cx: &mut Context<Self>) {
        let Some((project, worktree)) = self.folders_at(at) else {
            return;
        };
        if settings::get(cx).is_collapsed(project, worktree) {
            settings::update(cx, |settings| settings.toggle_collapsed(project, worktree));
        }
        self.workspace.select_branch(at.0, at.1);
        cx.notify();
    }
}
