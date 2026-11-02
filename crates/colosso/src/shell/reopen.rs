//! ⌘⇧T: the tab closed last comes back where it was, its pages opening again at the
//! addresses they showed, its files on the files they showed, as they are on disk, and the
//! diffs' pane on the checkout's diffs.

use std::path::PathBuf;

use gpui_kit::{Context, Window};

use super::Shell;
use crate::model::PaneKind;
use crate::navigation::Route;

impl Shell {
    /// Opens the tab closed last again, on its checkout at its place, and shows it: where
    /// it went back, as `(project, branch, tab)`, unless no tab was closed.
    pub(crate) fn reopen_closed_tab(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(usize, usize, usize)> {
        let (closed, at) = self.workspace.take_closed()?;
        let project = self.workspace.projects[at.0].path.clone();
        let mut tab = closed.tab;
        for pane in tab.panes_mut() {
            if pane.kind != PaneKind::Browser {
                continue;
            }
            let address = (closed.addresses.iter())
                .find(|(id, _)| *id == pane.id)
                .map(|(_, address)| address.clone())
                .unwrap_or_default();
            // Titled as it was until the page says otherwise.
            pane.browser = Some(self.browser_view(address, &project, window, cx));
        }
        self.workspace.restore_tab(at, tab);
        self.reopen_files(at, &closed.files, window, cx);
        self.reopen_changes(at, &closed.changes, window, cx);
        self.navigate(Route::Workspace, cx);
        cx.notify();
        Some(at)
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
        // The checkout it's on is selected now; its panels are made after this change.
        self.follow_selection(window, cx);
        let Some(checkout) = self.right_panel.checkout().cloned() else {
            return;
        };
        let opened: Vec<_> = (files.iter())
            .map(|(id, path)| (*id, self.file_pane(path.clone(), &checkout, false, window, cx)))
            .collect();
        let Some(tab) = self.workspace.tab_mut(at) else {
            return;
        };
        for pane in tab.panes_mut() {
            if let Some((_, file)) = opened.iter().find(|(id, _)| *id == pane.id) {
                pane.file = Some(file.clone());
            }
        }
    }
}
