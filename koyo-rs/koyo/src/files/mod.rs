//! Files: the selected checkout's files as a tree, in the right panel in place of the
//! changes; each file opened from it, or from elsewhere, in a workspace tab of its own, in
//! an editor; and under the tabs, when a symbol's references or definitions were asked
//! for, the list of them.

mod breadcrumb;
mod closing;
mod editor;
mod file_pane;
mod icons;
mod locations;
mod open;
mod panel;
mod place;
mod render;
mod rename;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use closing::{Then, ask, ask_before_closing};
pub use editor::{FileEditor, MOST_COLOURED};
pub use file_pane::{FilePane, name as file_name};
pub use icons::file as file_icon;
pub use locations::Locations;
pub use panel::FilesPanel;
pub use place::{At, Place, relative};
pub use render::ROW;

use gpui_kit::assets::IconName;
use gpui_kit::{
    App, Context, Div, ElementId, FontWeight, InteractiveElement as _, IntoElement,
    ParentElement as _, Stateful, Styled as _, TaskExt as _, Window, div, px,
};

use crate::modal;
use crate::navigation::Route;
use crate::settings::{self, SaveScope};
use crate::shell::{self, Shell};
use crate::theme::Palette;

gpui_kit::actions!(files, [Save, CloseLocations]);

/// Binds ⌘S, which saves the file shown, or every file as set, wherever the keyboard is;
/// and, while the list of places under the tabs has the keyboard, the keys that move along
/// it, Enter to open the place picked and Escape to close it.
pub fn init(cx: &mut App) {
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

/// Whether a question about unsaved edits is on screen.
pub fn asking(cx: &App) -> bool {
    modal::current::<closing::SaveDialog>(cx).is_some()
}

impl Shell {
    /// What the window does with the files' actions, wherever the keyboard is.
    pub fn handle_files(root: Div, cx: &mut Context<Self>) -> Div {
        root.on_action(cx.listener(|this, _: &Save, window, cx| this.save(window, cx)))
    }

    /// ⌘S saves the file shown or, as set, every file with unsaved edits in the window; on
    /// a settings page, with no file in sight, nothing.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.route(cx), Route::Settings(_)) {
            return;
        }
        let editors = match settings::get(cx).save_scope {
            SaveScope::Shown => (self.shown_file(window, cx).into_iter())
                .map(|file| file.read(cx).editor.clone())
                .collect(),
            SaveScope::All => self.unsaved(cx),
        };
        for editor in editors {
            (editor.update(cx, FileEditor::save)).detach_and_log_err(cx);
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
