//! The Files panel: the selected checkout's files as a tree, in the right panel in place of
//! the changes, and beside the tree the file picked in it, in an editor.

mod editor;
mod icons;
mod panel;
mod place;
mod render;
mod tabs;
#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use editor::FileEditor;
pub use icons::file as file_icon;
pub use panel::FilesPanel;
pub use place::{At, Place};
pub use render::ROW;
