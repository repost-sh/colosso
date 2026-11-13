//! A worktree's header in the Arc sidebar (`docs/arc-sidebar.md` 5), a label over its tabs: the
//! worktree's name, from where their icons start, and, while the pointer's on it, a pill under the
//! row and a `…` and a `+` at its end, fading in and out.
//! Clicked, it opens or collapses the worktree's rows, which slide as they go. Its `…`,
//! or a right-click, opens the worktree's menu, and its `+` a new tab in it; dragged, it takes the
//! worktree, rows and all, to another place among the project's; and a tab from another worktree
//! let go on it moves there.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::native_menu::NativeMenu;
use gpui_kit::{
    Action, App, AppContext as _, Context, Div, FontWeight, InteractiveElement as _, MouseButton,
    MouseDownEvent, ParentElement as _, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use super::Look;
use super::actions::ClearWorktree;
use super::tiles::DraggedTile;
use super::worktrees::{GLYPH, arc_button, arc_row, list_of, takes_tabs};
use crate::launcher::NewTab;
use koyo_git::Head;

use crate::model::BranchUnit;
use crate::pane::tab_button;
use crate::projects::{DeleteWorktree, path_items};
use crate::settings;
use crate::shell::{Shell, ellipsized, icon};
use crate::theme;

/// How long the pill, the `…` and the `+` take to fade in or out as the pointer comes onto the
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
    let name = title(unit, shell.own_prefix(project, cx));
    let (shown, hover) = theme::hover::eased(("arc-header", key), HOVER_FADE, window, cx);
    // In hand, it's lit as it is under the pointer, wherever the pointer is.
    let lit = if shell.worktree_hand.holding() == Some(key) {
        1.
    } else {
        shown
    };
    let at = (project, branch);
    let more = arc_button(tab_button(("arc-header-more", branch), look.p))
        .debug_selector(move || format!("arc-header-more-{branch}"))
        .child(icon(IconName::Ellipsis, GLYPH))
        .on_mouse_down(MouseButton::Left, opens_menu(at, cx));
    // The launcher opens its tab on the selected worktree, so this one is selected, and opened,
    // on the press that opens it.
    let new_tab = arc_button(tab_button(("arc-header-new-tab", branch), look.p))
        .debug_selector(move || format!("arc-header-new-tab-{branch}"))
        .child(icon(IconName::Plus, GLYPH))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.open_worktree(at, cx);
            }),
        )
        .on_click(|_, window, cx| window.dispatch_action(Box::new(NewTab), cx));
    let ends = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(2.))
        .opacity(shown)
        .child(more)
        .child(new_tab);
    let hand = shell.worktree_hand.clone();
    let header = arc_row(("arc-header", branch))
        .debug_selector(move || format!("arc-header-{branch}"))
        .pr(px(6.))
        // Its name smaller and lighter than the tabs' titles, a label over them.
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(look.label)
        .bg(look.row_hover.opacity(lit))
        .on_hover(hover)
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_worktree(at, cx)))
        .on_mouse_down(MouseButton::Right, opens_menu(at, cx))
        .on_drag(DraggedTile, move |_, grab, _, cx| {
            hand.take(list_of(project), key, branch, grab.y);
            cx.new(|_| DraggedTile)
        })
        .child(ellipsized(name).flex_1())
        .child(ends);
    // The visual check points at a header where it's drawn.
    #[cfg(feature = "visual-check")]
    let header = {
        use gpui_kit::base::ElementExt as _;
        header.on_prepaint(crate::visual::noting(format!("arc-header-{branch}")))
    };
    takes_tabs(header, at, look, cx)
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

/// A worktree's name in words: its branch less `prefix`, the one the person's own branches carry,
/// its dashes and underscores spaces and its first letter a capital, so
/// `schulzfel/some-nasty-name` is "Some nasty name". A detached checkout keeps its name.
pub(super) fn title(unit: &BranchUnit, prefix: Option<&str>) -> SharedString {
    let Head::Branch(branch) = &unit.worktree.head else {
        return unit.name();
    };
    let own = prefix.and_then(|prefix| branch.strip_prefix(prefix)?.strip_prefix('/'));
    let mut words = own.unwrap_or(branch).replace(['-', '_'], " ");
    if let Some(first) = words.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    words.into()
}

impl Shell {
    /// The prefix the person's own branches carry in the project at `project`, which `title`
    /// leaves off: the one new worktrees' branches get (Settings → General).
    pub(super) fn own_prefix<'a>(&'a self, project: usize, cx: &'a App) -> Option<&'a str> {
        let user = self.workspace.projects.get(project)?.user.as_deref();
        settings::get(cx).branch_prefix.own(user)
    }

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
