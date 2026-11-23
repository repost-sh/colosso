//! ⌘⇧T: the tab closed last comes back where it was, a worktree's on its checkout and a loose
//! page among its space's, each pane making what it showed again (`Shell::make_shown`): its
//! pages at the addresses they showed, its files as they are on disk, the diffs' pane on the
//! checkout's diffs. And the file panes the window or ⌘⇧T brought back, which open their files
//! once their tab is shown.

use std::path::{Path, PathBuf};

use gpui_kit::{Context, Entity, Window};

use super::Shell;
use crate::files::FilePane;
use crate::model::{ClosedFrom, ClosedTab, Content, Editor, EditorFile, Loose, Pane, Spot};

impl Shell {
    /// Opens the tab closed last again where it was, and shows it: a worktree's tab on its
    /// checkout at its place, a loose page among its space's at its place, its space shown.
    /// Where it went back, unless no tab was closed.
    pub(crate) fn reopen_closed_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Spot> {
        self.reopen_closed(|_| true, window, cx)
    }

    /// Opens the worktree's tab closed last again, as `reopen_closed_tab` does, as `(project,
    /// branch, tab)`: what app control's `tab_reopen` brings back. A loose page closed since is
    /// the person's, and stays closed for them to bring back.
    pub(crate) fn reopen_closed_checkout_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(usize, usize, usize)> {
        let checkout = |from: &ClosedFrom| matches!(from, ClosedFrom::Checkout(_));
        match self.reopen_closed(checkout, window, cx)? {
            Spot::Checkout(project, branch, at) => Some((project, branch, at)),
            Spot::Pinned | Spot::Loose(_) => None,
        }
    }

    fn reopen_closed(
        &mut self,
        which: impl Fn(&ClosedFrom) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Spot> {
        let (ClosedTab { from, at, tab }, back) = self.workspace.take_closed(which)?;
        let back = match (from, back) {
            (_, Spot::Checkout(project, branch, place)) => {
                self.workspace.restore_tab((project, branch, place), tab);
                Spot::Checkout(project, branch, place)
            }
            (ClosedFrom::Checkout(_), Spot::Pinned | Spot::Loose(_)) => return None,
            (ClosedFrom::Loose { project, .. }, back @ (Spot::Pinned | Spot::Loose(_))) => {
                if let Spot::Loose(space) = back
                    && !self.workspace.lists_loose_of(space)
                {
                    self.show_space_known_by(space, cx);
                }
                self.workspace.add_loose(Loose { tab, project }, at, true);
                Spot::Loose(self.workspace.space_shown())
            }
        };
        self.make_shown(window, cx);
        self.intend(crate::keyboard::Intent::Reopen);
        self.keys_to_shown_tab(cx);
        Some(back)
    }

    /// Shows the selected checkout's tab brought back with the window for the file at `path`,
    /// if one still waits to be shown, and opens the file in it: where it is, as `(project,
    /// branch, tab)`, and the file, so opening it again finds that tab rather than making
    /// another.
    pub(crate) fn open_brought_back(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<((usize, usize, usize), Entity<FilePane>)> {
        let (project, branch, _) = self.workspace.shown_tab()?;
        let waits = |pane: &&Pane| match &pane.content {
            Content::Editor(Editor::File(EditorFile::Unopened(waits))) => waits == path,
            _ => false,
        };
        let at =
            (self.workspace.branch()?.tabs.iter()).position(|tab| tab.panes().iter().any(waits))?;
        self.workspace.select_tab(project, branch, at);
        self.make_shown(window, cx);
        self.open_already(path, cx)
    }

    /// The file panes of the tab back at `at`, as `(project, branch, tab)`, which is shown,
    /// open the `files` they showed, by their panes' ids, kept.
    pub(super) fn reopen_files(
        &mut self,
        at: (usize, usize, usize),
        files: &[(u64, PathBuf)],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if files.is_empty() {
            return;
        }
        self.follow_selection(window, cx);
        let Some(checkout) = self.right_panel.checkout().cloned() else {
            return;
        };
        let opened: Vec<_> = (files.iter())
            .map(|(id, path)| {
                (
                    *id,
                    self.file_pane(path.clone(), &checkout, false, None, window, cx),
                )
            })
            .collect();
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        for pane in tab.panes_mut() {
            let opened = opened.iter().find(|(id, _)| *id == pane.id);
            if let (Some((_, file)), Content::Editor(editor)) = (opened, &mut pane.content) {
                *editor = Editor::File(EditorFile::Open(file.clone()));
            }
        }
    }
}
