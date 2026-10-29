//! A file shown beside the Files panel's tree: a view over the checkout's document for it,
//! shown in its language's colours and typed into when it's one of the checkout's, or a
//! note saying why it isn't shown. The view's text is the one that counts once it differs
//! from the document's, which is the file's: it tells the document so, and gives it the
//! text to save. The tab holds the document; the view only looks at it. The editor behind
//! the view is gpui-kit's, which is to be replaced; nothing outside this file knows it.

use std::ops::Range;
use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, RopeExt as _};
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, Styled as _, Subscription, Task, WeakEntity,
    Window, div,
};

use super::{At, Place};
use crate::checkout::{Content, Document, DocumentEvent};
use crate::language::{Attached, FindReferences, Found};
use crate::modal::spinner;
use crate::shell::icon;
use crate::theme;

/// Past this size a file's shown without colours: setting up its colours costs the main
/// thread too, 38 ms (44 at worst) to open a 1 MB Rust file against 13 ms for 1.5 MB plain.
const MOST_COLOURED: usize = 1 << 20;

pub struct FileEditor {
    document: WeakEntity<Document>,
    /// Whether the text is typed into: a file of the checkout is, one outside it is only
    /// read.
    editable: bool,
    /// The editor showing the document's text, once it's read.
    editor: Option<Shown>,
    /// The language server the file is given to, if one knows it.
    language: Option<Attached>,
    /// Where to put the cursor once the text is shown.
    reveal: Option<At>,
    /// Whether the editor takes the keyboard once the text is shown.
    focus_when_shown: bool,
    /// Looking up the places a symbol leads to, while it goes on.
    finding: Task<()>,
    _document: Subscription,
}

/// The editor showing the document's text.
struct Shown {
    editor: Entity<EditorState>,
    _changes: Subscription,
}

/// A definition in another file, for the Files panel to show.
impl EventEmitter<Place> for FileEditor {}

/// The places a symbol in the file led to, for the Files panel to list under it.
impl EventEmitter<Found> for FileEditor {}

impl FileEditor {
    /// A view of `document`, shown once it's read, and typed into if `editable`.
    pub fn new(
        document: &Entity<Document>,
        editable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let _document = cx.subscribe_in(document, window, |this, _, event, window, cx| {
            match event {
                DocumentEvent::Read { edits } => this.read(edits.as_deref(), window, cx),
                DocumentEvent::Saved => this.tell_dirty(cx),
            }
        });
        let mut view = FileEditor {
            document: document.downgrade(),
            editable,
            editor: None,
            language: None,
            reveal: None,
            focus_when_shown: false,
            finding: Task::ready(()),
            _document,
        };
        view.show(window, cx);
        view
    }

    /// The document it shows, while a tab holds it.
    pub fn document(&self) -> Option<Entity<Document>> {
        self.document.upgrade()
    }

    /// Gives the file to its language's server: the editor's hooks, and its diagnostics.
    pub fn attach(&mut self, mut attached: Attached, cx: &mut Context<Self>) {
        if let Some(shown) = &self.editor {
            attached.equip(&shown.editor, cx.entity().downgrade(), cx);
            attached.refresh(&shown.editor, cx);
        }
        self.language = Some(attached);
        cx.notify();
    }

    /// The server changed what it has for the file: its diagnostics, perhaps.
    pub fn language_changed(&mut self, cx: &mut Context<Self>) {
        if let (Some(attached), Some(shown)) = (&mut self.language, &self.editor) {
            attached.refresh(&shown.editor, cx);
        }
    }

    /// Whether the text shown is other than the file's: it's compared with the document's
    /// text, which is over once their lengths differ, as they do after most keystrokes.
    pub fn is_dirty(&self, cx: &App) -> bool {
        let (Some(shown), Some(document)) = (&self.editor, self.document.upgrade()) else {
            return false;
        };
        let Content::Text(content) = document.read(cx).content() else {
            return true;
        };
        shown.editor.read(cx).text() != content.as_str()
    }

    /// The editor's text changed, by the user or by a read of the file put into it: the
    /// server hears of either, and the document whether the text is now the file's.
    fn changed(&mut self, cx: &mut Context<Self>) {
        if let (Some(attached), Some(shown)) = (&self.language, &self.editor) {
            attached.changed(&shown.editor, cx);
        }
        self.tell_dirty(cx);
    }

