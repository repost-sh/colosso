//! A timeline row of work (`docs/chat.md` 5.3): tools one after the other as one line of what
//! they did, each of them a line that opens on what it gave back; or a step of a kind the person
//! sees more of, on a row of its own with as much of what it gave back as they chose.

use gpui_kit::assets::IconName;
use gpui_kit::base::SelectableText;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled, div, px,
};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::{
    Hunk, Item, ItemBody, ItemStatus, Todo, TodoStatus, Tool, ToolFamily,
};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::RowId;
use koyo_agent::words::{THOUGHT, work_line, work_summary};

use super::output::{block, diff, patch_counts};
use super::rows::{Drawing, chevron, disclosure, word_button};
use crate::changes::counts;
use crate::settings::{Shows, Step, Verbosity};
use crate::shell::{ellipsized, icon};
use crate::theme::Palette;

/// How many lines of what a tool gave back show of it until it's opened.
const PREVIEW_LINES: usize = 4;

/// The kind of step an item of work is, for how much of it shows.
pub(super) fn step_of(item: &Item) -> Option<Step> {
    match &item.body {
        ItemBody::Tool(tool) => Some(match tool.family {
            ToolFamily::Edit => Step::Changes,
            ToolFamily::Command => Step::Commands,
            ToolFamily::Todo => Step::Todos,
            _ => Step::Results,
        }),
        ItemBody::Reasoning(_) => Some(Step::Thinking),
        ItemBody::Todos(_) => Some(Step::Todos),
        _ => None,
    }
}

/// How much of an item of work shows until a person opens or closes it: what's chosen for its
/// kind, or its line if it's of none.
pub(super) fn chosen(item: &Item, verbosity: Verbosity) -> Shows {
    step_of(item).map_or(Shows::Line, |step| verbosity.shows(step))
}

/// Whether an item of work has a row of its own: it's of a kind that shows more than its line,
/// which reads in the order it came rather than behind a summary of the work beside it.
pub(super) fn stands_alone(item: &Item, verbosity: Verbosity) -> bool {
    chosen(item, verbosity) != Shows::Line
}

/// The work at `items`: its one line, or a summary that opens on one line for each.
pub(super) fn group(
    drawing: &Drawing,
    replica: &Replica,
    items: &[usize],
    id: &RowId,
    index: usize,
) -> AnyElement {
    let lines: Vec<&Item> = (items.iter())
        .filter_map(|item| replica.items.get(*item))
        .collect();
    match lines.as_slice() {
        [only] => line(drawing, only, index).into_any_element(),
        several => summary(drawing, several, id, index).into_any_element(),
    }
}

fn summary(drawing: &Drawing, lines: &[&Item], id: &RowId, index: usize) -> Div {
    let p = drawing.p;
    let open = drawing.open.contains(id);
    let failed = lines.iter().any(|item| item.status == ItemStatus::Failed);
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            disclosure(("chat-work", index), p)
                .child(work_summary(lines.iter().copied()))
                .when(failed, |row| {
                    row.child(
                        div()
                            .text_color(p.danger)
                            .child(icon(IconName::CircleX, 12.)),
                    )
                })
                .child(chevron(open))
                .on_click(drawing.opens(id.clone(), index)),
        )
        .when(open, |row| {
            row.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .pl_3()
                    .border_l_1()
                    .border_color(p.hairline)
                    .children(lines.iter().map(|item| line(drawing, item, index))),
            )
        })
}

/// One item of work: what it did, and as much of what it gave back as shows.
fn line(drawing: &Drawing, item: &Item, index: usize) -> Div {
    let p = drawing.p;
    let id = RowId::Item(item.id.clone());
    let opens_up = has_details(item);
    let shows = drawing.shows(item, &id);
    let open = opens_up && shows != Shows::Line;
    let opens = drawing.opens(id, index);
    let says = match &item.body {
        // The line is the thought's last, which what's opened under it says again.
        ItemBody::Reasoning(_) if open => THOUGHT.to_owned(),
        _ => work_line(item, &drawing.cwd),
    };
    let color = if item.status == ItemStatus::Failed {
        p.danger
    } else {
        p.muted
    };
    let thought = matches!(item.body, ItemBody::Reasoning(_));
    let head = div()
        .id(SharedString::from(format!("work-{}", item.id)))
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .text_sm()
        .text_color(color)
        .child(div().flex_shrink_0().child(icon(mark_of(item), 13.)))
        .child(
            ellipsized(SelectableText::new(
                SharedString::from(format!("work-says-{}", item.id)),
                says,
            ))
            // What it thought reads as an aside, in italics.
            .when(thought, Styled::italic),
        )
        .children(changed(item, p))
        .when(item.status == ItemStatus::Declined, |row| {
            row.child(div().flex_shrink_0().text_xs().child("Declined"))
        })
        .when(opens_up, |row| {
            row.cursor_pointer()
                .hover(|row| row.text_color(p.text))
                .child(div().flex_shrink_0().child(chevron(open)))
                .on_click(opens)
        });
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(head)
        .when(open, |row| {
            row.children(details(drawing, item, shows, index))
        })
}

/// How many lines an edit added and removed, beside its line.
fn changed(item: &Item, p: Palette) -> Option<Div> {
    let ItemBody::Tool(tool) = &item.body else {
        return None;
    };
    let (added, removed) = patch_counts(patch_of(tool)?);
    (added + removed > 0).then(|| counts(added, removed, p))
}

fn mark_of(item: &Item) -> IconName {
    match &item.body {
        ItemBody::Tool(tool) => tool_mark(tool.family),
        ItemBody::Reasoning(_) => IconName::Brain,
        ItemBody::Todos(_) => IconName::ListTodo,
        _ => IconName::Wrench,
    }
}

