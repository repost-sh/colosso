//! The chat tab's timeline: the rows of its thread in a list that keeps its place. A view of its
//! own, so what's typed in the composer draws none of it and a token none of the composer. A
//! change of structure derives the rows again and replaces only those that differ; a token touches
//! none of this, only its own text (`docs/chat.md` 5.4).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle, IntoElement,
    Pixels, Render, StyleRefinement, Styled as _, Subscription, Task, Window,
};
use koyo_agent::protocol::ids::{ItemId, RunId, ThreadId};
use koyo_agent::protocol::methods::Catalog;
use koyo_agent::protocol::model::{Hunk, ItemBody, ItemStatus};
use koyo_agent::replica::Replica;
use koyo_agent::timeline::{Row, RowId, RowKind, WorkFold, changed, rows};

use super::output::{PatchColours, patch_colours};
use super::queued::QueuedEdit;
use super::rows::Drawing;
use super::thread::ChatThread;
use super::view::ChatViewEvent;
use super::work::{patch_of, stands_alone};
use crate::settings::{self, ChatWidth, Verbosity};
use crate::theme;
use crate::theme::fonts::at_chat_size;

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
    verbosity: Verbosity,
    /// How big the text is, in the bits a float compares by, and how wide the column, as a person
    /// chose.
    layout: (u32, ChatWidth),
    /// What a person opened: a long message, a settled run's work, a line of work; or a line of
    /// work that showed more, closed to its line.
    open: Rc<HashSet<RowId>>,
    /// The colours of the diffs worked out so far, by item, and the items they're being worked
    /// out for. A diff shows plain until its colours are.
    pub(super) colours: Rc<HashMap<ItemId, Rc<PatchColours>>>,
    colouring: HashSet<ItemId>,
    /// What the live row shows, and since when.
    running: Option<(ItemId, Instant)>,
    /// Whether another chat is over this one, so it has nothing to keep up with until it's back.
    covered: bool,
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
impl EventEmitter<OpenSubagent> for Timeline {}
impl EventEmitter<GoBackTo> for Timeline {}

/// A person went for the thread of a subagent in the timeline.
pub(super) struct OpenSubagent(pub(super) ThreadId);

/// A person went for the message that started a run in the timeline, to say it again.
pub(super) struct GoBackTo(pub(super) RunId);

