//! The card's subagents: those working now, each going into its own chat on a click, and those
//! done with behind a line that opens them.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    ClickEvent, Context, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, Subagent};
use koyo_agent::replica::Replica;
use koyo_agent::words::{subagent_line, subagent_status};

use super::super::ChatView;
use super::{clickable, row};
use crate::chat::rows::chevron;
use crate::chat::subagents::avatar;
use crate::shell::ellipsized;
use crate::theme::Palette;

/// The subagents working now, then those done with, behind a line.
pub(super) fn section(
    view: &ChatView,
    replica: Option<&Replica>,
    p: Palette,
    cx: &Context<ChatView>,
) -> Option<Div> {
    let agents: Vec<(&Item, &Subagent)> = (replica?.items.iter())
        .filter_map(|item| match &item.body {
            ItemBody::Subagent(agent) => Some((item, &**agent)),
            _ => None,
        })
        .collect();
    if agents.is_empty() {
        return None;
    }
    let (working, done): (Vec<_>, Vec<_>) =
        (agents.into_iter()).partition(|(item, _)| item.status.is_open());
    let open = view.details.previous;
    let line = row(IconName::Bot, p)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child(format!("Previous agents · {}", done.len())),
        )
        .child(div().text_color(p.muted).child(chevron(open)));
    let toggle = clickable(line, "chat-details-previous", p).on_click(cx.listener(
        |view, _: &ClickEvent, _, cx| {
            view.details.previous ^= true;
            cx.notify();
        },
    ));
    Some(
        div()
            .flex()
            .flex_col()
            .children(
                working
                    .iter()
                    .map(|(item, agent)| subagent(item, agent, p, cx)),
            )
            .when(!done.is_empty(), |list| list.child(toggle))
            .when(open, |list| {
                list.children(
                    done.iter()
                        .map(|(item, agent)| subagent(item, agent, p, cx)),
                )
            }),
    )
}

fn subagent(item: &Item, agent: &Subagent, p: Palette, cx: &Context<ChatView>) -> Stateful<Div> {
    let status = subagent_line(
        subagent_status(item.status),
        agent.started_at.as_ref(),
        agent.ended_at.as_ref(),
        chrono::Utc::now(),
    );
    let color = if item.status == ItemStatus::Failed {
        p.danger
    } else {
        p.muted
    };
    let line = div()
        .flex()
        .items_center()
        .gap(px(8.))
        .min_h(px(36.))
        .px(px(8.))
        .rounded(px(8.))
        .child(avatar(item.status, p))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(ellipsized(agent.description.clone()).text_color(p.text))
                .child(ellipsized(status).text_xs().text_color(color)),
        );
    let id = SharedString::from(format!("chat-details-subagent-{}", item.id));
    let thread: Option<ThreadId> = agent.thread_id.clone();
    line.id(id).when_some(thread, |line, thread| {
        line.cursor_pointer()
            .hover(move |line| line.bg(p.hover))
            .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                view.open_subagent(&thread, window, cx);
            }))
    })
}
