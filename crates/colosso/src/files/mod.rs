//! The Files panel: the selected checkout's files as a tree, in the right panel in place of
//! the changes, and beside the tree the file picked in it, in an editor; under the editor,
//! when a symbol's references or definitions were asked for, the list of them.

mod closing;
mod editor;
mod icons;
mod locations;
mod panel;
mod place;
mod render;
mod rename;
mod tabs;
#[cfg(test)]
mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use editor::FileEditor;
pub use icons::file as file_icon;
pub use panel::FilesPanel;
pub use place::{At, Place, relative};
pub use render::ROW;

gpui_kit::actions!(files, [Save, CloseLocations]);

/// Binds the keys the file shown answers to, wherever the keyboard is; and, while the list
/// of places under it has the keyboard, the keys that move along it, Enter to open the
/// place picked and Escape to close it.
pub fn init(cx: &mut gpui_kit::App) {
    use crate::list::{Activate, SelectFirst, SelectLast};
    use gpui_kit::KeyBinding;
    cx.bind_keys([KeyBinding::new("cmd-s", Save, None)]);
    crate::list::bind_keys("Locations", cx);
    let list = Some("Locations");
    cx.bind_keys([
        KeyBinding::new("home", SelectFirst, list),
        KeyBinding::new("end", SelectLast, list),
        KeyBinding::new("enter", Activate, list),
        KeyBinding::new("escape", CloseLocations, list),
    ]);
}
