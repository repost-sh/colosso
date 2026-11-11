//! The accounts' button (`docs/claude-accounts.md` 6), in the room at the left of the Arc
//! sidebar's foot that keeps the space marks centred against the + on the right. It shows the
//! active account's week as a ring, on the same glass as the +, and its press opens the panel
//! over it. A view of its own, so a new reading redraws it and not the shell. With Koyo not
//! managing the accounts, it's nothing.

use gpui_kit::component::Sizable as _;
use gpui_kit::{
    AnyElement, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _,
    Subscription, Window, div, px,
};
use koyo_agent::words::NEARLY_FULL;

use super::Accounts;
use crate::ring::ring;
use crate::shell::glass;
use crate::theme;

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

impl Render for AccountsButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.accounts.read(cx);
        if accounts.running.is_none() {
            return div().into_any_element();
        }
        let p = theme::palette(cx);
        let now = super::now();
        let active = accounts.view.active();
        let used = active.and_then(|row| row.weekly()?.used(now));
        let colour = match used {
            Some(used) if used >= f64::from(NEARLY_FULL) => p.danger,
            Some(_) => p.text,
            None => p.muted,
        };
        let tip = match (active, used) {
            (Some(row), Some(used)) => format!("{}: {used:.0}% of the week used", row.email),
            (Some(row), None) => row.email.to_string(),
            (None, _) => "Claude accounts".to_owned(),
        };
        let shown: AnyElement = glass("accounts-button", colour, cx)
            .small()
            .tooltip(tip)
            .child(ring(used.map_or(0, |used| used.round() as u8)).size(px(16.)))
            .into_any_element();
        accounts
            .trigger
            .wraps(shown, |_, window, cx| {
                cx.stop_propagation();
                super::accounts(cx).update(cx, |accounts, cx| accounts.toggle_panel(window, cx));
            })
            .into_any_element()
    }
}
