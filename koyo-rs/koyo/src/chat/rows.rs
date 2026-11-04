//! A timeline row as it's drawn (`docs/chat.md` 5.3): what the derivation said it is, from what the
//! thread holds now. What each row can open is kept by the timeline, not here.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Div, ElementId, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, WeakEntity, Window, div, px, relative,
};
use koyo_agent::protocol::ids::ItemId;
use koyo_agent::protocol::model::{Delivered, Item, ItemBody, Run};
use koyo_agent::protocol::requests::RequestPayload;
use koyo_agent::timeline::{Row, RowId, RowKind};
use koyo_agent::words::{answer_meta, clock, duration, live_line, worked_for};

use super::thread::ChatThread;
use super::timeline::Timeline;
use super::view::COLUMN;
use super::work;
use crate::pane::tab_button;
use crate::shell::{ellipsized, icon};
use crate::theme::Palette;

/// The room over the first row.
const TOP: f32 = 16.;
/// How many lines of a message show until it's opened.
const COLLAPSED_LINES: usize = 8;
/// About how many characters a line of a bubble holds, to tell a long message without laying it
/// out. A guess only says whether "Show more" is offered: it never hides what can't be opened.
const BUBBLE_LINE: usize = 72;
/// How tall a line of a bubble is, which a collapsed one is cut to a number of.
const BUBBLE_LINE_HEIGHT: f32 = 24.;
/// How often the live row's highlight moves: slow enough for the rows it redraws not to matter.
const SHIMMER_FPS: f32 = 20.;

/// What the list's rows are drawn from.
pub(super) struct Drawing {
    pub(super) thread: Entity<ChatThread>,
    pub(super) timeline: WeakEntity<Timeline>,
    pub(super) rows: Rc<Vec<Row>>,
    pub(super) open: Rc<HashSet<RowId>>,
    pub(super) running_since: Option<Instant>,
    pub(super) copied: Option<ItemId>,
    pub(super) cwd: Rc<str>,
    /// The room under the last row.
    pub(super) inset: Pixels,
    pub(super) p: Palette,
}

impl Drawing {
    /// The row at `index`, in the timeline's column.
    pub(super) fn row(&self, index: usize, cx: &mut App) -> AnyElement {
        let row = self.rows.get(index);
        let under = if index + 1 == self.rows.len() {
            self.inset
        } else {
            px(row.map_or(0., |row| gap_after(&row.kind)))
        };
        div()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .px_6()
            .when(index == 0, |row| row.pt(px(TOP)))
            .pb(under)
            .text_color(self.p.text)
            .children(row.and_then(|row| self.content(row, index, cx)))
            .into_any_element()
    }

    fn content(&self, row: &Row, index: usize, cx: &mut App) -> Option<AnyElement> {
        if let RowKind::Answer { item, .. } = row.kind {
            self.make_text(item, cx);
        }
        let thread = self.thread.read(cx);
        let replica = thread.replica()?;
        let run_at = |at: &usize| replica.runs.get(*at);
        let item_at = |at: &usize| replica.items.get(*at);
        let id = &row.id;
        let element = match &row.kind {
            RowKind::User { item, run } => self
                .user(item_at(item)?, run_at(run)?, id, index)?
                .into_any_element(),
            RowKind::Answer { item, run, last } => {
                let answered = &item_at(item)?.id;
                let meta = last.then_some(run_at(run)?).map(answer_meta);
                self.answer(answered, thread.drawn_text(answered)?, meta)
                    .into_any_element()
            }
            RowKind::Work { items } => work::group(self, replica, items, id, index),
            RowKind::Live { item } => {
                let working_on = item.as_ref().and_then(item_at);
                self.live(working_on).into_any_element()
            }
            RowKind::Fold { run } => self.fold(run_at(run)?, id, index).into_any_element(),
            RowKind::Request { request, .. } => self
                .request(&replica.requests.get(*request)?.payload)
                .into_any_element(),
            RowKind::Compaction { item } => self.compaction(item_at(item)?),
            RowKind::Notice { item } => self.notice(item_at(item)?).into_any_element(),
            RowKind::Failure { run } => self.failure(run_at(run)?).into_any_element(),
            RowKind::Stopped { run } => self.stopped(run_at(run)?),
            RowKind::Files { run } => self.files(run_at(run)?).into_any_element(),
            RowKind::Queued { run } => self.queued(run_at(run)?).into_any_element(),
        };
        Some(element)
    }

