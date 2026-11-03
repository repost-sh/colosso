//! Asking before unsaved edits would be lost: closing a tab or a pane whose file has them,
//! or the window with any. They're saved, let go, or what was closing stays open.

use futures::future::join_all;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, div,
};

use super::FileEditor;
use super::file_pane::name;
use crate::modal::{self, Button, SMALL_LINE, Tone};
use crate::theme;

const WIDTH: f32 = 400.;
const GAP: f32 = 8.;

/// Asks over `window` what to do with the unsaved edits in `editors` before the window
/// closes: it closes unless it's kept.
pub fn ask_before_closing(editors: Vec<Entity<FileEditor>>, window: &mut Window, cx: &mut App) {
    ask(editors, Then::CloseWindow(window.window_handle()), window, cx);
}

/// Asks over `window` what to do with the unsaved edits in `editors` before `then` happens:
/// it does unless they're kept.
pub fn ask(editors: Vec<Entity<FileEditor>>, then: Then, window: &mut Window, cx: &mut App) {
    let title: SharedString = match editors.as_slice() {
        [editor] => format!("Save the changes to {}?", file_name(editor, cx)),
        _ => format!("Save the changes to {} files?", editors.len()),
    }
    .into();
    // Sized for the title as it wraps: a long file name takes it onto a second line.
    let heading = modal::heading_height(&title, WIDTH, window);
    let height = 2. * modal::PADDING + heading + GAP + SMALL_LINE + 2. * GAP + modal::FOOTER;
    let dialog = SaveDialog {
        editors,
        then: Some(then),
        title,
        saving: false,
        error: None,
    };
    modal::open(WIDTH, height, window, cx, |_, _| dialog);
}

/// The name of the file `editor` shows.
fn file_name(editor: &Entity<FileEditor>, cx: &App) -> String {
    (editor.read(cx).document()).map_or_else(
        || "the file".to_owned(),
        |document| name(document.read(cx).path()),
    )
}

/// What closes a tab or a pane, once.
pub type Closing = Box<dyn FnOnce(&mut App)>;

/// What closes once the edits are saved or let go.
pub enum Then {
    /// A tab or a pane, closed by what's given.
    Close(Closing),
    /// The window the question's asked over.
    CloseWindow(AnyWindowHandle),
}

pub(super) struct SaveDialog {
    editors: Vec<Entity<FileEditor>>,
    /// Taken when the dialog closes, so it happens once.
    then: Option<Then>,
    title: SharedString,
    saving: bool,
    error: Option<SharedString>,
}

impl SaveDialog {
    /// Saves the files, and closes what was closing once they're written; a failure is shown
    /// instead.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let writes: Vec<_> = (self.editors.iter())
            .map(|editor| editor.update(cx, FileEditor::save))
            .collect();
        self.saving = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let saved: Result<(), SharedString> = join_all(writes).await.into_iter().collect();
            this.update_in(cx, |this, window, cx| match saved {
                Ok(()) => this.close(window, cx),
                Err(error) => {
                    this.saving = false;
                    modal::fail(&mut this.error, "save the file", error);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Closes what was closing, with whatever edits its files still have, and the dialog.
    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.then.take() {
            Some(Then::Close(close)) => close(cx),
            Some(Then::CloseWindow(behind)) => {
                (behind.update(cx, |_, window, _| window.remove_window())).ok();
            }
            None => {}
        }
        modal::dismiss(window, cx);
    }
}

impl Render for SaveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let save = if self.editors.len() > 1 { "Save All" } else { "Save" };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(gpui_kit::px(GAP))
            .child(modal::heading(self.title.clone()))
            .child(
                div()
                    .flex_1()
                    .text_xs()
                    .text_color(p.muted)
                    .child("The changes are lost if they aren't saved."),
            )
            .child(
                modal::footer(self.error.clone(), p)
                    .child(modal::cancel("closing-cancel"))
                    .child(
                        Button::new("closing-discard", "Don't Save", Tone::Plain)
                            .disabled(self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.close(window, cx))),
                    )
                    .child(
                        Button::new("closing-save", save, Tone::Primary)
                            .busy(self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests;
