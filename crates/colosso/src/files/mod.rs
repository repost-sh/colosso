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
mod switcher;
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

use gpui_kit::assets::IconName;
use gpui_kit::{
    App, Context, Div, ElementId, Entity, FontWeight, InteractiveElement as _, IntoElement,
    ModifiersChangedEvent, ParentElement as _, Stateful, Styled as _, Window, div, px,
};

use crate::modal;
use crate::navigation::Route;
use crate::right_panel::View;
use crate::shell::{self, Shell};
use crate::theme::Palette;

gpui_kit::actions!(files, [Save, CloseTab, SwitchTab, SwitchTabBack, CloseLocations]);

/// Binds the keys the file shown answers to, wherever the keyboard is (⌘S saves it, ⌘W
/// closes its tab, or else what's looked at instead, ⌃⇥ switches to another); in the
/// switcher, the keys that move its pick; and, while the list of places under the file has
/// the keyboard, the keys that move along it, Enter to open the place picked and Escape to
/// close it.
pub fn init(cx: &mut App) {
    use crate::list::{Activate, SelectFirst, SelectLast};
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-tab", SwitchTab, None),
        KeyBinding::new("ctrl-shift-tab", SwitchTabBack, None),
    ]);
    crate::list::bind_keys("TabSwitcher", cx);
    cx.bind_keys([KeyBinding::new("enter", Activate, Some("TabSwitcher"))]);
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
    /// What the window does with the files' actions when the keyboard is outside the Files
    /// panel, which answers them itself otherwise.
    pub fn handle_files(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &CloseTab, window, cx| this.close_tab(window, cx)))
            .on_action(cx.listener(|this, _: &SwitchTab, window, cx| this.switch_tab(1, window, cx)))
            .on_action(cx.listener(|this, _: &SwitchTabBack, window, cx| this.switch_tab(-1, window, cx)))
            // ⌃ let go before the switcher's window has the keyboard is heard here.
            .on_modifiers_changed(|event: &ModifiersChangedEvent, window, cx| {
                if !event.modifiers.control {
                    switcher::released(window, cx);
                }
            })
    }

    /// The Files panel, while it's shown with a file in it.
    fn files_shown(&self, cx: &App) -> Option<Entity<FilesPanel>> {
        (self.right_panel.files())
            .filter(|files| self.right_panel.shows(View::Files) && files.read(cx).viewing().is_some())
            .cloned()
    }

    /// ⌘W closes what's looked at: the settings, or the file shown in the Files panel,
    /// asking about unsaved edits first; with neither, it's left to the shell, which closes
    /// the tab in the middle. Never the window.
    fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.route(cx), Route::Settings(_)) {
            self.navigate(Route::Workspace, cx);
        } else if let Some(files) = self.files_shown(cx) {
            files.update(cx, |files, cx| files.close_shown(window, cx));
        } else {
            cx.propagate();
        }
    }

    /// ⌃⇥: lists the Files panel's tabs to switch between, or moves the list's pick `by`
    /// rows. With no file shown, the key is left to whatever else listens for it.
    fn switch_tab(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        match self.files_shown(cx) {
            Some(files) => switcher::switch(files, by, window, cx),
            None => cx.propagate(),
        }
    }
}

/// A row that names a file, as the palettes list them: its icon, its `name` in bold and its
/// `folder` dimmed after it, with room for what the caller adds after that.
pub fn file_row(
    id: impl Into<ElementId>,
    selected: bool,
    icon: IconName,
    name: impl IntoElement,
    folder: impl IntoElement,
    p: Palette,
) -> Stateful<Div> {
    shell::row(id, selected, p)
        .w_full()
        .h(px(ROW))
        .gap_1p5()
        .rounded(px(5.))
        .child(shell::icon(icon, 14.).text_color(p.muted))
        .child(div().flex_shrink_0().font_weight(FontWeight::SEMIBOLD).child(name))
        .child(shell::ellipsized(folder).flex_1().text_color(p.muted))
}