    /// Tells the document whether the text shown is other than the file's.
    fn tell_dirty(&self, cx: &mut Context<Self>) {
        let dirty = self.is_dirty(cx);
        if let Some(document) = self.document.upgrade() {
            document.update(cx, |document, cx| document.set_dirty(dirty, cx));
        }
    }

    /// Writes the text shown to the file, when it's other than the file's and the file's
    /// the checkout's: done once it's written, or with why it couldn't be. Typed into
    /// meanwhile, the text stays unsaved.
    pub fn save(&mut self, cx: &mut Context<Self>) -> Task<Result<(), SharedString>> {
        let (Some(shown), Some(document)) = (&self.editor, self.document.upgrade()) else {
            return Task::ready(Ok(()));
        };
        if !self.editable || !self.is_dirty(cx) {
            return Task::ready(Ok(()));
        }
        let text = shown.editor.read(cx).value();
        document.update(cx, |document, cx| document.save(text, cx))
    }

    /// Gives the editor the keyboard, now if the text is shown, or once it is.
    pub fn focus_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match &self.editor {
            Some(shown) => shown.editor.update(cx, |editor, cx| editor.focus(window, cx)),
            None => self.focus_when_shown = true,
        }
    }

    /// Asks the server for every reference to the symbol at the caret, and has the panel
    /// list what it says.
    fn find_references(&mut self, _: &FindReferences, _: &mut Window, cx: &mut Context<Self>) {
        if let (Some(attached), Some(shown)) = (&self.language, &self.editor) {
            let found = attached.references(&shown.editor, cx);
            self.list(found, cx);
        }
    }

    /// Has the panel list what `found` brings, in place of any lookup still going on.
    pub fn list(&mut self, found: Task<Option<Found>>, cx: &mut Context<Self>) {
        self.finding = cx.spawn(async move |this, cx| {
            if let Some(found) = found.await {
                this.update(cx, |_, cx| cx.emit(found)).ok();
            }
        });
    }

    /// Puts the cursor `at` a place, now if the text is shown, or once it is.
    pub fn reveal(&mut self, at: At, cx: &mut Context<Self>) {
        self.reveal = Some(at);
        self.place(cx);
    }

    /// Picks the place to reveal; the editor scrolls a new selection into view as it lays
    /// it out, so this works on one not drawn yet.
    fn place(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = &self.editor else {
            return;
        };
        let Some(at) = self.reveal.take() else {
            return;
        };
        shown.editor.update(cx, |editor, cx| {
            let text = editor.text();
            let range = match at {
                At::Bytes(range) => range,
                At::Line(line) => {
                    let start = text.line_start_offset(line as usize);
                    start..start
                }
            };
            let end = range.end.min(text.len());
            editor.set_selected_range(range.start.min(end)..end, cx);
        });
    }

    /// The file was read again. As `edits` from the text it held, they go into the editor
    /// as such, so the cursor and the undo history keep their places; unless the text shown
    /// was other than the file's already, when it stays, unsaved. Otherwise it's shown over.
    fn read(&mut self, edits: Option<&[(Range<usize>, String)]>, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(shown), Some(edits)) = (&self.editor, edits) else {
            self.show(window, cx);
            return;
        };
        // What the document was told last, before it took the text read.
        let dirty = (self.document.upgrade()).is_some_and(|document| document.read(cx).is_dirty());
        if dirty {
            log::info!("a file read again under unsaved edits: they stay");
            self.tell_dirty(cx);
            return;
        }
        shown.editor.update(cx, |editor, cx| editor.edit(edits, window, cx));
    }

    /// Puts the document's text in a new editor, or the note saying why there's none.
    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(document) = self.document.upgrade() else {
            return;
        };
        let document = document.read(cx);
        let Content::Text(text) = document.content() else {
            self.editor = None;
            self.tell_dirty(cx);
            cx.notify();
            return;
        };
        let text = text.clone();
        let language = if text.len() > MOST_COLOURED {
            "text"
        } else {
            language(document.path(), &text)
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(language)
                .soft_wrap(false)
        });
        editor.update(cx, |editor, cx| editor.set_value(text, window, cx));
        if let Some(attached) = &mut self.language {
            attached.equip(&editor, cx.entity().downgrade(), cx);
            attached.refresh(&editor, cx);
        }
        let _changes = cx.subscribe(&editor, |this, _, event, cx| {
            if matches!(event, InputEvent::Change) {
                this.changed(cx);
            }
        });
        self.editor = Some(Shown { editor, _changes });
        self.tell_dirty(cx);
        self.place(cx);
        if std::mem::take(&mut self.focus_when_shown) {
            self.focus_editor(window, cx);
        }
        cx.notify();
    }

    /// The editor showing the text, once it's shown.
    #[cfg(any(test, feature = "visual-check"))]
    pub fn editor(&self) -> Option<&Entity<EditorState>> {
        self.editor.as_ref().map(|shown| &shown.editor)
    }

    /// The text it shows, if it's read and shown as text.
    #[cfg(test)]
    pub(super) fn text(&self, cx: &App) -> Option<SharedString> {
        Some(self.editor.as_ref()?.editor.read(cx).value())
    }
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let content = div()
            .size_full()
            .relative()
            .on_action(cx.listener(Self::find_references));
        if let Some(shown) = &self.editor {
            let editor = Editor::new(&shown.editor)
                .readonly(!self.editable)
                .appearance(false)
                .bordered(false)
                .h_full();
            return content.child(editor);
        }
        let document = self.document.upgrade();
        let note: SharedString = match document.as_ref().map(|document| document.read(cx).content()) {
            None | Some(Content::Reading | Content::Text(_)) => {
                return content.child(spinner(p.muted));
            }
            Some(Content::Binary) => "This file looks binary, so it isn't shown.".into(),
            Some(Content::TooLarge(bytes)) => format!(
                "This file is {:.1} MB, more than the {:.1} MB the editor opens.",
                megabytes(*bytes),
                megabytes(crate::checkout::MOST_BYTES)
            )
            .into(),
            Some(Content::Failed(why)) => format!("Couldn't read this file: {why}").into(),
        };
        content
            .flex()
            .items_center()
            .justify_center()
            .gap_2()
            .text_sm()
            .text_color(p.muted)
            .child(icon(IconName::FileX, 14.))
            .child(note)
    }
}

