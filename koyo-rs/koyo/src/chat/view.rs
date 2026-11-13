//! A chat tab: the timeline, and what floats over it (`docs/chat.md` 5.3). The two are views of
//! their own, each drawn only for its own changes; this is what's left: where they go, the pill
//! back to the end, and the older runs as the top gets near. What floats is among the window's
//! overlays, as the glass hides what's drawn under it. That draws this view again for any change
//! under it (a deferred element's reads aren't told apart), so it's kept to very little.

mod context_menu;
pub(super) mod details;
pub(super) mod floating;
pub(super) mod picture;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, AppContext as _, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, SharedString,
    StyleRefinement, Styled as _, Subscription, Task, Window, div, px,
};
use koyo_agent::protocol::ids::ThreadId;

use super::Chats;
use super::composer::{ChatComposer, ComposerEvent};
use super::rewind;
use super::subagent_bar::{KEYS, OpenParent, SubagentBar, WentToParent};
use super::thread::ChatThread;
use super::timeline::{GoBackTo, OpenSubagent, Timeline};
use crate::projects::with_menu;
use crate::settings::{self, ProjectScript};
use crate::theme;
use crate::viewer::Viewer;
use details::{Details, OpenIn, ToggleDetails, Workplace};
use floating::{CARD_GUESS, Floating};

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
    /// A person ran one of the project's scripts, for the checkout at `folder`.
    RunScript {
        script: ProjectScript,
        folder: PathBuf,
    },
}

/// A chat a tab has on show, or under what's on show: its timeline and where it scrolled to.
pub(super) struct Shown {
    id: ThreadId,
    pub(super) thread: Entity<ChatThread>,
    pub(super) timeline: Entity<Timeline>,
    pub(super) scroller: Entity<MessageScrollerState>,
    /// Whether there may be older runs than those held, and they aren't being asked for.
    can_fetch_older: bool,
    /// What takes the composer's place, in a subagent's chat.
    bar: Option<Entity<SubagentBar>>,
    _following: Vec<Subscription>,
}