    /// Makes the view of the answer at `item`'s text, the first time it's drawn. Asked first, as
    /// updating the thread while drawing would count as changing it.
    fn make_text(&self, item: usize, cx: &mut App) {
        let thread = self.thread.read(cx);
        let found = thread.replica().and_then(|replica| replica.items.get(item));
        let Some(found) = found.filter(|found| thread.drawn_text(&found.id).is_none()) else {
            return;
        };
        let id = found.id.clone();
        let text = found.body.text().unwrap_or_default().to_owned();
        self.thread.update(cx, |thread, cx| {
            thread.text(&id, &text, cx);
        });
    }

    /// A click that opens or closes what goes by `id`, drawn in the row at `index`.
    pub(super) fn opens(
        &self,
        id: RowId,
        index: usize,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let timeline = self.timeline.clone();
        move |_, _, cx| {
            let id = id.clone();
            timeline
                .update(cx, |timeline, cx| timeline.toggle(id, index, cx))
                .ok();
        }
    }

    fn user(&self, item: &Item, run: &Run, id: &RowId, index: usize) -> Option<Div> {
        let ItemBody::UserMessage(message) = &item.body else {
            return None;
        };
        let p = self.p;
        let long = is_long(&message.text);
        let open = self.open.contains(id);
        let steered = message.delivered == Delivered::Steer;
        let group: SharedString = format!("user-{}", item.id).into();
        // A steer joined the run: when the run was asked for isn't when it was said.
        let sent = if steered { None } else { clock(&run.requested_at) };
        let said = bubble()
            .bg(p.accent.opacity(0.16))
            .child(
                div()
                    .line_height(px(BUBBLE_LINE_HEIGHT))
                    .when(long && !open, |text| {
                        text.max_h(px(COLLAPSED_LINES as f32 * BUBBLE_LINE_HEIGHT))
                            .overflow_hidden()
                    })
                    .child(message.text.clone()),
            )
            .when(long, |said| said.child(self.show_more(id.clone(), index, open)));
        Some(
            div()
                .group(group.clone())
                .flex()
                .flex_col()
                .items_end()
                .gap_1()
                .when(steered, |row| {
                    row.child(div().text_xs().text_color(p.muted).child("↷ Steered"))
                })
                .child(said)
                .child(
                    hover_meta(&group, p)
                        .children(sent)
                        .child(self.copy_button(&item.id)),
                ),
        )
    }

    fn show_more(&self, id: RowId, index: usize, open: bool) -> impl IntoElement {
        div()
            .id(("chat-show-more", index))
            .pt_1()
            .text_xs()
            .text_color(self.p.muted)
            .cursor_pointer()
            .hover(|text| text.text_color(self.p.text))
            .child(if open { "Show less" } else { "Show more" })
            .on_click(self.opens(id, index))
    }

    /// A small button that copies what item `id` says, as it says it then.
    fn copy_button(&self, id: &ItemId) -> impl IntoElement {
        let timeline = self.timeline.clone();
        let item = id.clone();
        let mark = if self.copied.as_ref() == Some(id) {
            IconName::Check
        } else {
            IconName::Copy
        };
        tab_button(SharedString::from(format!("copy-{id}")), self.p)
            .cursor_pointer()
            .child(icon(mark, 12.))
            .on_click(move |_, _, cx| {
                let item = item.clone();
                timeline.update(cx, |timeline, cx| timeline.copy(item, cx)).ok();
            })
    }

