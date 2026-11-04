//! A chat tab: the timeline, and what floats over it (`docs/chat.md` 5.3). The two are views of
//! their own, each drawn only for its own changes; this is what's left: where they go, the pill
//! back to the end, and the older runs as the top gets near. What floats is among the window's
//! overlays, as the glass hides what's drawn under it. That draws this view again for any change
//! under it (a deferred element's reads aren't told apart), so it's kept to very little.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, Bounds, ClickEvent, Context, ElementId, Entity, EventEmitter,
    ExternalPaths, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, SharedString, StatefulInteractiveElement as _,
    StyleRefinement, Styled as _, Subscription, Task, Window, deferred, div, px,
};
use koyo_agent::protocol::ids::ThreadId;

use super::Chats;
use super::composer::{ChatComposer, ComposerEvent};
use super::thread::ChatThread;
use super::timeline::Timeline;
use crate::native::liquid;
use crate::shell::icon;
use crate::theme;

/// The widest the timeline and the composer get.
pub(super) const COLUMN: f32 = 760.;
/// How far the composer floats above the card's bottom edge.
const FLOAT: f32 = 12.;
/// How far the last row keeps from the composer.
const CLEARANCE: f32 = 12.;
/// How long after leaving the end the pill back to it shows, so a nudge doesn't flash it.
const PILL_DELAY: Duration = Duration::from_millis(150);
/// How many rows from the top the next older runs are asked for, so they're there when reached.
const NEAR_TOP: usize = 10;

/// What the shell hears from a chat tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatViewEvent {
    /// The first send made the chat's thread.
    Created(ThreadId),
    /// A person opened a file the chat changed.
    OpenFile(PathBuf),
}

/// The thread a tab shows, and its timeline.
pub(super) struct Shown {
    pub(super) thread: Entity<ChatThread>,
    pub(super) timeline: Entity<Timeline>,
}

pub struct ChatView {
    chats: Entity<Chats>,
    cwd: PathBuf,
    pub(super) shown: Option<Shown>,
    pub(super) composer: Entity<ChatComposer>,
    pub(super) scroller: Entity<MessageScrollerState>,
    /// How tall the composer last was, which the timeline's end clears.
    composer_height: Pixels,
    /// Whether the pill back to the end shows.
    pill: bool,
    /// Waits to show the pill.
    showing_pill: Option<Task<()>>,
    /// Whether there may be older runs than those held, and they aren't being asked for.
    can_fetch_older: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ChatViewEvent> for ChatView {}

impl ChatView {
    pub fn new(
        chats: Entity<Chats>,
        cwd: PathBuf,
        place: SharedString,
        thread: Option<ThreadId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        chats.update(cx, Chats::connect);
        let opened = thread.map(|id| chats.update(cx, |chats, cx| chats.thread(&id, cx)));
        let catalog = chats.update(cx, |chats, cx| chats.catalog(&cwd, cx));
        let composer = {
            let (chats, cwd, opened) = (chats.clone(), cwd.clone(), opened.clone());
            cx.new(|cx| ChatComposer::new(chats, catalog, cwd, place, opened, window, cx))
        };
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let subscriptions = vec![
            cx.subscribe(&composer, |view, _, event, cx| view.composer_said(event, cx)),
            cx.observe(&scroller, |view, _, cx| view.scrolled(cx)),
        ];
        let mut view = ChatView {
            chats,
            cwd,
            shown: None,
            composer,
            scroller,
            composer_height: Pixels::ZERO,
            pill: false,
            showing_pill: None,
            can_fetch_older: true,
            _subscriptions: subscriptions,
        };
        if let Some(thread) = opened {
            view.show(thread, cx);
        }
        view
    }

    fn composer_said(&mut self, event: &ComposerEvent, cx: &mut Context<Self>) {
        match event {
            ComposerEvent::Sent => self.scroller.update(cx, MessageScrollerState::scroll_to_end),
            ComposerEvent::Created(thread, id) => {
                self.show(thread.clone(), cx);
                cx.emit(ChatViewEvent::Created(id.clone()));
            }
        }
        cx.notify();
    }

    /// From now on the tab shows `thread`, whose timeline follows it.
    fn show(&mut self, thread: Entity<ChatThread>, cx: &mut Context<Self>) {
        let (cwd, inset) = (self.cwd.to_string_lossy().into(), self.inset());
        let (following, scroller) = (thread.clone(), self.scroller.clone());
        let catalog = self.composer.read(cx).catalog.clone();
        let composer = self.composer.read(cx).focus_handle(cx);
        let timeline = cx.new(|cx| Timeline::new(following, catalog, scroller, cwd, composer, inset, cx));
        let heard = cx.subscribe(&timeline, |_, _, event: &ChatViewEvent, cx| cx.emit(event.clone()));
        self._subscriptions.push(heard);
        self.shown = Some(Shown { thread, timeline });
    }