pub(super) fn tool_mark(family: ToolFamily) -> IconName {
    match family {
        ToolFamily::Command => IconName::SquareTerminal,
        ToolFamily::Read => IconName::FileText,
        ToolFamily::Edit => IconName::FilePen,
        ToolFamily::Search => IconName::Search,
        ToolFamily::Web => IconName::Globe,
        ToolFamily::Mcp => IconName::Plug,
        ToolFamily::Agent => IconName::Bot,
        ToolFamily::Todo => IconName::ListTodo,
        ToolFamily::Skill => IconName::Sparkles,
        ToolFamily::Other => IconName::Wrench,
    }
}

/// A change it made, if it made one.
pub(super) fn patch_of(tool: &Tool) -> Option<&[Hunk]> {
    tool.patch.as_deref().filter(|patch| !patch.is_empty())
}

fn has_details(item: &Item) -> bool {
    match &item.body {
        ItemBody::Tool(tool) => {
            let said = tool
                .output
                .as_deref()
                .is_some_and(|output| !output.trim().is_empty());
            said || patch_of(tool).is_some() || tool.family == ToolFamily::Mcp
        }
        ItemBody::Reasoning(text) => !text.text.trim().is_empty(),
        ItemBody::Todos(_) => true,
        _ => false,
    }
}

fn details(drawing: &Drawing, item: &Item, shows: Shows, index: usize) -> Option<AnyElement> {
    let p = drawing.p;
    let inner = match &item.body {
        ItemBody::Tool(tool) => {
            tool_details(drawing, &item.id, tool, shows, index).into_any_element()
        }
        ItemBody::Reasoning(text) => div()
            .text_sm()
            .italic()
            .text_color(p.muted)
            .child(SelectableText::new(
                SharedString::from(format!("thought-{}", item.id)),
                text.text.trim().to_owned(),
            ))
            .into_any_element(),
        ItemBody::Todos(todos) => todo_list(&item.id, &todos.todos, p).into_any_element(),
        _ => return None,
    };
    Some(div().pl(px(21.)).child(inner).into_any_element())
}

/// What a tool gave back: its diff, or what it printed, all of it or a few lines; and what an MCP
/// tool was given, once all of it shows.
fn tool_details(drawing: &Drawing, id: &ItemId, tool: &Tool, shows: Shows, index: usize) -> Div {
    let p = drawing.p;
    let all = shows == Shows::All;
    let input = (all && tool.family == ToolFamily::Mcp)
        .then(|| serde_json::to_string_pretty(&tool.input).ok())
        .flatten();
    let patch = patch_of(tool);
    let output =
        (tool.output.as_deref()).filter(|output| patch.is_none() && !output.trim().is_empty());
    let colours = drawing.colours.get(id).map(AsRef::as_ref);
    let last = tool.family == ToolFamily::Command;
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(input.map(|input| block(format!("input-{id}"), &input, p)))
        .children(patch.map(|patch| diff(id, patch, colours, p)))
        .children(output.map(|output| {
            if all {
                block(format!("output-{id}"), output, p).into_any_element()
            } else {
                preview(drawing, id, output, last, index).into_any_element()
            }
        }))
        .when_some(tool.exit_code.filter(|code| *code != 0), |row, code| {
            row.child(
                div()
                    .text_xs()
                    .text_color(p.danger)
                    .child(format!("Exited with {code}")),
            )
        })
}

/// A few lines of what a tool gave back, and a line that says how many more there are and opens
/// all of it: a command's last lines, as they say how it went, or any other tool's first.
fn preview(drawing: &Drawing, id: &ItemId, output: &str, last: bool, index: usize) -> Div {
    let p = drawing.p;
    let output = output.trim_end();
    let hidden = output.lines().count().saturating_sub(PREVIEW_LINES);
    let boxed = |text: &str| block(format!("output-{id}"), text, p);
    // A line more than shows isn't worth a line saying so.
    if hidden <= 1 {
        return div().child(boxed(output));
    }
    // The lines kept are of a piece: what's after the line breaks before the last of them, or
    // before the one after the first of them.
    let kept = if last {
        let breaks = output.rmatch_indices('\n').nth(PREVIEW_LINES - 1);
        breaks.map_or(output, |(at, _)| &output[at + 1..])
    } else {
        let breaks = output.match_indices('\n').nth(PREVIEW_LINES - 1);
        breaks.map_or(output, |(at, _)| &output[..at])
    };
    let more = word_button(
        SharedString::from(format!("more-{id}")),
        format!("… +{hidden} lines"),
        p,
    )
    .on_click(drawing.opens(RowId::Item(id.clone()), index));
    let (above, below) = if last {
        (Some(more), None)
    } else {
        (None, Some(more))
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(above)
        .child(boxed(kept))
        .children(below)
}

fn todo_list(id: &ItemId, todos: &[Todo], p: Palette) -> Div {
    let rows = todos.iter().enumerate().map(|(at, todo)| {
        let (mark, color) = match todo.status {
            TodoStatus::Completed => (IconName::CircleCheck, p.muted),
            TodoStatus::InProgress => (IconName::LoaderCircle, p.text),
            TodoStatus::Pending => (IconName::Circle, p.muted),
        };
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_color(color)
            .child(icon(mark, 13.))
            .child(SelectableText::new(
                SharedString::from(format!("todo-{id}-{at}")),
                todo.text.clone(),
            ))
    });
    div().flex().flex_col().gap(px(3.)).text_sm().children(rows)
}