fn megabytes(bytes: u64) -> f64 {
    bytes as f64 / f64::from(1 << 20)
}

/// The highlighter's name for the language of the file at `path`, by its name, its
/// extension, or for a script without one, the program its first line runs.
pub fn language(path: &Path, text: &str) -> &'static str {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    match name {
        "Makefile" | "makefile" | "GNUmakefile" => return "make",
        "Cargo.lock" | "Pipfile" | "poetry.lock" => return "toml",
        ".bashrc" | ".zshrc" | ".profile" | ".bash_profile" | ".zprofile" | ".envrc" => {
            return "bash";
        }
        _ => {}
    }
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("rs") => "rust",
        Some("ts" | "mts" | "cts") => "typescript",
        Some("tsx") => "tsx",
        Some("js" | "mjs" | "cjs" | "jsx") => "javascript",
        Some("py" | "pyi") => "python",
        Some("go") => "go",
        Some("json" | "jsonc" | "json5") => "json",
        Some("toml") => "toml",
        Some("yaml" | "yml") => "yaml",
        Some("md" | "markdown" | "mdx") => "markdown",
        Some("css" | "scss") => "css",
        Some("html" | "htm") => "html",
        Some("sh" | "bash" | "zsh") => "bash",
        Some("c" | "h") => "c",
        Some("cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx") => "cpp",
        Some("java") => "java",
        Some("swift") => "swift",
        Some("kt" | "kts") => "kotlin",
        Some("rb") => "ruby",
        Some("zig") => "zig",
        Some("lua") => "lua",
        Some("mk") => "make",
        Some("proto") => "proto",
        Some("graphql" | "gql") => "graphql",
        Some("diff" | "patch") => "diff",
        Some(_) => "text",
        None => {
            let first = text.lines().next().unwrap_or("");
            let shell = ["sh", "bash", "zsh"].iter().any(|program| {
                first.starts_with("#!")
                    && (first.ends_with(&format!("/{program}"))
                        || first.ends_with(&format!(" {program}")))
            });
            if shell { "bash" } else { "text" }
        }
    }
}
