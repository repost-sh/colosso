//! A symbol renamed in place: F2 puts a field over it, its name in it and selected; Enter
//! gives the server the new name, whose edit lands here and in the other files; Escape
//! leaves the name as it was, as does the keyboard leaving the field, or the symbol
//! scrolling out of view.

use std::ops::Range;

use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    ParentElement as _, Styled as _, Subscription, Window, anchored, deferred, point, px,
};

use super::editor::FileEditor;
use crate::language::{Prepared, Rename};
use crate::modal;
use crate::theme::{self, Palette};

/// The field over the symbol being renamed.
pub(super) struct Renaming {
    input: Entity<InputState>,
    /// The symbol's bytes in the editor's text, which the field sits over.
    range: Range<usize>,
    /// What it's called, which the field starts with.
    name: String,
    _events: Subscription,
}

impl FileEditor {
    /// F2: asks what's at the caret to rename, and puts the field over it.
    pub(super) fn rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(attached), Some(shown)) = (&self.language, &self.editor) else {
            return;
        };
        if self.renaming.is_some() {
            return;
        }
        let prepared = attached.prepare_rename(&shown.editor, cx);
        self.asking = cx.spawn_in(window, async move |this, cx| {
            if let Some(prepared) = prepared.await {
                this.update_in(cx, |this, window, cx| this.show_rename(prepared, window, cx))
                    .ok();
            }
        });
    }

    /// Puts the field over the symbol `prepared`, its name in it, selected, and gives it
    /// the keyboard; unless the keyboard left the file meanwhile, or the symbol's gone.
    fn show_rename(&mut self, prepared: Prepared, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor.as_ref().map(|shown| shown.editor.clone()) else {
            return;
        };
        if !editor.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        // Carried past what was typed while the server was asked.
        let Some(range) = editor.read(cx).translate_range(prepared.range, prepared.version) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx));
        input.update(cx, |input, cx| {
            input.set_value(&prepared.name, window, cx);
            input.select_all(window, cx);
            input.focus(window, cx);
        });
        let _events = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.commit_rename(window, cx),
            // The keyboard went elsewhere: the rename's left, the keyboard where it went.
            InputEvent::Blur => this.cancel_rename(cx),
            InputEvent::Change | InputEvent::Focus => {}
        });
        self.renaming = Some(Renaming {
            input,
            range,
            name: prepared.name,
            _events,
        });
        cx.notify();
    }

    /// Enter in the field: the name typed goes to the server, unless it's as it was, and
    /// the keyboard goes back to the editor.
    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        let name = renaming.input.read(cx).value().trim().to_owned();
        self.focus_editor(window, cx);
        cx.notify();
        if name.is_empty() || name == renaming.name {
            return;
        }
        if let (Some(attached), Some(shown)) = (&self.language, &self.editor) {
            attached.rename(&shown.editor, renaming.range.start, name, window, cx);
        }
    }

    /// The field goes, the name as it was.
    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    /// The field over the symbol being renamed, while there is one.
    #[cfg(test)]
    pub fn rename_field(&self) -> Option<&Entity<InputState>> {
        self.renaming.as_ref().map(|renaming| &renaming.input)
    }

    /// The field over the symbol being renamed, anchored on the symbol and wide enough for
    /// its name, in the editor's type. The symbol scrolled out of view ends the rename, as
    /// the keyboard leaving does.
    pub(super) fn rename_field_over(
        &mut self,
        p: Palette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let editor = self.editor.as_ref()?.editor.clone();
        let renaming = self.renaming.as_ref()?;
        let Some(bounds) = editor.read(cx).range_to_bounds(&renaming.range) else {
            self.cancel_rename(cx);
            self.focus_editor(window, cx);
            return None;
        };
        let field = modal::field(Input::new(&renaming.input), p)
            .on_action(cx.listener(|this, _: &Escape, window, cx| {
                this.cancel_rename(cx);
                this.focus_editor(window, cx);
            }))
            .h(px(26.))
            .min_w(px(160.))
            .w(bounds.size.width + px(28.))
            .rounded(px(5.))
            .border_color(p.accent)
            .shadow_md()
            .font_family(theme::MONO)
            .text_sm();
        let anchored = anchored()
            .position(bounds.origin - point(px(6.), px(4.)))
            .snap_to_window_with_margin(px(8.))
            .child(field);
        Some(deferred(anchored).with_priority(1))
    }
}
