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

pub use editor::{FileEditor, MOST_COLOURED};
pub use icons::file as file_icon;
pub use panel::FilesPanel;
pub use place::{At, Place, relative};
pub use closing::ask_before_closing;
pub use render::ROW;

use gpui_kit::{App, Context, Div, InteractiveElement as _, Window};

use crate::modal;
use crate::right_panel::View;
use crate::shell::Shell;

gpui_kit::actions!(files, [Save, CloseTab, CloseLocations]);

/// Binds the keys the file shown answers to, wherever the keyboard is (⌘S saves it, ⌘W
/// closes its tab); and, while the list of places under it has the keyboard, the keys that
/// move along it, Enter to open the place picked and Escape to close it.
pub fn init(cx: &mut App) {
    use crate::list::{Activate, SelectFirst, SelectLast};
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-w", CloseTab, None),
    ]);
    crate::list::bind_keys("Locations", cx);
    let list = Some("Locations");
    cx.bind_keys([
        KeyBinding::new("home", SelectFirst, list),
        KeyBinding::new("end", SelectLast, list),
        KeyBinding::new("enter", Activate, list),
        KeyBinding::new("escape", CloseLocations, list),
    ]);
}

/// Whether a question about unsaved edits is on screen.
pub fn asking(cx: &App) -> bool {
    modal::current::<closing::SaveDialog>(cx).is_some()
}

impl Shell {
    /// What the window does with the files' actions, wherever the keyboard is.
    pub fn handle_files(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_tab(window, cx)))
    }

    /// ⌘W: closes the tab shown in the Files panel, asking about unsaved edits first; with no
    /// tab to close, the window, as a browser's last tab does.
    fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.right_panel.shows(View::Files)
            && let Some(files) = self.right_panel.files().cloned()
            && files.read(cx).viewing().is_some()
        {
            files.update(cx, |files, cx| files.close(files.shown, window, cx));
        } else {
            self.close_window(window, cx);
        }
    }
}