    /// A person scrolled: the pill follows, and older runs come as the top gets near.
    fn scrolled(&mut self, cx: &mut Context<Self>) {
        let scroller = self.scroller.read(cx);
        let up = scroller.is_scrolled_up();
        let near_top = scroller.logical_scroll_top().item_ix < NEAR_TOP;
        if !up {
            self.showing_pill = None;
            if self.pill {
                self.pill = false;
                cx.notify();
            }
        } else if self.showing_pill.is_none() {
            self.showing_pill = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(PILL_DELAY).await;
                this.update(cx, |this, cx| {
                    this.pill = true;
                    cx.notify();
                })
                .ok();
            }));
        }
        if up && near_top {
            self.fetch_older(cx);
        }
    }

    /// Asks for the runs before the oldest held, once at a time.
    fn fetch_older(&mut self, cx: &mut Context<Self>) {
        let Some(thread) = self.shown.as_ref().map(|shown| shown.thread.clone()) else {
            return;
        };
        let opened = thread.read(cx);
        let oldest = opened.replica().and_then(|replica| replica.runs.first());
        let Some(oldest) = oldest.filter(|_| self.can_fetch_older) else {
            return;
        };
        let page = self.chats.read(cx).history(opened.id(), oldest.id.clone(), cx);
        self.can_fetch_older = false;
        cx.spawn(async move |this, cx| {
            let page = page.await;
            this.update(cx, |this, cx| {
                this.can_fetch_older = true;
                match page {
                    Ok(page) => {
                        this.can_fetch_older = page.runs.len() == super::HISTORY_PAGE;
                        thread.update(cx, |thread, cx| thread.prepend(page, cx));
                    }
                    Err(error) => log::warn!("older runs of a chat: {error}"),
                }
            })
            .ok();
        })
        .detach();
    }

    /// How far the timeline's last row has to stay from the bottom to clear the glass.
    fn inset(&self) -> Pixels {
        self.composer_height + px(FLOAT + CLEARANCE)
    }

    /// The composer: over the end of the timeline, or in the middle of an empty chat.
    fn composer_in_place(&self, cx: &Context<Self>) -> AnyElement {
        let place = div()
            .absolute()
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .on_children_prepainted(self.measures_composer(cx));
        let place = if self.composer.read(cx).is_new() {
            place.top_0().bottom_0().justify_center().pb(px(48.))
        } else {
            place.bottom(px(FLOAT))
        };
        deferred(place.child(self.composer.clone())).into_any_element()
    }

    /// The composer's height, once it's laid out, for the timeline to keep clear of it. Told a
    /// frame late, since the timeline drew first.
    fn measures_composer(
        &self,
        cx: &Context<Self>,
    ) -> impl Fn(Vec<Bounds<Pixels>>, &mut Window, &mut App) + 'static {
        let view = cx.weak_entity();
        let held = self.composer_height;
        move |children, _, cx| {
            let height = children.first().map_or(Pixels::ZERO, |child| child.size.height);
            if height != held {
                let view = view.clone();
                cx.defer(move |cx| {
                    view.update(cx, |view, cx| view.composer_measured(height, cx)).ok();
                });
            }
        }
    }

    fn composer_measured(&mut self, height: Pixels, cx: &mut Context<Self>) {
        self.composer_height = height;
        let inset = self.inset();
        if let Some(shown) = &self.shown {
            shown.timeline.update(cx, |timeline, cx| timeline.set_inset(inset, cx));
        }
        cx.notify();
    }

    /// The pill back to the end of the timeline, once a person left it.
    fn scroll_pill(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let composer = self.composer.read(cx);
        if !self.pill || composer.covers_above() {
            return None;
        }
        let queued = composer.queued();
        let p = theme::palette(cx);
        let keyed = ElementId::NamedInteger("chat-pill-glass".into(), cx.entity_id().as_u64());
        let on_glass = liquid::backdrop(keyed, px(14.), window, cx);
        let pill = div()
            .id("chat-scroll-to-end")
            .relative()
            .flex()
            .items_center()
            .gap_1()
            .h(px(28.))
            .px_3()
            .rounded_full()
            .occlude()
            .cursor_pointer()
            .text_xs()
            .text_color(p.text);
        let pill = liquid::floating(pill, on_glass, p)
            .child(icon(IconName::ChevronDown, 13.))
            .child("Scroll to end")
            .when(queued > 0, |pill| pill.child(format!("· {queued} queued")))
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                this.scroller.update(cx, MessageScrollerState::scroll_to_end);
            }));
        let bar = div()
            .absolute()
            .bottom(self.inset())
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(pill);
        Some(deferred(bar).into_any_element())
    }
}

impl Focusable for ChatView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.composer.read(cx).focus_handle(cx)
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let timeline = (self.shown.as_ref())
            .map(|shown| shown.timeline.clone().cached(StyleRefinement::default().size_full()));
        div()
            .id("chat")
            .relative()
            .size_full()
            .text_color(theme::palette(cx).text)
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                this.composer.update(cx, |composer, cx| composer.attach_paths(dropped.paths(), cx));
            }))
            .children(timeline)
            .children(self.scroll_pill(window, cx))
            .child(self.composer_in_place(cx))
    }
}
