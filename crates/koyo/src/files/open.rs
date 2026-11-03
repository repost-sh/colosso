//! Files opened in the workspace's tabs: from the tree, the palettes, a definition or a
//! list of places. A file open already is shown where it is. Otherwise it opens in a tab
//! right after the one shown, or, previewed, in place of the checkout's preview, which the
//! next file previewed takes the place of until it's kept.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{App, AppContext as _, Context, Entity, Window};

use super::file_pane::{FilePane, FilePaneEvent};
use super::panel::FilesEvent;
use super::{FileEditor, FilesPanel, Place};
use crate::checkout::Checkout;
use crate::go_to_file;
use crate::language::{self, Found};
use crate::model::{Pane, Tab};
use crate::navigation::Route;
use crate::right_panel::View;
use crate::settings;
use crate::shell::Shell;

impl Shell {
    /// Shows the selected checkout's file `place` is in, in a tab of its own, puts the
    /// cursor where the place says, and gives its editor the keyboard; the file is noted
    /// as opened, for ⌘P.
    pub fn view_file(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(checkout) = self.selected_checkout(window, cx) {
            go_to_file::opened(&checkout.read(cx).root().clone(), &place.path, cx);
        }
        self.open_file(place, true, true, window, cx);
    }

    /// Shows the file `place` is in as the preview, unless it's open already or previews
    /// are turned off, when it's in a tab of its own; and gives its editor the keyboard.
    pub fn preview_file(&mut self, place: Place, window: &mut Window, cx: &mut Context<Self>) {
        self.open_file(place, false, true, window, cx);
    }

    /// Shows the file `place` is in on the selected checkout, kept open if `kept` or if
    /// previews are turned off; `focus` says whether its editor takes the keyboard, which
    /// the tree and the lists keep, to show what's moved over.
    pub(crate) fn open_file(
        &mut self,
        place: Place,
        kept: bool,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(checkout) = self.selected_checkout(window, cx) else {
            log::warn!("no checkout is selected to open {} on", place.path.display());
            return;
        };
        let kept = kept || !settings::get(cx).preview_files;
        let path = checkout.read(cx).root().join(&place.path);
        let file = if let Some((at, file)) = self.open_already(&path, cx) {
            if kept {
                file.update(cx, FilePane::keep);
            }
            self.workspace.select_tab(at.0, at.1, at.2);
            file
        } else {
            let file = self.file_pane(path, &checkout, !kept, window, cx);
            let preview = (!kept).then(|| self.preview_pane(cx)).flatten();
            match preview {
                Some(id) => self.replace_pane_file(id, &file, cx),
                None => self.workspace.open_beside_shown(Tab::Single(Pane::file(file.clone(), cx))),
            }
            file
        };
        let editor = file.read(cx).editor.clone();
        editor.update(cx, |editor, cx| {
            if let Some(at) = place.at {
                editor.reveal(at, cx);
            }
            if focus {
                editor.focus_editor(window, cx);
            }
        });
        // A file opened is something to look at, so leave settings for it.
        self.navigate(Route::Workspace, cx);
        cx.notify();
    }

