//! The Files panel: the selected checkout's files as a tree, in the right panel in place of
//! the changes, and the editor tabs it opens files in.

mod editor;
mod icons;
mod panel;
mod render;
#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use editor::FileEditor;
pub use panel::{FilesPanel, OpenFile};