    fn answer(&self, id: &ItemId, text: &Entity<TextViewState>, meta: Option<String>) -> Div {
        let group: SharedString = format!("answer-{id}").into();
        div()
            .group(group.clone())
            .flex()
            .flex_col()
            .gap_1()
            .child(TextView::new(text))
            .when_some(meta, |row, meta| {
                row.child(
                    hover_meta(&group, self.p)
                        .child(self.copy_button(id))
                        .child(meta),
                )
            })
    }

    /// The live row: what the run is working on and for how long.
    fn live(&self, item: Option<&Item>) -> Div {
        let elapsed = (self.running_since)
            .map(|since| since.elapsed().as_secs())
            .filter(|seconds| *seconds > 0);
        let label = ShimmerText::new(live_line(item, &self.cwd)).max_fps(SHIMMER_FPS);
        div()
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .text_sm()
            .text_color(self.p.muted)
            .child(ellipsized(label))
            .when_some(elapsed, |row, seconds| {
                row.child(div().flex_shrink_0().child(format!("· {}", duration(seconds))))
            })
    }

    fn fold(&self, run: &Run, id: &RowId, index: usize) -> impl IntoElement {
        let label = worked_for(run).map_or_else(
            || "Worked".to_owned(),
            |took| format!("Worked for {took}"),
        );
        disclosure(("chat-fold", index), self.p)
            .child(label)
            .child(chevron(self.open.contains(id)))
            .on_click(self.opens(id.clone(), index))
    }

    /// What the agent asked, in a line until the kinds of request have their own rows.
    fn request(&self, payload: &RequestPayload) -> impl IntoElement {
        let text = match payload {
            RequestPayload::Approval(approval) => approval.title.as_str(),
            RequestPayload::Question(_) => "A question",
            RequestPayload::Plan { markdown } => markdown.lines().next().unwrap_or("A plan"),
            RequestPayload::Unknown(_) => "Something this version of Koyo can't show",
        };
        div().text_sm().text_color(self.p.muted).child(text.to_owned())
    }
}

/// A message's shape, right or dashed as its row says.
pub(super) fn bubble() -> Div {
    div().max_w(relative(0.8)).px(px(14.)).py(px(9.)).rounded(px(16.))
}

/// A line that opens something, muted until the pointer's on it.
pub(super) fn disclosure(id: impl Into<ElementId>, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .text_sm()
        .text_color(p.muted)
        .cursor_pointer()
        .hover(|row| row.text_color(p.text))
}

/// An arrow that says whether what it heads is open.
pub(super) fn chevron(open: bool) -> impl IntoElement {
    let name = if open {
        IconName::ChevronDown
    } else {
        IconName::ChevronRight
    };
    icon(name, 12.)
}

/// What shows only as the pointer passes over the group `group`.
fn hover_meta(group: &SharedString, p: Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .h(px(20.))
        .text_xs()
        .text_color(p.muted)
        .opacity(0.)
        .group_hover(group.clone(), |meta| meta.opacity(1.))
}

/// The space under a row: a conversation's turns further apart than the lines of its work.
fn gap_after(kind: &RowKind) -> f32 {
    match kind {
        RowKind::Work { .. } | RowKind::Fold { .. } | RowKind::Live { .. } => 10.,
        RowKind::User { .. } | RowKind::Answer { .. } | RowKind::Queued { .. } => 14.,
        _ => 18.,
    }
}

fn is_long(text: &str) -> bool {
    let lines: usize = text.lines().map(|line| 1 + line.chars().count() / BUBBLE_LINE).sum();
    lines > COLLAPSED_LINES
}

#[cfg(test)]
mod tests {
    use super::is_long;

    #[test]
    fn a_message_is_long_past_eight_lines_counting_those_that_wrap() {
        assert!(!is_long(&"line\n".repeat(8)));
        assert!(is_long(&"line\n".repeat(9)));
        assert!(is_long(&"word ".repeat(200)));
        assert!(!is_long(&"語".repeat(200)), "characters, not their bytes");
    }
}
