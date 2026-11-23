//! What each checkout's panel shows: its changes or its files, open or closed. Each checkout
//! keeps its own, so going to another worktree finds that one's panel as it was left there. The
//! panel's actions act on the selected checkout's, and a worktree's buttons on that worktree's.

use std::path::{Path, PathBuf};

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{Context, Div, InteractiveElement as _};

use super::{RightPanel, ShowFiles, ToggleChanges, ToggleFiles, View};
use crate::navigation::Route;
use crate::shell::{Shell, ToggleRightPanel};

/// What a checkout's panel shows: the view it was last on, and whether it's open on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Side {
    pub open: bool,
    pub view: View,
}

impl RightPanel {
    /// What the panel of the checkout at `root` shows: closed on the changes until it's opened.
    pub(super) fn side(&self, root: &Path) -> Side {
        self.sides.get(root).copied().unwrap_or_default()
    }

    /// What the selected checkout's panel shows.
    pub(super) fn selected_side(&self) -> Side {
        (self.selected.as_deref()).map_or_else(Side::default, |root| self.side(root))
    }

    /// Whether the selected checkout's panel is open on `view`.
    pub fn shows(&self, view: View) -> bool {
        self.selected_side() == Side { open: true, view }
    }

    /// What the selected checkout's panel shows, while it's open.
    pub fn shown_view(&self) -> Option<View> {
        let side = self.selected_side();
        side.open.then_some(side.view)
    }

    /// Whether the checkout at `root` is the selected one, its panel open on `view`.
    pub fn shows_in(&self, root: &Path, view: View) -> bool {
        self.is_selected(root) && self.shows(view)
    }
}

impl Shell {
    /// What the window does with the right panel's actions and drags.
    pub fn handle_right_panel(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &ToggleRightPanel, _, cx| {
            let open = this.right_panel.selected_side().open;
            this.set_open(!open, None, cx);
        }))
        .on_action(cx.listener(|this, _: &ShowFiles, _, cx| this.show(View::Files, cx)))
        .on_action(cx.listener(|this, _: &ToggleChanges, _, cx| this.toggle(View::Changes, cx)))
        .on_action(cx.listener(|this, _: &ToggleFiles, _, cx| this.toggle(View::Files, cx)))
        .map(|root| Self::handle_go_to_file(root, cx))
        .map(|root| Self::handle_symbols(root, cx))
        .map(|root| Self::handle_files(root, cx))
        .on_drag_move(cx.listener(Self::resize_right_panel))
    }

    /// Opens or closes the panel of the checkout at `root`, on `view` or on what it was last
    /// on. Opened, the panel takes the keyboard.
    fn set_side(&mut self, root: PathBuf, open: bool, view: Option<View>, cx: &mut Context<Self>) {
        let panel = &mut self.right_panel;
        let side = panel.sides.entry(root).or_default();
        side.open = open;
        side.view = view.unwrap_or(side.view);
        panel.focus = open;
        cx.notify();
    }

    /// Opens or closes the selected checkout's panel, as `set_side` does.
    fn set_open(&mut self, open: bool, view: Option<View>, cx: &mut Context<Self>) {
        if let Some(root) = self.right_panel.selected.clone() {
            self.set_side(root, open, view, cx);
        }
    }

    /// Shows `view` in the selected checkout's panel, opening it if it's closed, and gives it
    /// the keyboard.
    pub(crate) fn show(&mut self, view: View, cx: &mut Context<Self>) {
        self.set_open(true, Some(view), cx);
        self.navigate(Route::Workspace, cx);
    }

    /// Closes the selected checkout's panel if it shows `view`, and shows `view` otherwise.
    fn toggle(&mut self, view: View, cx: &mut Context<Self>) {
        if self.right_panel.shows(view) {
            self.close_right_panel(cx);
        } else {
            self.show(view, cx);
        }
    }

    /// Closes the selected checkout's panel, whatever it shows.
    pub(crate) fn close_right_panel(&mut self, cx: &mut Context<Self>) {
        self.set_open(false, None, cx);
    }

    /// What a worktree's button for `view` does: on the selected worktree, what `toggle` does;
    /// on another, at `at`, as `(project, branch)`, shows `view` in its panel and selects it.
    pub(crate) fn toggle_in(
        &mut self,
        (project, branch): (usize, usize),
        view: View,
        cx: &mut Context<Self>,
    ) {
        let unit = (self.workspace.projects.get(project)).and_then(|it| it.branches.get(branch));
        let Some(root) = unit.map(|unit| unit.worktree.path.clone()) else {
            return;
        };
        if self.right_panel.is_selected(&root) {
            self.toggle(view, cx);
        } else {
            self.set_side(root, true, Some(view), cx);
            self.workspace.select_branch(project, branch);
            self.navigate(Route::Workspace, cx);
        }
    }
}
