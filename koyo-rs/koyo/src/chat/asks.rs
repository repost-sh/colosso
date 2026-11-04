//! What the agent waits on a person for, in its place in the timeline (`docs/chat.md` 5.3): what's
//! asked, and the buttons that answer it. They say so to the composer, as its keys do, since an
//! answer is one more thing a person says to the chat, and only what's asked now can be answered.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, Div, IntoElement, KeyBinding, ParentElement as _,
    SharedString, Styled as _, Window, actions, div, px,
};
use koyo_agent::protocol::requests::{Approval, Request, RequestPayload, RequestStatus};
use koyo_agent::words::approval_line;

use super::marks::marked;
use super::output::{block, change};
use super::rows::Drawing;
use super::work::tool_mark;
use crate::files::relative;
use crate::menu::{self, MenuItem};
use crate::shell::{ellipsized, glass, icon, icon_button};

actions!(chat, [AllowRequest, AllowRequestForChat, DenyRequest, StopRequest]);

/// The composer's context while the keys answer what's asked: nothing's typed, and an approval
/// waits. They do what they always do otherwise.
pub(super) const ANSWERABLE: &str = "ChatAnswerable";

pub(super) fn bind_keys(cx: &mut App) {
    let context = Some("ChatAnswerable > Input");
    cx.bind_keys([
        KeyBinding::new("cmd-enter", AllowRequest, context),
        KeyBinding::new("cmd-alt-enter", AllowRequestForChat, context),
        KeyBinding::new("cmd-backspace", DenyRequest, context),
    ]);
}

/// A request as it stands: what it asks while it waits, and how it ended once it doesn't.
pub(super) fn request(
    drawing: &Drawing,
    request: &Request,
    asking: bool,
    index: usize,
    cx: &App,
) -> AnyElement {
    let p = drawing.p;
    match &request.payload {
        RequestPayload::Approval(approval) if request.status == RequestStatus::Pending => {
            card(drawing, approval, &request.id.0, asking, index, cx).into_any_element()
        }
        RequestPayload::Approval(approval) => marked(tool_mark(approval.family), p.muted)
            .child(ellipsized(approval_line(request, approval, &drawing.cwd)))
            .into_any_element(),
        RequestPayload::Question(_) => line(drawing, "A question"),
        RequestPayload::Plan { markdown } => {
            line(drawing, markdown.lines().next().unwrap_or("A plan"))
        }
        RequestPayload::Unknown(_) => line(drawing, "Something this version of Koyo can't show"),
    }
}

fn line(drawing: &Drawing, text: &str) -> AnyElement {
    div().text_sm().text_color(drawing.p.muted).child(text.to_owned()).into_any_element()
}

fn card(
    drawing: &Drawing,
    approval: &Approval,
    id: &str,
    asking: bool,
    index: usize,
    cx: &App,
) -> Div {
    let p = drawing.p;
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(px(12.))
        .border_1()
        .border_color(p.accent.opacity(0.4))
        .bg(p.accent.opacity(0.04))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_sm()
                .child(icon(tool_mark(approval.family), 14.))
                .child(approval.title.clone()),
        )
        .children(asked(drawing, approval, id))
        .when_some(approval.reason.clone(), |card, reason| {
            card.child(div().text_xs().text_color(p.muted).child(reason))
        })
        .child(buttons(drawing, approval, asking, index, cx))
}

/// What's asked, as is: the change of an edit, under the file it's to, or else the text.
fn asked(drawing: &Drawing, approval: &Approval, id: &str) -> Option<AnyElement> {
    let (p, box_id) = (drawing.p, format!("asked-{id}"));
    match &approval.change {
        Some(lines) => {
            let cwd = Path::new(&*drawing.cwd);
            let file = relative(cwd, Path::new(&approval.detail));
            let shown = div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().text_color(p.muted).child(file))
                .child(change(box_id, lines, p));
            Some(shown.into_any_element())
        }
        None if approval.detail.is_empty() => None,
        None => Some(block(box_id, &approval.detail, p).into_any_element()),
    }
}

/// Deny · Allow and a menu of the rest. They wait while an older request is asked.
fn buttons(
    drawing: &Drawing,
    approval: &Approval,
    asking: bool,
    index: usize,
    cx: &App,
) -> impl IntoElement {
    let p = drawing.p;
    let says = |action: Box<dyn Action>| {
        let composer = drawing.composer.clone();
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            composer.dispatch_action(&*action, window, cx);
        }
    };
    let deny = glass(("chat-deny", index), p.text, cx)
        .small()
        .label("Deny")
        .tooltip("⌘⌫ with nothing typed, or type why and send")
        .disabled(!asking)
        .on_click(says(Box::new(DenyRequest)));
    let allow = Button::new(("chat-allow", index))
        .primary()
        .small()
        .label("Allow")
        .tooltip("⌘↩ with nothing typed")
        .disabled(!asking)
        .on_click(says(Box::new(AllowRequest)));
    let more = icon_button(("chat-allow-more", index), IconName::ChevronDown, "More answers", cx)
        .small()
        .disabled(!asking);
    let row = div()
        .flex()
        .items_center()
        .justify_end()
        .gap_1()
        .child(deny)
        .child(allow);
    if asking {
        let for_chat: Option<SharedString> =
            (approval.allow_session.as_ref()).map(|adds| adds.label.clone().into());
        row.child(menu::dropdown(
            ("chat-allow-more-menu", index),
            more,
            drawing.composer.clone(),
            move || answers_past_allow(for_chat.clone()),
        ))
    } else {
        row.child(more)
    }
}

/// Allowing for the chat, in what the host says that adds, when it can be; and stopping.
pub(super) fn answers_past_allow(for_chat: Option<SharedString>) -> Vec<MenuItem> {
    let allow = for_chat.map(|label| MenuItem {
        icon: IconName::Check,
        label,
        action: Box::new(AllowRequestForChat),
    });
    let stop = MenuItem {
        icon: IconName::CircleX,
        label: "Stop".into(),
        action: Box::new(StopRequest),
    };
    allow.into_iter().chain([stop]).collect()
}