    /// The selected checkout, made now if the shell hasn't followed the selection to it yet.
    fn selected_checkout(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<Checkout>> {
        if self.right_panel.checkout().is_none() {
            self.follow_selection(window, cx);
        }
        self.right_panel.checkout().cloned()
    }

    /// The tab on the selected checkout showing the file at `path`, as `(project, branch,
    /// tab)`, and the pane showing it.
    fn open_already(&self, path: &Path, cx: &App) -> Option<((usize, usize, usize), Entity<FilePane>)> {
        let (project, branch, _) = self.workspace.shown_tab()?;
        let unit = self.workspace.branch()?;
        unit.tabs.iter().enumerate().find_map(|(at, tab)| {
            let file = tab.files().into_iter().find(|file| file.read(cx).path == path)?;
            Some(((project, branch, at), file.clone()))
        })
    }

    /// The selected checkout's preview, by its pane's id.
    fn preview_pane(&self, cx: &App) -> Option<u64> {
        let unit = self.workspace.branch()?;
        unit.tabs.iter().flat_map(Tab::panes).find_map(|pane| {
            let file = pane.file.as_ref()?;
            file.read(cx).is_preview().then_some(pane.id)
        })
    }

    /// Pane `id` on the selected checkout shows `file` in place of what it showed, and its
    /// tab is shown.
    fn replace_pane_file(&mut self, id: u64, file: &Entity<FilePane>, cx: &App) {
        let title = file.read(cx).name();
        let Some(unit) = self.workspace.branch_mut() else {
            return;
        };
        let Some(pane) = unit.pane_mut(id) else {
            return;
        };
        pane.file = Some(file.clone());
        pane.title = title.into();
        unit.active_tab = unit.tab_with(id).unwrap_or(unit.active_tab);
    }

    /// The file at `path` of `checkout` in a pane, a preview if `preview`: its editor's
    /// definitions in other files previewed, and the places a symbol in it leads to listed
    /// under the tabs.
    pub(crate) fn file_pane(
        &mut self,
        path: PathBuf,
        checkout: &Entity<Checkout>,
        preview: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<FilePane> {
        let root = checkout.read(cx).root().clone();
        let document = checkout.update(cx, |checkout, cx| checkout.document(&path, cx));
        let relative = (path.strip_prefix(&root).ok())
            .and_then(Path::to_str)
            .map(Arc::from);
        // A file of the checkout is edited in place; one outside it is only read.
        let editable = relative.is_some();
        let editor = cx.new(|cx| FileEditor::new(&document, editable, window, cx));
        language::attach(checkout, &editor, cx);
        cx.subscribe_in(&editor, window, |this, _, place: &Place, window, cx| {
            this.preview_file(place.clone(), window, cx);
        })
        .detach();
        cx.subscribe_in(&editor, window, |this, _, found: &Found, window, cx| {
            this.show_locations(found.clone(), window, cx);
        })
        .detach();
        let file = cx.new(|cx| FilePane::new(path, relative, document, editor, preview, cx));
        cx.subscribe_in(&file, window, Self::file_pane_event).detach();
        // Its tab shows whether it's a preview and whether it has unsaved edits.
        cx.observe(&file, |_, _, cx| cx.notify()).detach();
        file
    }

    fn file_pane_event(
        &mut self,
        _: &Entity<FilePane>,
        event: &FilePaneEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            FilePaneEvent::PickInTree(path) => {
                self.show(View::Files, cx);
                // Made now rather than after this change, to pick the path in.
                self.follow_selection(window, cx);
                if let Some(files) = self.right_panel.files().cloned() {
                    files.update(cx, |files, cx| files.pick_in_tree(path, window, cx));
                }
            }
        }
    }

    /// What the tree asks: a file opened, kept or previewed, the keyboard staying in the
    /// tree to go on moving through it.
    pub(crate) fn files_event(
        &mut self,
        _: &Entity<FilesPanel>,
        event: &FilesEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let FilesEvent::Open { place, kept } = event;
        self.open_file(place.clone(), *kept, false, window, cx);
    }

    /// Keeps the files of the tab at `at`, as `(project, branch, tab)`, open when another
    /// file's previewed.
    pub fn keep_tab(&mut self, at: (usize, usize, usize), cx: &mut Context<Self>) {
        let files: Vec<_> = (self.workspace.tab(at))
            .map(|tab| tab.files().into_iter().cloned().collect())
            .unwrap_or_default();
        for file in files {
            file.update(cx, FilePane::keep);
        }
    }

    /// The file shown: in the shown tab, the one whose editor has the keyboard, or else
    /// its first.
    pub fn shown_file(&self, window: &Window, cx: &App) -> Option<Entity<FilePane>> {
        let unit = self.workspace.branch()?;
        let files = unit.tabs.get(unit.active_tab)?.files();
        let focused = files.iter().find(|file| {
            let editor = file.read(cx).editor.read(cx);
            editor.has_focus(window, cx)
        });
        focused.or(files.first()).map(|file| (*file).clone())
    }

    /// The files open in the window's tabs with unsaved edits, on every checkout, by their
    /// editors.
    pub fn unsaved(&self, cx: &App) -> Vec<Entity<FileEditor>> {
        (self.workspace.projects.iter())
            .flat_map(|project| &project.branches)
            .flat_map(|unit| &unit.tabs)
            .flat_map(|tab| tab.unsaved(cx))
            .collect()
    }

    /// The first file the shown tab shows.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn viewing(&self) -> Option<Entity<FilePane>> {
        let unit = self.workspace.branch()?;
        unit.tabs.get(unit.active_tab)?.files().first().map(|file| (*file).clone())
    }

    /// Lets go of every file open on the checkout at `root`, whatever it has unsaved: their
    /// tabs close.
    #[cfg(test)]
    pub fn forget_files(&mut self, root: &Path, cx: &mut Context<Self>) {
        loop {
            let found = self.workspace.find_pane(|pane| {
                (pane.file.as_ref()).is_some_and(|file| file.read(cx).path.starts_with(root))
            });
            let Some((at, _)) = found else {
                return;
            };
            self.close_tab_now(at, cx);
        }
    }
}

#[cfg(test)]
mod tests;
