//! The dialog that commits what's changed in a chat's checkout, with the message a person gives.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    App, AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};

use super::counted;
use crate::checkout::Checkout;
use crate::modal::{self, Button, FIELD, FOOTER, HEADING_LINE, PADDING, Tone};
use crate::theme;

const WIDTH: f32 = 420.;
/// What's over the field, and under it.
const OVER: f32 = 14.;
const UNDER: f32 = 16.;
const HEIGHT: f32 = PADDING * 2. + HEADING_LINE + OVER + FIELD + UNDER + FOOTER;

pub(super) fn open(files: usize, checkout: Entity<Checkout>, window: &mut Window, cx: &mut App) {
    modal::open(WIDTH, HEIGHT, window, cx, |window, cx| {
        let message = cx.new(|cx| InputState::new(window, cx).placeholder("What changed, and why"));
        message.focus_handle(cx).focus(window, cx);
        let _typing = cx.subscribe_in(
            &message,
            window,
            |this: &mut CommitDialog, _, event, window, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } => this.commit(window, cx),
                _ => {}
            },
        );
        CommitDialog {
            files,
            checkout,
            message,
            busy: false,
            error: None,
            _typing,
        }
    });
}

struct CommitDialog {
    files: usize,
    checkout: Entity<Checkout>,
    message: Entity<InputState>,
    busy: bool,
    error: Option<SharedString>,
    _typing: Subscription,
}

impl CommitDialog {
    fn commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = self.message.read(cx).value().trim().to_owned();
        if message.is_empty() || self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let running = self.checkout.update(cx, |checkout, cx| {
            checkout.run(move |git| git.commit_all(&message), cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            match running.await {
                Ok(()) => this
                    .update_in(cx, |_, window, cx| modal::dismiss(window, cx))
                    .ok(),
                Err(error) => this
                    .update(cx, |this, cx| {
                        this.busy = false;
                        modal::fail(&mut this.error, "commit", error.message);
                        cx.notify();
                    })
                    .ok(),
            };
        })
        .detach();
    }
}

impl Render for CommitDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let empty = self.message.read(cx).value().trim().is_empty();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading(format!(
                "Commit {}",
                counted(self.files, "file")
            )))
            .child(div().mt(px(OVER)).child(modal::field(
                Input::new(&self.message).disabled(self.busy),
                p,
            )))
            .child(
                modal::footer(self.error.clone(), p)
                    .mt(px(UNDER))
                    .child(modal::cancel("chat-commit-cancel"))
                    .child(
                        Button::new("chat-commit", "Commit", Tone::Primary)
                            .busy(self.busy)
                            .disabled(empty)
                            .on_click(cx.listener(|this, _, window, cx| this.commit(window, cx))),
                    ),
            )
    }
}