/// A second's tick for as long as something's `running`, for its elapsed time.
pub(super) fn keep_ticking<T: 'static>(
    ticking: &mut Option<Task<()>>,
    running: bool,
    cx: &mut Context<T>,
) {
    if running == ticking.is_some() {
        return;
    }
    *ticking = running.then(|| {
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
        let choosing = settings::on_change(cx, |timeline: &mut Self, cx| {
            timeline.verbosity_chosen(cx);
            timeline.layout_chosen(cx);
        });
        let mut timeline = Timeline {
            thread,
            catalog,
            scroller,
            cwd,
            composer,
            verbosity: settings::get(cx).chat_verbosity,
            layout: layout(cx),
            rows: Rc::default(),
            open: Rc::default(),
            colours: Rc::default(),
            colouring: HashSet::new(),
            running: None,
            covered: false,
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
        if self.covered {
            return;
        }
        let replica = self.thread.read(cx).replica();
        let verbosity = self.verbosity;
        let fold = |run: &RunId| {
            if !verbosity.fold_turns {
                WorkFold::None
            } else if self.open.contains(&RowId::Fold(run.clone())) {
                WorkFold::Open
            } else {
                WorkFold::Closed
            }
        };
        let alone = |item: &_| stands_alone(item, verbosity);
        let new = replica.map_or_else(Vec::new, |replica| rows(replica, fold, alone));
        let uncoloured = replica.map_or_else(Vec::new, |replica| self.uncoloured(replica));
        let running = replica.and_then(|replica| running_item(replica, &new));
        let subagent_running = replica.is_some_and(|replica| {
            (replica.items.iter()).any(|item| {
                matches!(item.body, ItemBody::Subagent(_)) && item.status == ItemStatus::Running
            })
        });
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
        self.colour(uncoloured, cx);
        keep_ticking(
            &mut self.ticking,
            self.running.is_some() || subagent_running,
            cx,
        );
        self.follow_queued_edit(cx);
        cx.notify();
    }

    /// The changes the thread holds whose colours nobody's working out yet: each one's item, the
    /// file it changed, and its patch.
    fn uncoloured(&mut self, replica: &Replica) -> Vec<(ItemId, String, Vec<Hunk>)> {
        let mut found = Vec::new();
        for item in &replica.items {
            let ItemBody::Tool(tool) = &item.body else {
                continue;
            };
            let Some(patch) = patch_of(tool).filter(|_| !self.colouring.contains(&item.id)) else {
                continue;
            };
            self.colouring.insert(item.id.clone());
            let file = tool.files.first().unwrap_or(&tool.label).clone();
            found.push((item.id.clone(), file, patch.to_vec()));
        }
        found
    }

    /// Works out the colours of `changes` off the main thread, and draws them once they are.
    fn colour(&mut self, changes: Vec<(ItemId, String, Vec<Hunk>)>, cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        let worked_out = cx.background_spawn(async move {
            (changes.into_iter())
                .map(|(id, file, patch)| (id, patch_colours(&file, &patch)))
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |this, cx| {
            let colours = worked_out.await;
            this.update(cx, |timeline, cx| {
                let held = Rc::make_mut(&mut timeline.colours);
                held.extend(
                    colours
                        .into_iter()
                        .map(|(id, colours)| (id, Rc::new(colours))),
                );
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Takes up how much of the work a person now wants to see, which starts over what they opened
    /// and closed against what they had: every row may be another height.
    fn verbosity_chosen(&mut self, cx: &mut Context<Self>) {
        let verbosity = settings::get(cx).chat_verbosity;
        if verbosity == self.verbosity {
            return;
        }
        self.verbosity = verbosity;
        self.open = Rc::default();
        self.follow(cx);
        self.remeasure(cx);
    }

    /// Takes up how big a person now wants the text, or how wide the column: every row may be
    /// another height, or wrap another way.
    fn layout_chosen(&mut self, cx: &mut Context<Self>) {
        let layout = layout(cx);
        if layout == self.layout {
            return;
        }
        self.layout = layout;
        self.remeasure(cx);
        cx.notify();
    }

    /// Has every row measured again.
    fn remeasure(&self, cx: &mut Context<Self>) {
        let count = self.rows.len();
        self.scroller
            .update(cx, |scroller, cx| scroller.remeasure_items(0..count, cx));
    }

    /// Another chat came over this one, or left: what happened meanwhile is taken in at once.
    pub(super) fn set_covered(&mut self, covered: bool, cx: &mut Context<Self>) {
        self.covered = covered;
        if covered {
            self.ticking = None;
        } else {
            self.follow(cx);
        }
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
        MessageScroller::new(
            "chat-timeline",
            self.scroller.clone(),
            move |index, _, cx| {
                let row = drawing.row(index, cx);
                at_chat_size(row, cx)
            },
        )
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
            now: chrono::Utc::now(),
            thread: self.thread.clone(),
            catalog: self.catalog.clone(),
            timeline: cx.weak_entity(),
            rows: Rc::clone(&self.rows),
            open: Rc::clone(&self.open),
            verbosity: self.verbosity,
            width: self.layout.1,
            colours: Rc::clone(&self.colours),
            running_since: self.running.as_ref().map(|(_, since)| *since),
            copied: self.copied.as_ref().map(|(id, _)| id.clone()),
            cwd: Rc::clone(&self.cwd),
            queued_edit: (self.queued_edit.as_ref())
                .map(|edit| (edit.run.clone(), edit.text.clone())),
            composer: self.composer.clone(),
            can_go_back: self.thread.read(cx).replica().is_some_and(can_go_back),
            inset: self.inset,
            p: theme::palette(cx),
        }
    }
}

/// How big a chat's text is and how wide its column, as they're compared for whether every row
/// needs measuring again.
fn layout(cx: &App) -> (u32, ChatWidth) {
    let chosen = settings::get(cx);
    (chosen.fonts.chat_size().to_bits(), chosen.chat_width)
}

/// Whether the host would let a chat go back: not a subagent's, nothing at work or waiting.
pub(super) fn can_go_back(replica: &Replica) -> bool {
    replica.thread.parent.is_none() && replica.live().is_none() && replica.queued().next().is_none()
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
