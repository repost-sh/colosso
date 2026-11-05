//! A file open in a workspace tab, or in a pane of one: over it where it is, its folders from
//! the checkout's root; under that, its text in an editor over the checkout's document for
//! it, or, for a picture, a drawing or a PDF, the file shown as what it is. A drawing's text
//! is a click away, over the same bar.
//! It may be a preview, its title in italics, which the next file previewed takes the place
//! of until it's kept: by a double click on its tab, by Enter in the tree, or by an edit.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::{
    AnyView, App, Context, Entity, EventEmitter, Focusable as _, IntoElement, ParentElement as _,
    Render, StyleRefinement, Styled as _, Subscription, Window, div,
};

use super::FileEditor;
use crate::checkout::Document;
use crate::theme;
use crate::viewer::Viewer;

pub struct FilePane {
    /// Its absolute path.
    pub path: PathBuf,
    /// Its path as the tree names it; `None` for a file outside the checkout.
    pub relative: Option<Arc<str>>,
    body: Body,
    /// Whether the next file previewed takes its place.
    preview: bool,
    /// Whether its document has edits not yet saved, as it last said.
    dirty: bool,
}

/// What a file pane shows: its text, or the file as what it is, with a drawing's text beside
/// it once that's asked for.
enum Body {
    Text(Text),
    Viewer {
        viewer: Entity<Viewer>,
        /// A drawing's text, made the first time it's asked for and kept.
        source: Option<Text>,
        /// Whether that text is what's shown, or asked to be.
        showing_source: bool,
    },
}

/// What a file pane has on screen.
enum Shown<'a> {
    Text(&'a Entity<FileEditor>),
    Viewer(&'a Entity<Viewer>),
}

/// A file's text: the checkout's document for it, held open by the pane, the editor only
/// pointing at it, and what follows its edits.
struct Text {
    _document: Entity<Document>,
    editor: Entity<FileEditor>,
    /// Drawn again as its document turns unsaved, or saved; and kept once it's typed into,
    /// so the next file previewed doesn't take its edits. Reads and the like change nothing
    /// its tab shows, so they draw nothing.
    _changes: Subscription,
}

impl Text {
    fn new(document: Entity<Document>, editor: Entity<FileEditor>, cx: &mut Context<FilePane>) -> Text {
        let _changes = cx.observe(&document, |this, document, cx| {
            let dirty = document.read(cx).is_dirty();
            let preview = this.preview && !dirty;
            if (dirty, preview) != (this.dirty, this.preview) {
                (this.dirty, this.preview) = (dirty, preview);
                cx.notify();
            }
        });
        Text {
            _document: document,
            editor,
            _changes,
        }
    }
}

/// What a file pane asks of the shell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilePaneEvent {
    /// A folder or the file's name was clicked over the editor: pick it in the tree.
    PickInTree(Arc<str>),
    /// A drawing's source was asked for and it has no editor yet: make it one.
    WantsText,
}

impl EventEmitter<FilePaneEvent> for FilePane {}

impl FilePane {
    /// A file shown as text, in `editor` over `document`.
    pub fn with_text(
        path: PathBuf,
        relative: Option<Arc<str>>,
        document: Entity<Document>,
        editor: Entity<FileEditor>,
        preview: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let dirty = document.read(cx).is_dirty();
        FilePane {
            path,
            relative,
            body: Body::Text(Text::new(document, editor, cx)),
            preview: preview && !dirty,
            dirty,
        }
    }

    /// A file shown as what it is, by `viewer`.
    pub fn with_viewer(
        path: PathBuf,
        relative: Option<Arc<str>>,
        viewer: Entity<Viewer>,
        preview: bool,
    ) -> Self {
        FilePane {
            path,
            relative,
            body: Body::Viewer {
                viewer,
                source: None,
                showing_source: false,
            },
            preview,
            dirty: false,
        }
    }

    /// Gives a drawing its text, in `editor` over `document`, shown from now on.
    pub fn set_text(
        &mut self,
        document: Entity<Document>,
        editor: Entity<FileEditor>,
        cx: &mut Context<Self>,
    ) {
        self.dirty = document.read(cx).is_dirty();
        self.preview = self.preview && !self.dirty;
        let text = Text::new(document, editor, cx);
        if let Body::Viewer { source, showing_source, .. } = &mut self.body {
            (*source, *showing_source) = (Some(text), true);
        }
        cx.notify();
    }

    /// The editor over its text, if it has one; for a drawing, whether or not it's shown.
    pub fn editor(&self) -> Option<&Entity<FileEditor>> {
        match &self.body {
            Body::Text(text) | Body::Viewer { source: Some(text), .. } => Some(&text.editor),
            Body::Viewer { source: None, .. } => None,
        }
    }

    /// The editor over its text, when that has edits not yet saved.
    pub fn unsaved(&self) -> Option<&Entity<FileEditor>> {
        self.editor().filter(|_| self.dirty)
    }

    /// What shows the file as what it is, for a picture, a drawing or a document.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn viewer(&self) -> Option<&Entity<Viewer>> {
        match &self.body {
            Body::Viewer { viewer, .. } => Some(viewer),
            Body::Text(_) => None,
        }
    }

    /// What's shown: its text, or the file as what it is, which a drawing stays until the
    /// editor over its text is made.
    fn shown(&self) -> Shown<'_> {
        match &self.body {
            Body::Text(text)
            | Body::Viewer {
                source: Some(text),
                showing_source: true,
                ..
            } => Shown::Text(&text.editor),
            Body::Viewer { viewer, .. } => Shown::Viewer(viewer),
        }
    }

    /// Whether its text is what's shown.
    pub fn shows_text(&self) -> bool {
        matches!(self.shown(), Shown::Text(_))
    }

    /// Shows a drawing's text, or the drawing again; the shell is asked for an editor the
    /// first time its text is shown. The keyboard goes to what's shown.
    pub fn show_text(&mut self, text: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Body::Viewer { source, showing_source, .. } = &mut self.body else {
            return;
        };
        if text && !crate::viewer::has_text(&self.path) {
            return;
        }
        if text && source.is_none() {
            cx.emit(FilePaneEvent::WantsText);
        }
        *showing_source = text;
        self.focus(window, cx);
        cx.notify();
    }

    /// Gives the keyboard to what's shown: the editor or the viewer.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        match self.shown() {
            Shown::Text(editor) => editor.update(cx, |editor, cx| editor.focus_editor(window, cx)),
            Shown::Viewer(viewer) => {
                let focus = viewer.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
        }
    }

    /// Whether what it shows has the keyboard.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        match &self.body {
            Body::Text(text) => text.editor.read(cx).has_focus(window, cx),
            Body::Viewer { viewer, source, .. } => {
                (source.as_ref()).is_some_and(|text| text.editor.read(cx).has_focus(window, cx))
                    || viewer.read(cx).focus_handle(cx).contains_focused(window, cx)
            }
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
        let style = StyleRefinement::default().flex_1().min_w_0().min_h_0();
        // What's shown is drawn again only when it changes, not each time the shell is.
        let shown = match self.shown() {
            Shown::Text(editor) => AnyView::from(editor.clone()),
            Shown::Viewer(viewer) => AnyView::from(viewer.clone()),
        };
        let shown = shown.cached(style);
        div()
            .size_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.breadcrumb(p, cx))
            .child(shown)
    }
}

/// A file's name, as its tab shows it: the whole path when it has none.
pub fn name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
