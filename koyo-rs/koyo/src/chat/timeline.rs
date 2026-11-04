//! The chat tab's timeline: the rows of its thread in a list that keeps its place. A view of its
//! own, so what's typed in the composer draws none of it and a token none of the composer. A
//! change of structure derives the rows again and replaces only those that differ; a token touches
//! none of this, only its own text (`docs/chat.md` 5.4).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::{
    ClipboardItem, Context, Entity, EventEmitter, FocusHandle, IntoElement, Pixels, Render, StyleRefinement,
    Styled as _, Subscription, Task, Window,
};
use koyo_agent::protocol::ids::{ItemId, RunId};
use koyo_agent::protocol::methods::Catalog;
use koyo_agent::protocol::model::ItemBody;
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind, WorkFold, changed, rows};

use super::queued::QueuedEdit;
use super::rows::Drawing;
use super::thread::ChatThread;
use super::view::ChatViewEvent;
use crate::settings::{self, ChatWork};
use crate::theme;

/// How long a copy button says it copied.
const COPIED_FOR: Duration = Duration::from_millis(1500);

pub(super) struct Timeline {
    pub(super) thread: Entity<ChatThread>,
    /// What the models are called.
    catalog: Entity<Catalog>,
    pub(super) scroller: Entity<MessageScrollerState>,
    cwd: Rc<str>,
    /// Who an answer to what's asked is said to.
    pub(super) composer: FocusHandle,
    pub(super) rows: Rc<Vec<Row>>,
    /// How much of the work shows, as a person chose.
    work: ChatWork,
    /// What a person opened: a long message, a settled run's work; or a line of work, or with all
    /// of them open, closed.
    open: Rc<HashSet<RowId>>,
    /// What the live row shows, and since when.
    running: Option<(ItemId, Instant)>,
    /// Keeps the elapsed time going while something's running.
    ticking: Option<Task<()>>,
    /// What was just copied, until the button stops saying so.
    copied: Option<(ItemId, Task<()>)>,
    /// The room under the last row, for what floats there.
    inset: Pixels,
    /// The queued message being changed where it shows.
    pub(super) queued_edit: Option<QueuedEdit>,
    _following: Subscription,
    _choosing: Subscription,
    #[cfg(test)]
    pub(super) renders: usize,
}

impl EventEmitter<ChatViewEvent> for Timeline {}

impl Timeline {
    pub(super) fn new(
        thread: Entity<ChatThread>,
        catalog: Entity<Catalog>,
        scroller: Entity<MessageScrollerState>,
        cwd: Rc<str>,
        composer: FocusHandle,
        inset: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        let following = cx.observe(&thread, |timeline, _, cx| timeline.follow(cx));
        let choosing = settings::on_change(cx, |timeline: &mut Self, cx| timeline.work_chosen(cx));
        let mut timeline = Timeline {
            thread,
            catalog,
            scroller,
            cwd,
            composer,
            work: settings::get(cx).chat_work,
            rows: Rc::default(),
            open: Rc::default(),
            running: None,
            ticking: None,
            copied: None,
            inset,
            queued_edit: None,
            _following: following,
            _choosing: choosing,
            #[cfg(test)]
            renders: 0,
        };
        timeline.follow(cx);
        timeline
    }

