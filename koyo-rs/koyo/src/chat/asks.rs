//! What the agent waits on a person for, in its place in the timeline (`docs/chat.md` 5.3): what's
//! asked, and the buttons that answer it. They say so to the composer, as its keys do, since an
//! answer is one more thing a person says to the chat, and only what's asked now can be answered.
//! A question and a plan are answered in the composer's drawers; what shows here is what they ask
//! and what became of them.

use std::path::Path;

use gpui_kit::assets::IconName;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyElement, App, ClickEvent, Div, FontWeight, IntoElement, KeyBinding,
    ParentElement as _, SharedString, Styled as _, Window, actions, div, px,
};
use koyo_agent::protocol::ids::{ItemId, RequestId};
use koyo_agent::protocol::requests::{Approval, Questions, Request, RequestPayload, RequestStatus};
use koyo_agent::timeline::RowId;
use koyo_agent::words::{approval_line, plan_outcome, plan_parts, question_lines};

use super::marks::marked;
use super::output::{block, change};
use super::rows::Drawing;
use super::work::tool_mark;
use crate::files::relative;
use crate::menu::{self, MenuItem};
use crate::shell::{ellipsized, glass, icon, icon_button, solid};
use crate::theme::Palette;

actions!(chat, [AllowRequest, AllowRequestForChat, DenyRequest, StopRequest]);

/// Asks where to keep the plan of this request.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct SavePlan(pub(super) RequestId);

/// The composer's context while the keys answer what's asked: nothing's typed, and an approval
/// waits. They do what they always do otherwise.
pub(super) const ANSWERABLE: &str = "ChatAnswerable";

/// How many lines of a plan show until a person asks for the rest.
const PLAN_LINES: usize = 20;

pub(super) fn bind_keys(cx: &mut App) {
    let context = format!("{ANSWERABLE} > Input");
    let context = Some(context.as_str());
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
    item: &ItemId,
    row: &RowId,
    index: usize,
    cx: &App,
) -> Option<AnyElement> {
    let p = drawing.p;
    let drawn = match &request.payload {
        RequestPayload::Approval(approval) if request.status == RequestStatus::Pending => {
            card(drawing, approval, &request.id.0, asking, index, cx).into_any_element()
        }
        RequestPayload::Approval(approval) => marked(tool_mark(approval.family), p.muted)
            .child(ellipsized(approval_line(request, approval, &drawing.cwd)))
            .into_any_element(),
        RequestPayload::Question(asked) => question(drawing, request, asked),
        RequestPayload::Plan { markdown } => plan(drawing, request, markdown, item, row, index, cx)?,
        RequestPayload::Unknown(_) => div()
            .text_sm()
            .text_color(p.muted)
            .child("Something this version of Koyo can't show")
            .into_any_element(),
    };
    Some(drawn)
}

/// What a question asked, or what was said to it. The drawer asks it while it's live; one the agent
/// stopped waiting on is answered by the next message, so its options are said here.
fn question(drawing: &Drawing, request: &Request, asked: &Questions) -> AnyElement {
    let p = drawing.p;
    let lines = question_lines(request, &asked.questions);
    let waits_for_message = request.status == RequestStatus::Pending && !asked.live;
    let options = (asked.questions.iter().filter(|_| waits_for_message)).map(|question| {
        let labels: Vec<&str> = question.options.iter().map(|option| option.label.as_str()).collect();
        div().pl_5().text_xs().child(labels.join(" · "))
    });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_color(p.muted)
        .children(lines.into_iter().map(|said| {
            marked(IconName::CircleQuestionMark, p.muted).child(ellipsized(said))
        }))
        .children(options)
        .into_any_element()
}

/// A plan, to read and to keep. It's answered in the drawer above the composer, while it's asked.
fn plan(
    drawing: &Drawing,
    request: &Request,
    markdown: &str,
    item: &ItemId,
    row: &RowId,
    index: usize,
    cx: &App,
) -> Option<AnyElement> {
    let p = drawing.p;
    let text = drawing.thread.read(cx).drawn_text(item)?;
    let (title, body) = plan_parts(markdown);
    // Past so many lines with something on them, each at least a line tall, there's always more
    // to show. It's only ever cut where it says so.
    let long = body.lines().filter(|line| !line.trim().is_empty()).nth(PLAN_LINES).is_some();
    let open = drawing.open.contains(row);
    let copy = icon_button(("chat-plan-copy", index), drawing.copy_mark(item), "Copy", cx)
        .small()
        .on_click(drawing.copies(item));
    let save = icon_button(("chat-plan-save", index), IconName::Download, "Save as…", cx)
        .small()
        .on_click(says(drawing, Box::new(SavePlan(request.id.clone()))));
    let heading = div()
        .flex()
        .items_center()
        .gap_2()
        .text_xs()
        .text_color(p.muted)
        .child(icon(IconName::ListChecks, 13.))
        .child("Plan")
        .children(plan_outcome(request).map(|outcome| format!("· {outcome}")))
        .child(div().flex_1())
        .child(copy)
        .child(save);
    let card = frame(p, request.status == RequestStatus::Pending)
        .child(heading)
        .children(title.map(|title| div().font_weight(FontWeight::SEMIBOLD).child(title.to_owned())))
        .child(
            div()
                .text_sm()
                .child(TextView::new(text).when(long && !open, |text| text.max_lines(PLAN_LINES))),
        )
        .when(long, |card| card.child(drawing.show_more(row.clone(), index, open)));
    Some(card.into_any_element())
}

/// The box around what's asked, which stands out while it waits.
fn frame(p: Palette, waiting: bool) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded(px(12.))
        .border_1()
        .border_color(p.hairline)
        .when(waiting, |frame| {
            frame.border_color(p.text.opacity(0.3)).bg(p.raised)
        })
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
    frame(p, true)
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

/// A click that says `action` to the composer.
pub(super) fn says(
    drawing: &Drawing,
    action: Box<dyn Action>,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    let composer = drawing.composer.clone();
    move |_, window, cx| composer.dispatch_action(&*action, window, cx)
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
    let deny = glass(("chat-deny", index), p.text, cx)
        .small()
        .label("Deny")
        .tooltip("⌘⌫ with nothing typed, or type why and send")
        .disabled(!asking)
        .on_click(says(drawing, Box::new(DenyRequest)));
    let allow = solid(("chat-allow", index), asking, cx)
        .small()
        .label("Allow")
        .tooltip("⌘↩ with nothing typed")
        .on_click(says(drawing, Box::new(AllowRequest)));
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
