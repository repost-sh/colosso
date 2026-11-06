//! A markdown file, shown as its code or as it reads: a preview of the text in its editor,
//! unsaved edits and all, its Mermaid blocks drawn as diagrams. A tab opens it as Settings →
//! Editor says, as it reads unless set otherwise, or as code when it's opened at a place in its
//! text; the other is picked on a floating Liquid Glass toggle at its top right, or with ⌘⇧V.

mod diagram;
mod mermaid;
mod preview;
mod toggle;
#[cfg(test)]
mod tests;

use std::path::Path;

use gpui_kit::Action;
use gpui_kit::component::native_menu::NativeMenu;
use serde::{Deserialize, Serialize};

pub use preview::{MarkdownPreview, bind_keys};
pub use toggle::toggle;

use crate::settings::checked_menu;

/// How a markdown file is shown, and how it opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    /// Its text, in the editor.
    Code,
    /// As it reads.
    Preview,
}

/// Sets how a markdown file opens, from the Editor settings' menu.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = settings, no_json)]
pub struct SetMarkdownOpensAs(pub Mode);

impl Mode {
    const ALL: [Mode; 2] = [Mode::Preview, Mode::Code];

    /// Its name, as the toggle's segment and the settings' menu have it.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Code => "Code",
            Mode::Preview => "Preview",
        }
    }

    /// A menu of how a markdown file can open, with `self` checked.
    pub fn menu(self) -> NativeMenu {
        checked_menu(Mode::ALL, self, Mode::label, |mode| Box::new(SetMarkdownOpensAs(mode)))
    }

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
