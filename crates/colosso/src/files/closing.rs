//! Asking before a tab whose file has unsaved edits is closed: they're saved, let go, or
//! the tab stays open.

use gpui_kit::{
    App, Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div,
};

use super::FileEditor;
use super::panel::FilesPanel;
use crate::modal::{self, Button, LINE, SMALL_LINE, Tone};
use crate::theme;

const WIDTH: f32 = 400.;
const GAP: f32 = 8.;
const HEIGHT: f32 = 2. * modal::PADDING + LINE + GAP + SMALL_LINE + 2. * GAP + modal::FOOTER;

/// Asks over `window` what to do with the unsaved edits in `editor`, the view in a tab of
/// `panel` showing the file called `name`: the tab closes unless it's kept.
pub fn ask(
    panel: Entity<FilesPanel>,
    editor: Entity<FileEditor>,
    name: &str,
    window: &mut Window,
    cx: &mut App,
) {
    let dialog = SaveDialog {
        panel,
        editor,
        title: format!("Save the changes to {name}?").into(),
        saving: false,
        error: None,
    };
    modal::open(WIDTH, HEIGHT, window, cx, |_, _| dialog);
}

pub(super) struct SaveDialog {
    panel: Entity<FilesPanel>,
    editor: Entity<FileEditor>,
    title: SharedString,
    saving: bool,
    error: Option<SharedString>,
}

impl SaveDialog {
    /// Saves the file, and closes the tab once it's written; a failure is shown instead.
    pub(super) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.saving {
            return;
        }
        let saved = self.editor.update(cx, FileEditor::save);
        self.saving = true;
        self.error = None;
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let saved = saved.await;
            this.update_in(cx, |this, window, cx| match saved {
                Ok(()) => this.close_tab(window, cx),
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

    /// Closes the tab, with whatever edits it still has, and the dialog.
    pub(super) fn close_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor = self.editor.clone();
        self.panel
            .update(cx, |panel, cx| panel.remove_view(&editor, cx));
        modal::dismiss(window, cx);
    }
}

impl Render for SaveDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
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
                            .on_click(cx.listener(|this, _, window, cx| this.close_tab(window, cx))),
                    )
                    .child(
                        Button::new("closing-save", "Save", Tone::Primary)
                            .busy(self.saving)
                            .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                    ),
            )
    }
}
