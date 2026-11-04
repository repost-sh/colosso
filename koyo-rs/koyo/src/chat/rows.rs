//! A timeline row as it's drawn (`docs/chat.md` 5.3): what the derivation said it is, from what the
//! thread holds now. What each row can open is kept by the timeline, not here.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::TextareaState;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, WeakEntity, Window, div, px, relative,
};
use koyo_agent::protocol::ids::{ItemId, RunId};
use koyo_agent::protocol::methods::Catalog;
use koyo_agent::protocol::model::{Attachment, Delivered, Item, ItemBody, Run};
use koyo_agent::protocol::requests::{Request, RequestPayload};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind};
use koyo_agent::words::{answer_meta, clock, duration, live_line, plan_parts, worked_for};

use super::{asks, attachments, subagents};
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
/// How big a picture a message attached shows.
const ATTACHED_PICTURE: f32 = 160.;
/// How often the live row's highlight moves: slow enough for the rows it redraws not to matter.
pub(super) const SHIMMER_FPS: f32 = 20.;

/// What the list's rows are drawn from.
pub(super) struct Drawing {
    /// Now, as of this draw of the timeline, for how long what's running took.
    pub(super) now: chrono::DateTime<chrono::Utc>,
    pub(super) thread: Entity<ChatThread>,
    pub(super) catalog: Entity<Catalog>,
    pub(super) timeline: WeakEntity<Timeline>,
    pub(super) rows: Rc<Vec<Row>>,
    pub(super) open: Rc<HashSet<RowId>>,
    /// Whether a line of work is open until a person closes it, rather than closed until they open
    /// it.
    pub(super) all_work_open: bool,
    pub(super) running_since: Option<Instant>,
    pub(super) copied: Option<ItemId>,
    pub(super) cwd: Rc<str>,
    /// The queued message being changed where it shows, and what it's being changed to.
    pub(super) queued_edit: Option<(RunId, Entity<TextareaState>)>,
    /// Who an answer to what's asked is said to.
    pub(super) composer: FocusHandle,
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
        self.make_text(&row.kind, cx);
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
                let meta = (last.then_some(run_at(run)?))
                    .map(|run| answer_meta(run, &self.catalog.read(cx).models));
                self.answer(answered, thread.drawn_text(answered)?, meta)
                    .into_any_element()
            }
            RowKind::Work { items } => work::group(self, replica, items, id, index),
            RowKind::Subagents { items } => subagents::group(self, replica, items, id, index),
            RowKind::Live { item } => {
                let working_on = item.as_ref().and_then(item_at);
                self.live(working_on).into_any_element()
            }
            RowKind::Fold { run } => self
                .fold(run_at(run)?, &replica.requests, id, index)
                .into_any_element(),
            RowKind::Request {
                request,
                item,
                asking,
            } => {
                let (asked, item) = (replica.requests.get(*request)?, &item_at(item)?.id);
                asks::request(self, asked, *asking, item, id, index, cx)?
            }
            RowKind::Compaction { item } => self.compaction(item_at(item)?),
            RowKind::Notice { item } => self.notice(item_at(item)?).into_any_element(),
            RowKind::Failure { run } => self.failure(run_at(run)?).into_any_element(),
            RowKind::Stopped { run } => self.stopped(run_at(run)?),
            RowKind::Files { run } => self.files(run_at(run)?).into_any_element(),
            RowKind::Queued { run } => self.queued(run_at(run)?).into_any_element(),
        };
        Some(element)
    }

    /// Makes the view of the text a row shows, the first time it's drawn. Asked first, as updating
    /// the thread while drawing would count as changing it.
    fn make_text(&self, kind: &RowKind, cx: &mut App) {
        let thread = self.thread.read(cx);
        let shown = thread.replica().and_then(|replica| shown_text(kind, replica));
        let Some((id, text)) = shown.filter(|(id, _)| thread.drawn_text(id).is_none()) else {
            return;
        };
        let (id, text) = (id.clone(), text.to_owned());
        self.thread.update(cx, |thread, cx| {
            thread.text(&id, &text, cx);
        });
    }

    /// Whether the line of work that goes by `id` shows what's under it.
    pub(super) fn work_open(&self, id: &RowId) -> bool {
        self.open.contains(id) != self.all_work_open
    }

    /// What an element of a row hears, a click or an action, that `does` something to the
    /// timeline.
    pub(super) fn on_timeline<E: ?Sized>(
        &self,
        does: impl Fn(&mut Timeline, &mut Window, &mut Context<Timeline>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let timeline = self.timeline.clone();
        move |_, window, cx| {
            timeline.update(cx, |timeline, cx| does(timeline, window, cx)).ok();
        }
    }

    /// A click that opens or closes what goes by `id`, drawn in the row at `index`.
    pub(super) fn opens(
        &self,
        id: RowId,
        index: usize,
    ) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        self.on_timeline(move |timeline, _, cx| timeline.toggle(id.clone(), index, cx))
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
            .bg(p.selected)
            .when(!message.attachments.is_empty(), |said| {
                said.child(attached(&item.id.0, &message.attachments, p))
            })
            .when(!message.text.is_empty(), |said| {
                said.child(
                    div()
                        .line_height(px(BUBBLE_LINE_HEIGHT))
                        .when(long && !open, |text| {
                            text.max_h(px(COLLAPSED_LINES as f32 * BUBBLE_LINE_HEIGHT))
                                .overflow_hidden()
                        })
                        .child(message.text.clone()),
                )
            })
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

    pub(super) fn show_more(&self, id: RowId, index: usize, open: bool) -> impl IntoElement {
        let says = if open { "Show less" } else { "Show more" };
        word_button(("chat-show-more", index), says, self.p)
            .pt_1()
            .on_click(self.opens(id, index))
    }

    /// A small button that copies what item `id` says, as it says it then.
    fn copy_button(&self, id: &ItemId) -> impl IntoElement {
        tab_button(SharedString::from(format!("copy-{id}")), self.p)
            .cursor_pointer()
            .child(icon(self.copy_mark(id), 12.))
            .on_click(self.copies(id))
    }

    /// What a copy button of item `id` shows: whether it just copied.
    pub(super) fn copy_mark(&self, id: &ItemId) -> IconName {
        if self.copied.as_ref() == Some(id) {
            IconName::Check
        } else {
            IconName::Copy
        }
    }

    /// A click that copies what item `id` says.
    pub(super) fn copies(&self, id: &ItemId) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let item = id.clone();
        self.on_timeline(move |timeline, _, cx| timeline.copy(item.clone(), cx))
    }

    fn answer(&self, id: &ItemId, text: &Entity<TextViewState>, meta: Option<String>) -> Div {
        let group: SharedString = format!("answer-{id}").into();
        div()
            .group(group.clone())
            .flex()
            .flex_col()
            .gap_1()
            .child(TextView::new(text))
            .child(hover_meta(&group, self.p).child(self.copy_button(id)).children(meta))
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

    fn fold(&self, run: &Run, requests: &[Request], id: &RowId, index: usize) -> impl IntoElement {
        let label = worked_for(run, requests).map_or_else(
            || "Worked".to_owned(),
            |took| format!("Worked for {took}"),
        );
        disclosure(("chat-fold", index), self.p)
            .child(label)
            .child(chevron(self.open.contains(id)))
            .on_click(self.opens(id.clone(), index))
    }
}