    /// Takes in what the thread holds now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let replica = self.thread.read(cx).replica();
        let fold = |run: &RunId| match self.work {
            ChatWork::Steps | ChatWork::Everything => WorkFold::None,
            ChatWork::Turns if self.open.contains(&RowId::Fold(run.clone())) => WorkFold::Open,
            ChatWork::Turns => WorkFold::Closed,
        };
        let new = replica.map_or_else(Vec::new, |replica| rows(replica, fold));
        let running = replica.and_then(|replica| running_item(replica, &new));
        self.running = running.map(|id| {
            let since = (self.running.as_ref())
                .filter(|(held, _)| *held == id)
                .map_or_else(Instant::now, |(_, since)| *since);
            (id, since)
        });
        if let Some((replaced, count)) = changed(&self.rows, &new) {
            self.scroller.update(cx, |scroller, cx| {
                scroller.splice(replaced, count, cx);
            });
        }
        self.rows = Rc::new(new);
        self.keep_time(cx);
        self.follow_queued_edit(cx);
        cx.notify();
    }

    /// Takes up how much of the work a person now wants to see, which starts over what they opened
    /// and closed against what they had: every row may be another height.
    fn work_chosen(&mut self, cx: &mut Context<Self>) {
        let work = settings::get(cx).chat_work;
        if work == self.work {
            return;
        }
        self.work = work;
        self.open = Rc::default();
        self.follow(cx);
        let count = self.rows.len();
        self.scroller.update(cx, |scroller, cx| scroller.remeasure_items(0..count, cx));
    }

    /// A second's tick for as long as something's running, for its elapsed time.
    fn keep_time(&mut self, cx: &mut Context<Self>) {
        let running = self.running.is_some();
        if running == self.ticking.is_some() {
            return;
        }
        self.ticking = running.then(|| {
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
            })
        });
    }

    /// Opens or closes what goes by `id`, drawn in the row at `index`. A settled run's work is
    /// rows of its own; anything else is the row's, which only changes height.
    pub(super) fn toggle(&mut self, id: RowId, index: usize, cx: &mut Context<Self>) {
        let open = Rc::make_mut(&mut self.open);
        let folds = matches!(id, RowId::Fold(_));
        if !open.remove(&id) {
            open.insert(id);
        }
        if folds {
            self.follow(cx);
        } else {
            self.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(index..index + 1, cx);
            });
            cx.notify();
        }
    }

    /// Puts what item `id` says now on the clipboard, and has its button say so for a moment.
    pub(super) fn copy(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let replica = self.thread.read(cx).replica();
        let Some(text) = replica.and_then(|replica| said(replica, &id)) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        let forgets = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COPIED_FOR).await;
            this.update(cx, |this, cx| {
                this.copied = None;
                cx.notify();
            })
            .ok();
        });
        self.copied = Some((id, forgets));
        cx.notify();
    }

    /// The room under the last row is now `inset`.
    pub(super) fn set_inset(&mut self, inset: Pixels, cx: &mut Context<Self>) {
        self.inset = inset;
        let count = self.rows.len();
        self.scroller.update(cx, |scroller, cx| {
            scroller.remeasure_items(count.saturating_sub(1)..count, cx);
        });
        cx.notify();
    }
}

/// The list, with room under its last row for what floats there. That room is the row's and not
/// the list's: what scrolls the list measures it by its rows, without its padding, and jumps by
/// the difference.
impl Render for Timeline {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let drawing = self.drawing(cx);
        MessageScroller::new("chat-timeline", self.scroller.clone(), move |index, _, cx| {
            drawing.row(index, cx)
        })
        .jump_button(false)
        .with_list_style(StyleRefinement::default().py_0())
        .with_row_style(StyleRefinement::default().px_0().pb_0())
        .size_full()
    }
}

impl Timeline {
    /// What the rows are drawn from now.
    pub(super) fn drawing(&self, cx: &Context<Self>) -> Drawing {
        Drawing {
            thread: self.thread.clone(),
            catalog: self.catalog.clone(),
            timeline: cx.weak_entity(),
            rows: Rc::clone(&self.rows),
            open: Rc::clone(&self.open),
            all_work_open: self.work == ChatWork::Everything,
            running_since: self.running.as_ref().map(|(_, since)| *since),
            copied: self.copied.as_ref().map(|(id, _)| id.clone()),
            cwd: Rc::clone(&self.cwd),
            queued_edit: (self.queued_edit.as_ref()).map(|edit| (edit.run.clone(), edit.text.clone())),
            composer: self.composer.clone(),
            inset: self.inset,
            p: theme::palette(cx),
        }
    }
}

/// The item the live row in `rows` shows.
fn running_item(replica: &Replica, rows: &[Row]) -> Option<ItemId> {
    let item = rows.iter().rev().find_map(|row| match row.kind {
        RowKind::Live { item } => item,
        _ => None,
    })?;
    Some(replica.items.get(item)?.id.clone())
}

/// What a person would copy of an item.
fn said<'a>(replica: &'a Replica, id: &ItemId) -> Option<&'a str> {
    let item = replica.items.iter().rev().find(|held| held.id == *id)?;
    match &item.body {
        ItemBody::UserMessage(message) => Some(&message.text),
        ItemBody::Request(asked) => replica.plan(&asked.request_id),
        body => body.text(),
    }
}
