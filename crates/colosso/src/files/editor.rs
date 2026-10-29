//! A file open in an editor tab, read-only for now: a view over the checkout's document for
//! it, shown in its language's colours, or a note saying why it isn't shown. The editor
//! behind it is gpui-kit's, which is to be replaced; nothing outside this file knows it.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Subscription, Window, div,
};

use crate::checkout::{Content, Document};
use crate::modal::spinner;
use crate::shell::icon;
use crate::theme;

/// Past this size a file's shown without colours: setting up its colours costs the main
/// thread too, 38 ms (44 at worst) to open a 1 MB Rust file against 13 ms for 1.5 MB plain.
const MOST_COLOURED: usize = 1 << 20;

pub struct FileEditor {
    pub(super) document: Entity<Document>,
    /// The editor showing the document's text, and the document's version it holds.
    editor: Option<(Entity<EditorState>, u64)>,
    _document: Subscription,
}

impl FileEditor {
    /// A view of `document`, shown once it's read.
    pub fn new(document: Entity<Document>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let _document = cx.observe_in(&document, window, |this, _, window, cx| {
            this.show(window, cx);
        });
        let mut view = FileEditor {
            document,
            editor: None,
            _document,
        };
        view.show(window, cx);
        view
    }

    /// Puts the document's text in the editor, unless it holds that version already.
    fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let document = self.document.read(cx);
        let version = document.version();
        let Content::Text(text) = document.content() else {
            self.editor = None;
            cx.notify();
            return;
        };
        if self
            .editor
            .as_ref()
            .is_some_and(|(_, shown)| *shown == version)
        {
            return;
        }
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
        self.editor = Some((editor, version));
        cx.notify();
    }

    /// The text it shows, if it's read and shown as text.
    #[cfg(test)]
    pub(super) fn text(&self, cx: &gpui_kit::App) -> Option<SharedString> {
        Some(self.editor.as_ref()?.0.read(cx).value())
    }
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let content = div().size_full().relative();
        if let Some((editor, _)) = &self.editor {
            return content.child(
                Editor::new(editor)
                    .readonly(true)
                    .appearance(false)
                    .bordered(false)
                    .h_full(),
            );
        }
        let note: SharedString = match self.document.read(cx).content() {
            Content::Reading | Content::Text(_) => return content.child(spinner(p.muted)),
            Content::Binary => "This file looks binary, so it isn't shown.".into(),
            Content::TooLarge(bytes) => format!(
                "This file is {:.1} MB, more than the {:.1} MB the editor opens.",
                megabytes(*bytes),
                megabytes(crate::checkout::MOST_BYTES)
            )
            .into(),
            Content::Failed(why) => format!("Couldn't read this file: {why}").into(),
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
