//! A timeline row of work (`docs/chat.md` 5.3): tools one after the other as one line of what
//! they did, each of them a line that opens on what it gave back.

use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, Div, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, Todo, TodoStatus, Tool, ToolFamily};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::RowId;
use koyo_agent::words::{THOUGHT, work_line, work_summary};

use super::output::{block, diff};
use super::rows::{Drawing, chevron, disclosure};
use crate::shell::{ellipsized, icon};
use crate::theme::Palette;

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
    let open = drawing.work_open(id);
    let failed = lines.iter().any(|item| item.status == ItemStatus::Failed);
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            disclosure(("chat-work", index), p)
                .child(work_summary(lines.iter().copied()))
                .when(failed, |row| {
                    row.child(div().text_color(p.danger).child(icon(IconName::CircleX, 12.)))
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

/// One item of work: what it did, and what it gave back once opened.
fn line(drawing: &Drawing, item: &Item, index: usize) -> Div {
    let p = drawing.p;
    let id = RowId::Item(item.id.clone());
    let opens_up = has_details(item);
    let open = opens_up && drawing.work_open(&id);
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
    let head = div()
        .id(SharedString::from(format!("work-{}", item.id)))
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .text_sm()
        .text_color(color)
        .child(div().flex_shrink_0().child(icon(mark_of(item), 13.)))
        .child(ellipsized(says))
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
        .when(open, |row| row.children(details(drawing, item)))
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

fn has_details(item: &Item) -> bool {
    match &item.body {
        ItemBody::Tool(tool) => {
            let said = tool.output.as_deref().is_some_and(|output| !output.trim().is_empty());
            said || tool.patch.is_some() || tool.family == ToolFamily::Mcp
        }
        ItemBody::Reasoning(text) => !text.text.trim().is_empty(),
        ItemBody::Todos(_) => true,
        _ => false,
    }
}

fn details(drawing: &Drawing, item: &Item) -> Option<AnyElement> {
    let p = drawing.p;
    let inner = match &item.body {
        ItemBody::Tool(tool) => tool_details(&item.id, tool, p).into_any_element(),
        ItemBody::Reasoning(text) => div()
            .text_sm()
            .text_color(p.muted)
            .child(text.text.trim().to_owned())
            .into_any_element(),
        ItemBody::Todos(todos) => todo_list(&todos.todos, p).into_any_element(),
        _ => return None,
    };
    Some(div().pl(px(21.)).child(inner).into_any_element())
}

fn tool_details(id: &ItemId, tool: &Tool, p: Palette) -> Div {
    let input = (tool.family == ToolFamily::Mcp)
        .then(|| serde_json::to_string_pretty(&tool.input).ok())
        .flatten();
    let output = tool.output.as_deref().filter(|_| tool.patch.is_none());
    div()
        .flex()
        .flex_col()
        .gap_1()
        .children(input.map(|input| block(format!("input-{id}"), &input, p)))
        .children(tool.patch.as_ref().map(|patch| diff(id, patch, p)))
        .children(output.map(|output| block(format!("output-{id}"), output, p)))
        .when_some(tool.exit_code.filter(|code| *code != 0), |row, code| {
            row.child(
                div()
                    .text_xs()
                    .text_color(p.danger)
                    .child(format!("Exited with {code}")),
            )
        })
}

fn todo_list(todos: &[Todo], p: Palette) -> Div {
    let rows = todos.iter().map(|todo| {
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
            .child(todo.text.clone())
    });
    div()
        .flex()
        .flex_col()
        .gap(px(3.))
        .text_sm()
        .children(rows)
}
