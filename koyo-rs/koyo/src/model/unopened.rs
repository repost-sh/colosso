//! Editor panes brought back with the window (`docs/browser.md` 4): each knows the file it
//! showed, or that it showed its checkout's diffs, and opens it once its tab is shown, as a page
//! brought back opens its address then (`Unloaded`). A file or the diffs are made over their
//! checkout's panels, which only the selected checkout has.

use std::path::{Path, PathBuf};

use gpui_kit::{App, Entity};

use super::{Pane, PaneKind};
use crate::changes::ChangesTab;
use crate::files::{FilePane, file_name};

/// What an editor pane with nothing open yet shows once its tab is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unopened {
    /// The file at this path.
    File(PathBuf),
    /// Its checkout's diffs.
    Changes,
}

impl Pane {
    /// An editor pane that opens `unopened` once its tab is shown, titled as it will be then.
    pub fn unopened(unopened: Unopened) -> Self {
        let title = match &unopened {
            Unopened::File(path) => file_name(path),
            Unopened::Changes => "Changes".to_owned(),
        };
        Pane {
            title: title.into(),
            unopened: Some(Box::new(unopened)),
            ..Pane::new(PaneKind::Editor)
        }
    }

    /// Whether it shows its checkout's diffs, or will once its tab is shown.
    pub fn shows_changes(&self) -> bool {
        self.changes.is_some() || self.unopened.as_deref() == Some(&Unopened::Changes)
    }

    /// The file it opens once its tab is shown, if it was brought back for one.
    pub fn unopened_file(&self) -> Option<&Path> {
        match self.unopened.as_deref()? {
            Unopened::File(path) => Some(path),
            Unopened::Changes => None,
        }
    }

    /// The file an editor pane shows, or opens once its tab is shown.
    pub fn file_path(&self, cx: &App) -> Option<PathBuf> {
        match &self.file {
            Some(file) => Some(file.read(cx).path.clone()),
            None => self.unopened_file().map(Path::to_path_buf),
        }
    }

    /// The pane shows `file` from now on, and no longer waits to open anything.
    pub fn set_file(&mut self, file: Entity<FilePane>) {
        self.file = Some(file);
        self.unopened = None;
    }

    /// The pane shows `changes`, its checkout's diffs, from now on, and no longer waits to.
    pub fn set_changes(&mut self, changes: Entity<ChangesTab>) {
        self.changes = Some(changes);
        self.unopened = None;
    }
}

#[cfg(test)]
mod tests {
    use super::Unopened;
    use crate::model::{Pane, PaneKind};

    #[test]
    fn an_unopened_pane_is_titled_as_what_it_opens() {
        let file = Pane::unopened(Unopened::File("/src/app/src/main.rs".into()));
        assert_eq!((file.kind, file.title.as_ref()), (PaneKind::Editor, "main.rs"));
        assert!(file.file.is_none() && file.changes.is_none());
        assert_eq!(
            file.unopened_file(),
            Some("/src/app/src/main.rs".as_ref()),
            "what looks for the tab showing a file finds it"
        );
        assert!(!file.shows_changes());
        let changes = Pane::unopened(Unopened::Changes);
        assert_eq!(changes.title, "Changes");
        assert_eq!(changes.unopened.as_deref(), Some(&Unopened::Changes));
        assert!(changes.shows_changes() && changes.unopened_file().is_none());
    }
}
