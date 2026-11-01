//! A file open in a workspace tab, or in a pane of one: over its editor where it is, its
//! folders from the checkout's root, and the editor over the checkout's document for it.
//! It may be a preview, its title in italics, which the next file previewed takes the place
//! of until it's kept: by a double click on its tab, by Enter in the tree, or by an edit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{
    Context, Entity, EventEmitter, IntoElement, ParentElement as _, Render, StyleRefinement,
    Styled as _, Subscription, Window, div,
};

use super::FileEditor;
use crate::checkout::Document;
use crate::theme;

pub struct FilePane {
    /// Its absolute path.
    pub path: PathBuf,
    /// Its path as the tree names it; `None` for a file outside the checkout.
    pub relative: Option<Arc<str>>,
    /// The checkout's document for it, held open by the pane: the editor only points at it.
    _document: Entity<Document>,
    pub editor: Entity<FileEditor>,
    /// Whether the next file previewed takes its place.
    preview: bool,
    /// Whether its document has edits not yet saved, as it last said.
    dirty: bool,
    /// Drawn again as its document turns unsaved, or saved; and kept once it's typed into,
    /// so the next file previewed doesn't take its edits. Reads and the like change nothing
    /// its tab shows, so they draw nothing.
    _changes: Subscription,
}

/// What a file pane asks of the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilePaneEvent {
    /// A folder or the file's name was clicked over the editor: pick it in the tree.
    PickInTree(Arc<str>),
}

impl EventEmitter<FilePaneEvent> for FilePane {}

impl FilePane {
    pub fn new(
        path: PathBuf,
        relative: Option<Arc<str>>,
        document: Entity<Document>,
        editor: Entity<FileEditor>,
        preview: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let _changes = cx.observe(&document, |this, document, cx| {
            let dirty = document.read(cx).is_dirty();
            let preview = this.preview && !dirty;
            if (dirty, preview) != (this.dirty, this.preview) {
                (this.dirty, this.preview) = (dirty, preview);
                cx.notify();
            }
        });
        let dirty = document.read(cx).is_dirty();
        FilePane {
            path,
            relative,
            _document: document,
            editor,
            preview: preview && !dirty,
            dirty,
            _changes,
        }
    }

    /// Whether the next file previewed takes its place.
    pub fn is_preview(&self) -> bool {
        self.preview
    }

    /// Keeps it open when another file's previewed.
    pub fn keep(&mut self, cx: &mut Context<Self>) {
        if self.preview {
            self.preview = false;
            cx.notify();
        }
    }

    /// Whether its file has edits not yet saved.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Its file's name, as its tab shows it.
    pub fn name(&self) -> String {
        name(&self.path)
    }
}

impl Render for FilePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        // The editor is drawn again only when it changes, not each time the shell is.
        let editor = (self.editor.clone())
            .cached(StyleRefinement::default().flex_1().min_w_0().min_h_0());
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.breadcrumb(p, cx))
            .child(editor)
    }
}

/// A file's name, as its tab shows it: the whole path when it has none.
pub fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