/// What a message attached, above its words: a picture a fixed size, so the row's height is known
/// before it's read, and any other file by its name. Each opens where the person would.
pub(super) fn attached(key: &str, attachments: &[Attachment], p: Palette) -> Div {
    let folder = attachments::folder();
    let tiles = attachments.iter().enumerate().map(|(at, attachment)| {
        let id: SharedString = format!("attached-{key}-{at}").into();
        let path = folder.as_ref().map(|folder| folder.join(&attachment.file));
        attachments::tile(id, attachment, folder.as_deref(), ATTACHED_PICTURE, p)
            .when_some(path, |tile, path| {
                tile.on_click(move |_: &ClickEvent, _, cx| cx.open_with_system(&path))
            })
    });
    div().flex().flex_wrap().justify_end().gap_2().pb_1().children(tiles)
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

/// A small word that does something once clicked, brighter under the pointer.
pub(super) fn word_button(id: impl Into<ElementId>, says: &'static str, p: Palette) -> Stateful<Div> {
    div()
        .id(id)
        .text_xs()
        .text_color(p.muted)
        .cursor_pointer()
        .hover(move |word| word.text_color(p.text))
        .child(says)
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
        // What's said on the way is a paragraph of the same turn, and the room for its copy button
        // already keeps it from what follows.
        RowKind::Answer { last: false, .. } => 0.,
        RowKind::Work { .. } | RowKind::Fold { .. } | RowKind::Live { .. } | RowKind::Subagents { .. } => 10.,
        RowKind::User { .. } | RowKind::Answer { .. } | RowKind::Queued { .. } => 14.,
        _ => 18.,
    }
}

/// The text a row of this kind shows in a view of its own: an answer, or what's under a plan's
/// title. What it's kept by, and what it says.
fn shown_text<'a>(kind: &RowKind, replica: &'a Replica) -> Option<(&'a ItemId, &'a str)> {
    match *kind {
        RowKind::Answer { item, .. } => {
            let item = replica.items.get(item)?;
            Some((&item.id, item.body.text()?))
        }
        RowKind::Request { item, request, .. } => {
            let RequestPayload::Plan { markdown } = &replica.requests.get(request)?.payload else {
                return None;
            };
            Some((&replica.items.get(item)?.id, plan_parts(markdown).1))
        }
        _ => None,
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
