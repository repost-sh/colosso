//! The dialog a question shows in: its words, its fields, and the buttons that answer it
//! to the page's view. Put away without an answer, it answers no.

use colosso_browser_ipc::Answer;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, KeyDownEvent, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
};

use super::{Carries, Words};
use crate::browser::view::BrowserView;
use crate::modal::{self, Button, Tone};
use crate::theme;

/// The dialog a question shows in, answering to the page's view.
pub(super) struct PageDialog {
    view: WeakEntity<BrowserView>,
    id: u64,
    words: Words,
    /// The prompt's answer, or the sign-in's user name and password.
    fields: Vec<Entity<InputState>>,
    answered: bool,
    focus: FocusHandle,
    focused: bool,
    _subscriptions: Vec<Subscription>,
}

impl PageDialog {
    pub(super) fn new(
        view: WeakEntity<BrowserView>,
        id: u64,
        words: Words,
        fields: Vec<(&'static str, String, bool)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fields: Vec<Entity<InputState>> = fields
            .into_iter()
            .map(|(placeholder, text, secret)| {
                cx.new(|cx| {
                    let mut state = InputState::new(window, cx)
                        .placeholder(placeholder)
                        .masked(secret);
                    if !text.is_empty() {
                        state.set_value(text, window, cx);
                    }
                    state
                })
            })
            .collect();
        let subscriptions = fields
            .iter()
            .map(|field| {
                cx.subscribe_in(field, window, |this, _, event: &InputEvent, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.answer(true, window, cx);
                    }
                })
            })
            .collect();
        // Put away without an answer, by Escape or a press elsewhere: the page gets a no.
        cx.on_release(|dialog: &mut Self, cx| {
            if !dialog.answered {
                let id = dialog.id;
                let _ = dialog
                    .view
                    .update(cx, |view, cx| view.answered(id, Answer::Dismiss, cx));
            }
        })
        .detach();
        PageDialog {
            view,
            id,
            words,
            fields,
            answered: false,
            focus: cx.focus_handle(),
            focused: false,
            _subscriptions: subscriptions,
        }
    }

    fn answer(&mut self, accept: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.answered {
            return;
        }
        self.answered = true;
        let text = |at: usize| {
            self.fields
                .get(at)
                .map(|field| field.read(cx).value().to_string())
                .unwrap_or_default()
        };
        let answer = match (accept, &self.words.carries) {
            (false, _) => Answer::Dismiss,
            (true, Carries::Nothing) => Answer::Accept,
            (true, Carries::Text { .. }) => Answer::Text { text: text(0) },
            (true, Carries::Credentials) => Answer::Credentials {
                username: text(0),
                password: text(1),
            },
        };
        let id = self.id;
        let _ = self
            .view
            .update(cx, |view, cx| view.answered(id, answer, cx));
        modal::dismiss(window, cx);
    }

    /// Tab moves to the next field, Shift-Tab back, round the fields, its text selected.
    fn next_field(&self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.fields.len();
        let at = self
            .fields
            .iter()
            .position(|field| field.read(cx).focus_handle(cx).is_focused(window))
            .unwrap_or(count - 1);
        let to = if forward {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        self.fields[to].update(cx, |field, cx| {
            field.focus(window, cx);
            field.select_all(window, cx);
        });
    }

    fn button(&self, accept: bool, cx: &mut Context<Self>) -> Option<Button> {
        let (id, label) = if accept {
            ("browser-dialog-accept", self.words.confirm)
        } else {
            ("browser-dialog-dismiss", self.words.cancel?)
        };
        let tone = if accept == self.words.accept_default {
            Tone::Primary
        } else {
            Tone::Plain
        };
        let answer = cx.listener(move |this, _: &ClickEvent, window, cx| {
            this.answer(accept, window, cx);
        });
        Some(Button::new(id, label, tone).on_click(answer))
    }
}

impl Render for PageDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !std::mem::replace(&mut self.focused, true) {
            let first = self
                .fields
                .first()
                .map(|field| field.read(cx).focus_handle(cx));
            window.focus(&first.unwrap_or_else(|| self.focus.clone()), cx);
        }
        let p = theme::palette(cx);
        let accept_default = self.words.accept_default;
        // The safe answer sits on the right, where Return lands.
        let mut buttons = vec![self.button(false, cx), self.button(true, cx)];
        if !accept_default {
            buttons.reverse();
        }
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "enter" => this.answer(accept_default, window, cx),
                    "tab" if this.fields.len() > 1 => {
                        this.next_field(!event.keystroke.modifiers.shift, window, cx);
                    }
                    _ => return,
                }
                cx.stop_propagation();
            }))
            .child(modal::heading(self.words.heading.clone()))
            .child(
                div()
                    .id("browser-dialog-text")
                    .mt_2()
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .text_color(p.muted)
                    .children(self.words.text.clone()),
            )
            .children(
                self.fields
                    .iter()
                    .map(|field| div().mt_2().child(modal::field(Input::new(field), p))),
            )
            .child(
                div()
                    .mt_3()
                    .child(modal::footer(None, p).children(buttons.into_iter().flatten())),
            )
    }
}
