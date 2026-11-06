//! Files: the selected checkout's files as a tree, in the right panel in place of the
//! changes; each file opened from it, or from elsewhere, in a workspace tab of its own, in
//! an editor, or a markdown file as it reads; and under the tabs, when a symbol's references
//! or definitions were asked for, the list of them.

mod breadcrumb;
mod closing;
mod editor;
mod file_pane;
mod icons;
mod link;
mod locations;
mod markdown;
mod open;
mod panel;
mod place;
mod rename;
mod render;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(feature = "visual-check")]
pub mod visual;

pub use closing::{Then, ask, ask_before_closing};
pub use editor::{FileEditor, MOST_COLOURED};
pub use file_pane::{FilePane, name as file_name};
pub use icons::file as file_icon;
pub use link::{FileLinks, OpenLinked, opens_links};
pub use locations::Locations;
pub use markdown::{Mode as MarkdownMode, SetMarkdownOpensAs};
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

gpui_kit::actions!(files, [Save, CloseLocations, TogglePreview]);

/// Binds ⌘S, which saves the file shown, or every file as set, wherever the keyboard is; ⌘⇧V,
/// which shows a markdown file as it reads or as code again, and the keys that scroll it as
/// it reads; and, while the list of places under the tabs has the keyboard, the keys that
/// move along it, Enter to open the place picked and Escape to close it.
pub fn init(cx: &mut App) {
    use crate::list::{Activate, SelectFirst, SelectLast};
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-shift-v", TogglePreview, None),
    ]);
    markdown::bind_keys(cx);
    cx.on_action(|SetMarkdownOpensAs(mode): &SetMarkdownOpensAs, cx| {
        settings::update(cx, |settings| settings.markdown_opens_as = *mode);
    });
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
            .on_action(cx.listener(|this, open: &OpenLinked, window, cx| {
                this.open_path(&open.path, open.at.clone(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &TogglePreview, window, cx| {
                this.toggle_preview(window, cx);
            }))
    }

    /// ⌘⇧V shows the markdown file shown as it reads, or as code again; on a settings page,
    /// or with any other file shown, nothing.
    fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.route(cx), Route::Settings(_)) {
            return;
        }
        if let Some(file) = self.shown_file(window, cx) {
            file.update(cx, |file, cx| file.flip(window, cx));
        }
    }

    /// ⌘S saves the file shown or, as set, every file with unsaved edits in the window; on
    /// a settings page, with no file in sight, nothing.
    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.route(cx), Route::Settings(_)) {
            return;
        }
        let editors = match settings::get(cx).save_scope {
            SaveScope::Shown => (self.shown_file(window, cx).into_iter())
                .filter_map(|file| file.read(cx).editor().cloned())
                .collect(),
            SaveScope::All => self.unsaved(cx),
        };
        for editor in editors {
            (editor.update(cx, FileEditor::save)).detach_and_log_err(cx);
        }
    }
}

/// A file as the file tree names it: its icon, dimmed, `size` big, and its `name` beside it. The
/// tree's rows, the palettes' and a link to a file in a chat all draw one.
pub fn file_label(icon: IconName, size: f32, name: impl IntoElement, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .min_w_0()
        .child(shell::icon(icon, size).flex_shrink_0().text_color(p.muted))
        .child(name)
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
    let name = div().font_weight(FontWeight::SEMIBOLD).child(name);
    shell::row(id, selected, p)
        .w_full()
        .h(px(ROW))
        .gap_1p5()
        .rounded(px(5.))
        .child(file_label(icon, 14., name, p).flex_shrink_0())
        .child(shell::ellipsized(folder).flex_1().text_color(p.muted))
}
