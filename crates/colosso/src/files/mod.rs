//! The Files panel: the selected checkout's files as a tree, in the right panel in place of
//! the changes, and beside the tree the file picked in it, in an editor.

mod closing;
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

gpui_kit::actions!(files, [Save]);

/// Binds the keys the file shown answers to, wherever the keyboard is.
pub fn init(cx: &mut gpui_kit::App) {
    cx.bind_keys([gpui_kit::KeyBinding::new("cmd-s", Save, None)]);
}
