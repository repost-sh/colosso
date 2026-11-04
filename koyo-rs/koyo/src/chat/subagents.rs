//! A subagent's row in the timeline it was started from (`docs/chat.md` 5.3): how it's doing and
//! for how long, opening on its own chat; subagents one after the other under one line that says
//! how they're doing, open as long as any of them runs.

use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Svg, div, px, rgb, svg,
};
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, Subagent};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::RowId;
use koyo_agent::words::{subagent_line, subagent_status, subagent_used};

use super::rows::{Drawing, SHIMMER_FPS, chevron, disclosure};
use super::timeline::OpenSubagent;
use crate::shell::ellipsized;
use crate::theme::Palette;

const AVATAR: f32 = 26.;
const DOT: f32 = 8.;
/// Claude's little guy, in a grid where his pixels are 1 wide and 2 tall: head with two eyes, arms
/// out, four legs.
const CLAUDE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 18 10" fill="black"><path d="M3 0h12v2H3zM3 2h2v2H3zM6 2h6v2H6zM13 2h2v2h-2zM1 4h16v2H1zM3 6h12v2H3zM4 8h1v2H4zM6 8h1v2H6zM11 8h1v2h-1zM13 8h1v2h-1z"/></svg>"#;
/// His orange, which isn't the theme's to change.
const CLAUDE_ORANGE: u32 = 0x00D7_7757;
/// How many avatars a line of subagents stacks.
const STACKED: usize = 3;
/// How much of each avatar the next one covers.
const OVERLAP: f32 = 8.;

/// The subagents at `items`: the one's row, or a line that opens on a row for each.
pub(super) fn group(
    drawing: &Drawing,
    replica: &Replica,
    items: &[usize],
    id: &RowId,
    index: usize,
) -> AnyElement {
    let agents: Vec<(&Item, &Subagent)> = (items.iter())
        .filter_map(|item| replica.items.get(*item))
        .filter_map(|item| match &item.body {
            ItemBody::Subagent(agent) => Some((item, &**agent)),
            _ => None,
        })
        .collect();
    // One running on past its run moves nothing: for as long as it takes, the window isn't kept
    // drawing for its words, as the dot and the time say it's running.
    let moving = replica.live().is_some();
    match agents.as_slice() {
        [(item, agent)] => card(drawing, item, agent, moving).into_any_element(),
        several => line(drawing, several, moving, id, index).into_any_element(),
    }
}

fn line(
    drawing: &Drawing,
    agents: &[(&Item, &Subagent)],
    moving: bool,
    id: &RowId,
    index: usize,
) -> Div {
    let p = drawing.p;
    // Open until a person closes it, whatever they're doing by then.
    let open = !drawing.open.contains(id);
    let stacked = agents.iter().take(STACKED).enumerate().map(|(at, (item, _))| {
        avatar(item.status, p).when(at > 0, |avatar| avatar.ml(px(-OVERLAP)))
    });
    let more = agents.len().saturating_sub(STACKED);
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            disclosure(("chat-subagents", index), p)
                .gap_2()
                .child(div().flex().items_center().children(stacked))
                .when(more > 0, |row| row.child(format!("+{more}")))
                .child(format!("{} subagents · {}", agents.len(), how_they_are(agents)))
                .child(chevron(open))
                .on_click(drawing.opens(id.clone(), index)),
        )
        .when(open, |row| {
            row.children(agents.iter().map(|(item, agent)| card(drawing, item, agent, moving)))
        })
}

/// How subagents are doing together, what's worst first.
fn how_they_are(agents: &[(&Item, &Subagent)]) -> String {
    let count = |status| agents.iter().filter(|(item, _)| item.status == status).count();
    let said: Vec<String> = [
        (count(ItemStatus::Running) + count(ItemStatus::Pending), "running"),
        (count(ItemStatus::Failed), "failed"),
        (count(ItemStatus::Interrupted), "stopped"),
        (count(ItemStatus::Completed), "done"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, said)| format!("{count} {said}"))
    .collect();
    said.join(", ")
}

/// One subagent: what it's called, what it's doing or what it said, how it's doing and for how
/// long. It opens its chat, if it has one.
fn card(drawing: &Drawing, item: &Item, agent: &Subagent, moving: bool) -> Stateful<Div> {
    let p = drawing.p;
    let doing = subagent_status(item.status);
    let (started, ended) = (agent.started_at.as_ref(), agent.ended_at.as_ref());
    let status = subagent_line(doing, started, ended, drawing.now);
    let color = if item.status == ItemStatus::Failed { p.danger } else { p.muted };
    div()
        .id(SharedString::from(format!("subagent-{}", item.id)))
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .py_2()
        .rounded(px(10.))
        .border_1()
        .border_color(p.hairline)
        .bg(p.raised)
        .child(avatar(item.status, p))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(ellipsized(agent.description.clone()).text_sm().text_color(p.text))
                .children(detail(item, agent, moving, p)),
        )
        .child(div().flex_shrink_0().text_xs().text_color(color).child(status))
        .when_some(agent.thread_id.clone(), |row, thread| {
            row.cursor_pointer()
                .hover(|row| row.bg(p.hover))
                .child(div().flex_shrink_0().text_color(p.muted).child(chevron(false)))
                .on_click(drawing.on_timeline(move |_, _, cx| {
                    cx.emit(OpenSubagent(thread.clone()));
                }))
        })
}

/// The line under its name: what it's doing, or the start of what it said.
fn detail(item: &Item, agent: &Subagent, moving: bool, p: Palette) -> Option<Div> {
    let line = if is_running(item) {
        let doing = agent.progress.clone().unwrap_or_else(|| "Working…".to_owned());
        let used = agent.usage.as_ref().map(subagent_used);
        let says = used.map_or_else(|| doing.clone(), |used| format!("{doing} · {used}"));
        if moving {
            ShimmerText::new(says).max_fps(SHIMMER_FPS).into_any_element()
        } else {
            says.into_any_element()
        }
    } else {
        let said = agent.summary.as_deref()?.lines().find(|line| !line.trim().is_empty())?;
        said.trim().to_owned().into_any_element()
    };
    Some(ellipsized(line).text_xs().text_color(p.muted))
}

/// Claude's little guy on a round mark, with a dot that says how it's doing.
pub(super) fn avatar(status: ItemStatus, p: Palette) -> Div {
    div()
        .relative()
        .flex_shrink_0()
        .size(px(AVATAR))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(p.hairline)
        .bg(p.sunken)
        .child(little_guy())
        .child(
            div()
                .absolute()
                .right(px(-1.))
                .bottom(px(-1.))
                .size(px(DOT))
                .rounded_full()
                .bg(dot(status, p)),
        )
}

fn little_guy() -> Svg {
    svg().data(CLAUDE.as_bytes()).w(px(18.)).h(px(10.)).text_color(rgb(CLAUDE_ORANGE))
}

fn dot(status: ItemStatus, p: Palette) -> Hsla {
    match status {
        ItemStatus::Pending | ItemStatus::Running => p.text,
        ItemStatus::Completed => p.added,
        ItemStatus::Failed => p.danger,
        ItemStatus::Declined | ItemStatus::Interrupted => p.muted,
    }
}

fn is_running(item: &Item) -> bool {
    matches!(item.status, ItemStatus::Running | ItemStatus::Pending)
}
