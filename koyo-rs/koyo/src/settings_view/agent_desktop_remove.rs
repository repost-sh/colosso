//! Settings → Agent desktop → Remove… (`docs/agent-desktop.md` 5.5): macOS deletes a user
//! record only with an administrator's own name and password, so the dialog asks for them. They
//! go to the helper's daemon alone, which checks them before touching anything and keeps them
//! nowhere; the app doesn't either.

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Entity, Focusable as _, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div, px,
};

use crate::modal::{self, Button, Tone};
use crate::{agent_desktop, theme};

pub fn open(window: &mut Window, cx: &mut gpui_kit::App) {
    modal::open(420., 300., window, cx, Removal::new);
}

struct Removal {
    name: Entity<InputState>,
    password: Entity<InputState>,
    refused: Option<SharedString>,
    _subscription: Subscription,
}

impl Removal {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let me = std::env::var("USER").unwrap_or_default();
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(me)
                .placeholder("Administrator's name")
        });
        let password = cx.new(|cx| {
            InputState::new(window, cx)
                .masked(true)
                .placeholder("Password")
        });
        password.focus_handle(cx).focus(window, cx);
        let _subscription = cx.subscribe_in(&password, window, |this, _, event, window, cx| {
            if let InputEvent::PressEnter { .. } = event {
                this.remove(window, cx);
            }
        });
        Removal {
            name,
            password,
            refused: None,
            _subscription,
        }
    }

    fn remove(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.name.read(cx).value().trim().to_owned();
        let password = self.password.read(cx).value().to_string();
        if name.is_empty() || password.is_empty() {
            self.refused = Some("Both the name and the password are needed.".into());
            cx.notify();
            return;
        }
        modal::dismiss(window, cx);
        agent_desktop::remove(name, password, cx);
    }
}

impl Render for Removal {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let note: SharedString = self.refused.clone().unwrap_or_else(|| {
            "The agent account, its home and its session go, with Screen Sharing's access for it, \
             the firewall rule and the helper. macOS asks an administrator's own password to \
             delete an account; it goes to Koyo's helper and nowhere else."
                .into()
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(modal::heading("Remove the agent desktop?"))
            .child(div().mt(px(4.)).text_xs().text_color(p.muted).child(note))
            .child(
                div()
                    .mt(px(14.))
                    .child(modal::field(Input::new(&self.name), p)),
            )
            .child(
                div()
                    .mt(px(8.))
                    .child(modal::field(Input::new(&self.password), p)),
            )
            .child(div().flex_1())
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("agent-desktop-remove-cancel"))
                    .child(
                        Button::new("agent-desktop-remove-confirm", "Remove", Tone::Danger)
                            .on_click(cx.listener(|this, _, window, cx| this.remove(window, cx))),
                    ),
            )
    }
}
