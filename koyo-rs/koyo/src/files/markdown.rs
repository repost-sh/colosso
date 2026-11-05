//! A markdown file, shown as its code or as it reads: a preview of the text in its editor,
//! unsaved edits and all, its Mermaid blocks drawn as diagrams. Which of the two a tab shows is
//! picked on a floating Liquid Glass toggle at its top right, or with ⌘⇧V.

mod diagram;
mod mermaid;
mod preview;
mod toggle;
#[cfg(test)]
mod tests;

use std::path::Path;

pub use preview::{MarkdownPreview, bind_keys};
pub use toggle::toggle;

/// How a markdown file is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Its text, in the editor.
    Code,
    /// As it reads.
    Preview,
}

impl Mode {
    /// The other one.
    pub fn flipped(self) -> Mode {
        match self {
            Mode::Code => Mode::Preview,
            Mode::Preview => Mode::Code,
        }
    }
}

/// Whether the file at `path` is markdown, by its name, as the editor colours it.
pub fn is_markdown(path: &Path) -> bool {
    super::editor::language(path, "") == "markdown"
}
