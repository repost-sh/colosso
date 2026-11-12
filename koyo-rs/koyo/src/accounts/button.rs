//! The accounts' button (`docs/claude-accounts.md` 6), in the room at the left of the Arc
//! sidebar's foot that keeps the space marks centred against the + on the right. It shows the
//! active account's week as a ring, in the text's colour, amber as it nears auto-switch's line
//! and red at it, as the panel's bars are; and its press opens the panel over it. A view of its
//! own, so a new reading redraws it and not the shell. With Koyo not managing the accounts, it's
//! nothing.

use super::model::View;
use super::{Accounts, panel};
use crate::ring::ring;
use crate::theme;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    AnyElement, App, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    px,
};

pub struct AccountsButton {
    accounts: Entity<Accounts>,
    _observing: Subscription,
}

impl AccountsButton {
    pub fn new(accounts: Entity<Accounts>, cx: &mut Context<Self>) -> Self {
        let observing = cx.observe(&accounts, |_, _, cx| cx.notify());
        AccountsButton {
            accounts,
            _observing: observing,
        }
    }
}

/// The button as `view` has it.
pub(super) fn drawn(view: &View, cx: &App) -> AnyElement {
    let p = theme::palette(cx);
    let now = super::now();
    let active = view.active();
    let used = active.and_then(|row| row.weekly()?.used(now));
    let colour = panel::level(used, view.line(), p.text, p);
    let tip: SharedString = match (active, used) {
        (Some(row), Some(used)) => format!("{}: {used:.0}% of the week used", row.email),
        (Some(row), None) => row.email.to_string(),
        (None, _) => "Claude accounts".to_owned(),
    }
    .into();
    // The ring itself, as the composer's context ring is: a glass button's padding cuts it.
    div()
        .id("accounts-button")
        .debug_selector(|| "accounts-button".to_owned())
        .size(px(24.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.))
        .cursor_pointer()
        .hover(move |button| button.bg(p.hover))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        // A mask: it paints in its own text colour.
        .child(
            ring(used.map_or(0, |used| used.round() as u8))
                .size(px(16.))
                .text_color(colour),
        )
        .into_any_element()
}

impl Render for AccountsButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.accounts.read(cx);
        if accounts.running.is_none() {
            return div().into_any_element();
        }
        accounts
            .trigger
            .wraps(drawn(&accounts.view, cx), |_, window, cx| {
                cx.stop_propagation();
                super::accounts(cx).update(cx, |accounts, cx| accounts.toggle_panel(window, cx));
            })
            .into_any_element()
    }
}
