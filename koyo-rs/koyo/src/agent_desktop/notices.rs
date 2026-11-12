//! What the agent desktop's tab says over the desktop (`docs/agent-desktop.md` 9), each with
//! the one thing to do about it: an app of the person's own that is lent to agents, with
//! the way to take it back, and an app of the agent's that the person seems to have asked
//! for, with the way to their own.

use std::time::Duration;

use gpui_kit::{
    ClickEvent, Context, InteractiveElement as _, IntoElement, ParentElement as _, Styled as _,
    div, px,
};
use koyo_desk_wire::link::{Lent, ToHost};

use super::Desk;
use super::view::DeskView;
use crate::modal::{Button as Push, Tone};
use crate::theme;

/// How long the notice of an app asked for stays up.
const WANTED_FOR: Duration = Duration::from_secs(60);

impl Desk {
    /// The person pressed "Take It Back": the host gives their app back to them, in front on
    /// the desktop they're on. Gone from the notice at once; the host says what is lent still.
    pub fn take_back(&mut self, pid: i32, cx: &mut Context<Self>) {
        self.tell(ToHost::TakeBack { pid });
        self.lent.retain(|lent| lent.pid != pid);
        cx.notify();
    }
}

impl DeskView {
    /// The notices, one under the other at the top of the tab.
    pub(super) fn notices(&self, cx: &mut Context<Self>) -> Option<gpui_kit::Div> {
        let desk = self.desk.read(cx);
        let lent = desk.lent.clone();
        let wanted = (desk.wanted.as_ref())
            .filter(|wanted| wanted.at.elapsed() < WANTED_FOR)
            .map(|wanted| wanted.app.clone());
        if lent.is_empty() && wanted.is_none() {
            return None;
        }
        let mut notices = div()
            .absolute()
            .top_2()
            .left_0()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_2();
        // An app that is the person's own, on the agent desktop on their word.
        for Lent { pid, app } in lent {
            let said = format!(
                "{app} is yours, lent to agents: its windows are here until it's given back."
            );
            let back = cx.listener(move |this, _: &ClickEvent, _, cx| {
                this.desk.update(cx, |desk, cx| desk.take_back(pid, cx));
            });
            let push =
                Push::new("agent-desktop-take-back", "Take It Back", Tone::Primary).on_click(back);
            let row = usize::try_from(pid).unwrap_or_default();
            notices = notices.child(
                div()
                    .id(("agent-desktop-lent", row))
                    .child(notice(said, push, cx)),
            );
        }
        // An app the agent runs that was asked for, by the person it seems: nothing is
        // started for them on a guess.
        if let Some(app) = wanted {
            let said = format!(
                "{app} came to the front: if you opened it, the one running is the agent's."
            );
            let open = cx.listener(|this, _: &ClickEvent, _, cx| {
                this.desk.update(cx, Desk::open_for_person);
            });
            let push = Push::new("agent-desktop-yours", "Open Yours", Tone::Primary).on_click(open);
            notices = notices.child(notice(said, push, cx));
        }
        Some(notices)
    }
}

/// One notice: what it says, and the button for the thing to do about it.
fn notice(said: String, push: Push, cx: &Context<DeskView>) -> impl IntoElement {
    let p = theme::palette(cx);
    div()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(8.))
        .bg(p.raised)
        .border_1()
        .border_color(p.hairline)
        .text_sm()
        .text_color(p.text)
        .occlude()
        .child(said)
        .child(push)
}
