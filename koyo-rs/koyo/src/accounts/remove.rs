//! Asking before an account is removed, as claude-swap's `remove` does: Koyo forgets the
//! account's stored login for good, and the Mac's own login is left as it is.

use gpui_kit::{
    App, Context, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window, div,
    px,
};

use super::{Job, Slot};
use crate::modal::{self, Button, Tone};
use crate::theme;

struct Removing(Slot);

impl Render for Removing {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let slot = self.0.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(modal::heading(heading(&slot)))
            .child(div().flex_1().text_color(p.muted).child(
                "Koyo forgets this account's login. To keep it again, sign in to it with Claude \
                 Code and add the current login. The Mac's login is left as it is.",
            ))
            .child(
                modal::footer(None, p)
                    .child(modal::cancel("accounts-remove-cancel"))
                    .child(
                        Button::new("accounts-remove-confirm", "Remove", Tone::Danger).on_click(
                            move |_, window, cx| {
                                modal::dismiss(window, cx);
                                let job = Job::Remove(slot.clone());
                                super::accounts(cx)
                                    .update(cx, |accounts, cx| accounts.ask(job, cx));
                            },
                        ),
                    ),
            )
    }
}

const WIDTH: f32 = 380.;
/// The text under the heading: three lines at the dialog's width.
const BODY: f32 = 3. * modal::LINE;
const GAP: f32 = 8.;

/// Asks the person whether to remove `slot`'s account; the dialog is as tall as its heading,
/// which names the account, needs.
pub fn ask(slot: Slot, window: &mut Window, cx: &mut App) {
    let title = modal::heading_height(&heading(&slot), WIDTH, window);
    let height = 2. * modal::PADDING + title + GAP + BODY + GAP + modal::FOOTER;
    modal::open(WIDTH, height, window, cx, |_, _| Removing(slot));
}

fn heading(slot: &Slot) -> SharedString {
    format!("Remove {}?", slot.email).into()
}
