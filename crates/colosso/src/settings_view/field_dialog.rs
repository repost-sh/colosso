//! A dialog asking for one value: a heading, a note, the field and Save. Enter saves too;
//! what's typed is checked and kept by the page that asks, its refusal shown as the note.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};

use crate::modal::{self, Button, Tone};
use crate::theme;

/// What a field dialog asks for.
pub struct Field {
    /// Its buttons' ids.
    pub ids: [&'static str; 2],
    pub title: &'static str,
    pub note: SharedString,
    pub placeholder: Option<&'static str>,
    pub value: String,
    /// Keeps what's typed, trimmed, or says why it can't be kept.
    pub save: fn(&str, &mut App) -> Result<(), SharedString>,
}

/// Opens the dialog for `field` over `window`.
pub fn open(field: Field, window: &mut Window, cx: &mut App) {
    modal::open(380., 212., window, cx, |window, cx| {
        FieldDialog::new(field, window, cx)
    });
}

struct FieldDialog {
    field: Field,
    input: Entity<InputState>,
    refused: Option<SharedString>,
    _subscription: Subscription,
}

impl FieldDialog {
    fn new(field: Field, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            let input = InputState::new(window, cx).default_value(field.value.clone());
            match field.placeholder {
                Some(placeholder) => input.placeholder(placeholder),
                None => input,
            }
        });
        input.focus_handle(cx).focus(window, cx);
        let _subscription = cx.subscribe_in(&input, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.save(window, cx);
            }
        });
        FieldDialog {
            field,
            input,
            refused: None,
            _subscription,
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = self.input.read(cx).value().trim().to_owned();
        match (self.field.save)(&value, cx) {
            Ok(()) => modal::dismiss(window, cx),
            Err(why) => {
                self.refused = Some(why);
                cx.notify();
            }
        }
    }
}

impl Render for FieldDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let note = self
            .refused
            .clone()
            .unwrap_or_else(|| self.field.note.clone());
        let [cancel, save] = self.field.ids;
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading(self.field.title))
            .child(div().mt(px(4.)).text_xs().text_color(p.muted).child(note))
            .child(
                div()
                    .mt(px(14.))
                    .child(modal::field(Input::new(&self.input), p)),
            )
            .child(div().flex_1())
            .child(
                modal::footer(None, p).child(modal::cancel(cancel)).child(
                    Button::new(save, "Save", Tone::Primary)
                        .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                ),
            )
    }
}
