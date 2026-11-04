//! The chat tab's timeline: the rows of its thread in a list that keeps its place. A view of its
//! own, so what's typed in the composer draws none of it and a token none of the composer. A
//! change of structure derives the rows again and replaces only those that differ; a token touches
//! none of this, only its own text (`docs/chat.md` 5.4).

use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::{
    ClipboardItem, Context, Entity, EventEmitter, IntoElement, Pixels, Render, StyleRefinement,
    Styled as _, Subscription, Task, Window,
};
use koyo_agent::protocol::ids::{ItemId, RunId};
use koyo_agent::protocol::model::ItemBody;
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind, changed, rows};

use super::rows::Drawing;
use super::thread::ChatThread;
use super::view::ChatViewEvent;
use crate::theme;

/// How long a copy button says it copied.
const COPIED_FOR: Duration = Duration::from_millis(1500);

pub(super) struct Timeline {
    thread: Entity<ChatThread>,
    scroller: Entity<MessageScrollerState>,
    cwd: Rc<str>,
    rows: Rc<Vec<Row>>,
    /// What a person opened: a long message, a group of work, a tool, a settled run's work.
    open: Rc<HashSet<RowId>>,
    /// What the live row shows, and since when.
    running: Option<(ItemId, Instant)>,
    /// Keeps the elapsed time going while something's running.
    ticking: Option<Task<()>>,
    /// What was just copied, until the button stops saying so.
    copied: Option<(ItemId, Task<()>)>,
    /// The room under the last row, for what floats there.
    inset: Pixels,
    _following: Subscription,
    #[cfg(test)]
    pub(super) renders: usize,
}

impl EventEmitter<ChatViewEvent> for Timeline {}

impl Timeline {
    pub(super) fn new(
        thread: Entity<ChatThread>,
        scroller: Entity<MessageScrollerState>,
        cwd: Rc<str>,
        inset: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        let following = cx.observe(&thread, |timeline, _, cx| timeline.follow(cx));
        let mut timeline = Timeline {
            thread,
            scroller,
            cwd,
            rows: Rc::default(),
            open: Rc::default(),
            running: None,
            ticking: None,
            copied: None,
            inset,
            _following: following,
            #[cfg(test)]
            renders: 0,
        };
        timeline.follow(cx);
        timeline
    }

    /// Takes in what the thread holds now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let replica = self.thread.read(cx).replica();
        let unfolded = |run: &RunId| self.open.contains(&RowId::Fold(run.clone()));
        let new = replica.map_or_else(Vec::new, |replica| rows(replica, unfolded));
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
        cx.notify();
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
        let found = replica.and_then(|replica| replica.items.iter().rev().find(|held| held.id == id));
        let Some(text) = found.and_then(|found| said(&found.body)) else {
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
        let drawing = Drawing {
            thread: self.thread.clone(),
            timeline: cx.weak_entity(),
            rows: Rc::clone(&self.rows),
            open: Rc::clone(&self.open),
            running_since: self.running.as_ref().map(|(_, since)| *since),
            copied: self.copied.as_ref().map(|(id, _)| id.clone()),
            cwd: Rc::clone(&self.cwd),
            inset: self.inset,
            p: theme::palette(cx),
        };
        MessageScroller::new("chat-timeline", self.scroller.clone(), move |index, _, cx| {
            drawing.row(index, cx)
        })
        .jump_button(false)
        .with_list_style(StyleRefinement::default().py_0())
        .with_row_style(StyleRefinement::default().px_0().pb_0())
        .size_full()
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
fn said(body: &ItemBody) -> Option<&str> {
    match body {
        ItemBody::UserMessage(message) => Some(&message.text),
        _ => body.text(),
    }
}
