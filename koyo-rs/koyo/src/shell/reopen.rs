//! ⌘⇧T: the tab closed last comes back where it was, a worktree's on its checkout and a loose
//! page among its space's, its pages opening again at the addresses they showed, its files on
//! the files they showed, as they are on disk, and the diffs' pane on the checkout's diffs.
//! And the editor panes the window brought back, which open what they showed the same way once
//! their tab is shown.

use std::path::{Path, PathBuf};

use gpui_kit::{AppContext as _, Context, Entity, Window};

use super::Shell;
use crate::files::FilePane;
use crate::model::{ClosedFrom, Loose, Pane, PaneKind, Spot, Tab, Unopened};
use crate::navigation::Route;

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
        let (closed, back) = self.workspace.take_closed(which)?;
        let project = match (&closed.from, back) {
            (ClosedFrom::Loose { project, .. }, _) => project.clone(),
            (ClosedFrom::Checkout(_), Spot::Checkout(project, ..)) => {
                self.workspace.projects[project].path.clone()
            }
            (ClosedFrom::Checkout(_), Spot::Pinned | Spot::Loose(_)) => return None,
        };
        let mut tab = closed.tab;
        for pane in tab.panes_mut() {
            if pane.kind == PaneKind::Desktop {
                pane.set_desktop(cx.new(crate::agent_desktop::DeskView::new));
            }
            if pane.kind == PaneKind::Device {
                let chosen = (closed.devices.iter())
                    .find(|(id, _)| *id == pane.id)
                    .map(|(_, chosen)| chosen.clone());
                let view = self.device_view(chosen, cx);
                pane.set_device(view, cx);
                continue;
            }
            if pane.kind != PaneKind::Browser {
                continue;
            }
            let address = (closed.addresses.iter())
                .find(|(id, _)| *id == pane.id)
                .map(|(_, address)| address.clone())
                .unwrap_or_default();
            let (view, by) = (self.browser_view(address, &project, window, cx), pane.by());
            view.update(cx, |view, _| view.made_by(by));
            pane.set_view(view);
        }
        let back = match back {
            Spot::Checkout(project, branch, at) => {
                let at = (project, branch, at);
                self.workspace.restore_tab(at, tab);
                self.reopen_files(at, &closed.files, window, cx);
                self.reopen_changes(at, &closed.changes, window, cx);
                Spot::Checkout(at.0, at.1, at.2)
            }
            Spot::Loose(_) | Spot::Pinned => {
                if let Spot::Loose(space) = back {
                    self.show_space_known_by(space, cx);
                }
                let page = Loose { tab, project };
                self.workspace.add_loose(page, closed.at, true);
                Spot::Loose(self.workspace.space_shown())
            }
        };
        self.navigate(Route::Workspace, cx);
        self.intend(crate::keyboard::Intent::Reopen);
        self.keys_to_shown_tab(cx);
        Some(back)
    }

    /// Opens what the shown tab's editor panes brought back with the window showed
    /// (`model/unopened.rs`): each its file, kept, as it is on disk, or the checkout's diffs.
    /// After each change of the shell, as a file and the diffs are made over the panels of
    /// their checkout, which is the selected one once the tab is shown.
    pub(crate) fn make_shown_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.workspace.shown_checkout_tab() else {
            return;
        };
        let panes = self.workspace.tab(at).map(Tab::panes).unwrap_or_default();
        let files: Vec<(u64, PathBuf)> = (panes.iter())
            .filter_map(|pane| Some((pane.id, pane.unopened_file()?.to_owned())))
            .collect();
        let changes: Vec<u64> = (panes.iter())
            .filter(|pane| pane.unopened.as_deref() == Some(&Unopened::Changes))
            .map(|pane| pane.id)
            .collect();
        self.reopen_files(at, &files, window, cx);
        self.reopen_changes(at, &changes, window, cx);
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
        let waits = |pane: &&Pane| pane.unopened_file() == Some(path);
        let at =
            (self.workspace.branch()?.tabs.iter()).position(|tab| tab.panes().iter().any(waits))?;
        self.workspace.select_tab(project, branch, at);
        self.make_shown_editors(window, cx);
        self.open_already(path, cx)
    }

    /// The file panes of the tab back at `at`, as `(project, branch, tab)`, which is shown,
    /// open the `files` they showed, by their panes' ids, kept.
    fn reopen_files(
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
            if let Some((_, file)) = opened.iter().find(|(id, _)| *id == pane.id) {
                pane.set_file(file.clone());
            }
        }
    }
}