pub struct ChatView {
    pub(super) chats: Entity<Chats>,
    cwd: PathBuf,
    /// The tab's chat, once it has one, then the subagents' chats a person went into, each over
    /// the one it was started from.
    shown: Vec<Shown>,
    pub(super) composer: Entity<ChatComposer>,
    /// How tall the composer last was, which the timeline's end clears.
    composer_height: Pixels,
    /// Whether the pill back to the end shows.
    pill: bool,
    /// Waits to show the pill.
    showing_pill: Option<Task<()>>,
    /// The card that says where the chat works.
    details: Details,
    /// The prompt floating over the timeline, and how tall its card was last laid out.
    pub(super) floating: Option<Floating>,
    card_height: Rc<Cell<Pixels>>,
    /// How far that card is above where it floats: as far as a person scrolled down and not back
    /// up since, all out of sight at first. How far down the list was when last looked at, and how
    /// many times a person had scrolled it then: only what they scroll moves the card.
    hidden: Pixels,
    scrolled_to: Pixels,
    hand_scrolls: u64,
    /// The picture open over the chat.
    pub(super) picture: Option<Entity<Viewer>>,
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
        let subscriptions = vec![
            cx.subscribe_in(&composer, window, |view, _, event, window, cx| {
                view.composer_said(event, window, cx);
            }),
            // A person back from elsewhere may have done something with the branch there.
            cx.observe_window_activation(window, |view, window, cx| {
                if window.is_window_active() {
                    details::window_active_again(view, cx);
                }
            }),
            // Whether prompts float, in open chats at once.
            settings::on_change(cx, Self::float),
        ];
        let mut view = ChatView {
            chats,
            cwd,
            shown: Vec::new(),
            composer,
            composer_height: Pixels::ZERO,
            pill: false,
            showing_pill: None,
            details: Details::default(),
            floating: None,
            card_height: Rc::new(Cell::new(px(CARD_GUESS))),
            hidden: Pixels::MAX,
            scrolled_to: Pixels::ZERO,
            hand_scrolls: 0,
            picture: None,
            _subscriptions: subscriptions,
        };
        if let Some(thread) = opened {
            view.show(thread, None, window, cx);
        }
        view
    }

    fn composer_said(
        &mut self,
        event: &ComposerEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            ComposerEvent::Sent => {
                if let Some(shown) = self.showing() {
                    shown
                        .scroller
                        .update(cx, MessageScrollerState::scroll_to_end);
                }
            }
            ComposerEvent::Created(thread, id) => {
                self.show(thread.clone(), None, window, cx);
                cx.emit(ChatViewEvent::Created(id.clone()));
            }
        }
        cx.notify();
    }

    /// Puts `thread` over what's on show, its timeline following it and `bar` in the composer's
    /// place, if it has one.
    fn show(
        &mut self,
        thread: Entity<ChatThread>,
        bar: Option<Entity<SubagentBar>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (cwd, inset) = (self.cwd.to_string_lossy().into(), self.inset());
        let catalog = self.composer.read(cx).catalog.clone();
        let composer = self.composer.read(cx).focus_handle(cx);
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let timeline = {
            let (thread, scroller) = (thread.clone(), scroller.clone());
            cx.new(|cx| Timeline::new(thread, catalog, scroller, cwd, composer, inset, cx))
        };
        let mut following = vec![
            cx.subscribe(&timeline, |_, _, event: &ChatViewEvent, cx| {
                cx.emit(event.clone());
            }),
            cx.subscribe_in(&timeline, window, Self::went_into),
            cx.subscribe_in(&timeline, window, Self::went_back),
            cx.subscribe_in(&timeline, window, Self::picture_clicked),
            cx.observe(&scroller, |view, _, cx| view.scrolled(cx)),
        ];
        if let Some(bar) = &bar {
            following.push(cx.subscribe_in(
                bar,
                window,
                |view, _, _: &WentToParent, window, cx| {
                    view.came_back(window, cx);
                },
            ));
        }
        if let Some(under) = self.showing() {
            under
                .timeline
                .update(cx, |timeline, cx| timeline.set_covered(true, cx));
        }
        self.shown.push(Shown {
            id: thread.read(cx).id().clone(),
            thread,
            timeline,
            scroller,
            can_fetch_older: true,
            bar,
            _following: following,
        });
    }

    /// A person went for a subagent's chat, which takes the place of what's on show.
    fn went_into(
        &mut self,
        _: &Entity<Timeline>,
        event: &OpenSubagent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let OpenSubagent(id) = event;
        self.open_subagent(id, window, cx);
    }

    fn went_back(
        &mut self,
        timeline: &Entity<Timeline>,
        event: &GoBackTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let GoBackTo(run) = event;
        let thread = timeline.read(cx).thread.read(cx).id().clone();
        let (chats, composer) = (self.chats.clone(), self.composer.clone());
        rewind::confirm(chats, composer, thread, run.clone(), window, cx);
    }

    /// Thread `id`, a subagent's, takes the place of what's on show.
    fn open_subagent(&mut self, id: &ThreadId, window: &mut Window, cx: &mut Context<Self>) {
        let thread = self.chats.update(cx, |chats, cx| chats.thread(id, cx));
        let bar = {
            let (thread, chats) = (thread.clone(), self.chats.clone());
            let catalog = self.composer.read(cx).catalog.clone();
            cx.new(|cx| SubagentBar::new(thread, catalog, chats, cx))
        };
        self.show(thread, Some(bar), window, cx);
        self.showing_changed(window, cx);
    }

    /// Back in the chat under the subagent's.
    fn came_back(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.shown.pop();
        if let Some(under) = self.showing() {
            under
                .timeline
                .update(cx, |timeline, cx| timeline.set_covered(false, cx));
        }
        self.showing_changed(window, cx);
    }

    /// Another chat's on show: the pill was the last one's. A subagent's bar takes the keys, so
    /// Esc comes back from it; back in the chat under it, the prompt is left alone
    /// (`shell/keys.rs`), and the window has them once the bar is gone.
    fn showing_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pill = false;
        self.showing_pill = None;
        self.floating = None;
        self.hidden = Pixels::MAX;
        // Each chat has its own list: what a person scrolls is counted from where this one is.
        if let Some(shown) = self.shown.last() {
            let scroller = shown.scroller.read(cx);
            (self.scrolled_to, self.hand_scrolls) =
                (scroller.scroll_offset(), scroller.hand_scrolls());
        }
        if let Some(bar) = self.bar() {
            window.focus(&bar.read(cx).focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// Whether the details card is up.
    #[cfg(test)]
    pub(super) fn details_up(&self, cx: &App) -> bool {
        self.details.is_up(cx)
    }

    #[cfg(test)]
    pub(super) fn details_folder(&self) -> Option<PathBuf> {
        self.details.folder()
    }

    /// Where the chat works, as the shell says.
    pub(super) fn set_place(&mut self, place: Workplace, cx: &mut Context<Self>) {
        if self.details.set_place(place, cx) {
            cx.notify();
        }
    }

    /// What's on show has been seen, if the window's in front. Only a chat whose tab is shown is
    /// drawn, and every change of its summary draws it again.
    fn note_seen(&self, window: &Window, cx: &mut Context<Self>) {
        let Some(shown) = self.showing().filter(|_| window.is_window_active()) else {
            return;
        };
        if self
            .chats
            .read(cx)
            .summary(&shown.id)
            .is_some_and(|summary| summary.unread)
        {
            let (chats, id) = (self.chats.clone(), shown.id.clone());
            cx.defer(move |cx| chats.update(cx, |chats, cx| chats.saw(&id, cx)));
        }
    }

    /// The chat on show, a subagent's if a person went into one.
    pub(super) fn showing(&self) -> Option<&Shown> {
        self.shown.last()
    }

    /// The bar on show, which is where the composer would be.
    fn bar(&self) -> Option<&Entity<SubagentBar>> {
        self.showing().and_then(|shown| shown.bar.as_ref())
    }

    /// The composer, unless a bar takes its place.
    pub(super) fn composer_on_show(&self) -> Option<&Entity<ChatComposer>> {
        self.bar().is_none().then_some(&self.composer)
    }

    /// A person scrolled: the pill follows, and older runs come as the top gets near.
    fn scrolled(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = self.showing() else {
            return;
        };
        let scroller = shown.scroller.read(cx);
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
        self.reveal(cx);
        self.float(cx);
    }

    /// Asks for the runs before the oldest held, once at a time.
    fn fetch_older(&mut self, cx: &mut Context<Self>) {
        let Some(shown) = self.shown.last_mut().filter(|shown| shown.can_fetch_older) else {
            return;
        };
        let thread = shown.thread.clone();
        let opened = thread.read(cx);
        let Some(oldest) = opened.replica().and_then(|replica| replica.runs.first()) else {
            return;
        };
        let page = (self.chats.read(cx)).history(
            opened.id(),
            Some(oldest.id.clone()),
            super::HISTORY_PAGE as u32,
            cx,
        );
        shown.can_fetch_older = false;
        cx.spawn(async move |this, cx| {
            let page = page.await;
            this.update(cx, |this, cx| {
                let more = match page {
                    Ok(page) => {
                        let more = page.runs.len() == super::HISTORY_PAGE;
                        thread.update(cx, |thread, cx| thread.prepend(page, cx));
                        more
                    }
                    Err(error) => {
                        log::warn!("older runs of a chat: {error}");
                        true
                    }
                };
                if let Some(shown) = this.shown.iter_mut().find(|shown| shown.thread == thread) {
                    shown.can_fetch_older = more;
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Focusable for ChatView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.bar() {
            Some(bar) => bar.read(cx).focus_handle(cx),
            None => self.composer.read(cx).focus_handle(cx),
        }
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        details::keep_fresh(self, cx);
        self.note_seen(window, cx);
        // Once the list is laid out, as following its end moves it without a scroll.
        cx.defer_in(window, |this, _, cx| this.float(cx));
        let timeline = (self.showing()).map(|shown| {
            shown
                .timeline
                .clone()
                .cached(StyleRefinement::default().size_full())
        });
        div()
            .id("chat")
            .relative()
            .size_full()
            .text_color(theme::palette(cx).text)
            // Wherever the keys are in a subagent's chat: clicking its text takes them from the bar.
            .when(self.bar().is_some(), |chat| {
                chat.key_context(KEYS)
                    .on_action(cx.listener(|this, _: &OpenParent, window, cx| {
                        this.came_back(window, cx);
                    }))
            })
            .on_drop(cx.listener(|this, dropped: &ExternalPaths, _, cx| {
                if let Some(composer) = this.composer_on_show() {
                    composer.update(cx, |composer, cx| {
                        composer.attach_paths(dropped.paths(), cx);
                    });
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleDetails, _, cx| {
                this.details.toggle(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, open: &OpenIn, _, cx| details::opened_in(this, open, cx)))
            .map(|chat| with_menu(chat, context_menu::menu))
            .child(details::measure(cx))
            .children(timeline)
            .children(self.floating_prompt(window, cx))
            .children(self.scroll_pill(window, cx))
            .child(details::floating(self, window, cx))
            .child(self.composer_in_place(cx))
            .children(self.picture_over(cx))
    }
}
