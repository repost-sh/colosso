//! The composer (`docs/chat.md` 5.3): one surface of glass where a chat's messages are written and
//! sent from. A view of its own, as its caret blinks and what's typed changes with every key, none
//! of which is the timeline's to draw. If a message doesn't go the text comes back, with why.
//!
//! All a person says to the chat goes through here, so answers to what the agent asks do too: from
//! the keys, the drawers above what's typed, the buttons of a row, or what's typed itself, which
//! declines an approval and answers a question.
//!
//! What it shows of the thread and the host it holds a copy of. A view is drawn again each time
//! what it read is updated, told or not, and every token updates both.

pub(super) mod activity;
mod asked;
mod attach;
mod context;
mod draft;
mod effort;
mod footer;
mod markdown;
mod missing;
mod model_menu;
mod outside;
mod question;
mod sending;
mod suggest;
mod turn;

pub(super) use sending::sent;

pub use asked::ImplementPlan;
pub use footer::TogglePlan;
pub use model_menu::OpenModelMenu;
pub use question::SkipQuestion;
pub use turn::{RecallPrompt, StopTurn};

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::base::{ElementExt as _, TextViewDefaults};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyContext, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Task, Window, div, px,
};
#[cfg(test)]
use koyo_agent::protocol::ids::RequestId;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::methods::Catalog;
#[cfg(test)]
use koyo_agent::protocol::methods::UpdateParams;
use koyo_agent::protocol::model::Prompt;
#[cfg(test)]
use koyo_agent::protocol::requests::Response;
use koyo_agent::replica::Replica;

use self::activity::{Activity, Placement};
use self::asked::Asked;
use self::attach::Attached;
use self::footer::{COMPOSER, ROOM_FOR_LABELS, labels_fitting};
use self::suggest::{SUGGESTING, Suggestions};
use self::turn::{RECALLING, STOPPABLE, Turn};
use super::chosen::Settings;
use super::drafts::{self, DraftOf};
use super::thread::ChatThread;
use super::{Chats, Connection};
use crate::menu::Trigger;
use crate::native::liquid;
use crate::shell::glass;
use crate::theme::fonts::at_chat_size;
use crate::theme::{self, Palette};

/// How round the composer's corners are, its glass's and its edge's.
const RADIUS: f32 = 18.;

pub(super) fn bind_keys(cx: &mut App) {
    asked::bind_keys(cx);
    footer::bind_keys(cx);
    model_menu::bind_keys(cx);
    question::bind_keys(cx);
    suggest::bind_keys(cx);
    turn::bind_keys(cx);
}

/// What the tab hears from its composer.
#[derive(Clone, Debug)]
pub(super) enum ComposerEvent {
    /// What was typed is on its way.
    Sent,
    /// The first send made the chat's thread, which the composer now sends to.
    Created(Entity<ChatThread>, ThreadId),
}

pub(super) struct ChatComposer {
    chats: Entity<Chats>,
    /// What the chat can choose from.
    pub(super) catalog: Entity<Catalog>,
    cwd: PathBuf,
    /// Where the chat is, as it says when it's empty: "koyo / main".
    place: SharedString,
    thread: Option<Entity<ChatThread>>,
    /// Waiting for a new chat's thread, which a second send would make again.
    creating: bool,
    pub(super) text: Entity<TextareaState>,
    /// The text's version its markdown was last laid over it for.
    styled: Option<u64>,
    /// Why the last thing a person asked of it didn't happen.
    pub(super) error: Option<SharedString>,
    /// Why the chat can't be shown.
    failed: Option<SharedString>,
    /// How things are with the host.
    connection: Connection,
    /// What the agent waits on a person for now.
    asked: Asked,
    /// What the chat is set to.
    settings: Settings,
    /// How full the conversation is, as of the last run that said: what it holds, and can.
    context: Option<(u64, u64)>,
    /// What opens the menu of models, the slider of efforts, and how full the conversation is.
    model_menu: Trigger,
    effort_menu: Trigger,
    context_menu: Trigger,
    /// What the word being typed completes to.
    suggest: Option<Suggestions>,
    /// Where the word starts that was told not to complete, until the caret leaves it.
    dismissed: Option<usize>,
    /// What's attached to the message being written.
    attached: Vec<Attached>,
    /// How many tiles there've been, which each goes by.
    tiles_made: usize,
    /// The message that last didn't go, whose id it keeps if it's sent again as it was.
    unsent: Option<Prompt>,
    /// Where what's attached is kept.
    attachments: Option<PathBuf>,
    /// The chat whose draft it writes to, unless another composer is.
    draft: Option<DraftOf>,
    /// The text's version and the caret, as last written to its draft.
    draft_written: Option<(u64, usize)>,
    /// Where the chat's turn is.
    turn: Turn,
    /// The edge that shows the chat working, which the tab lays over it.
    pub(super) activity: Entity<Activity>,
    /// Where it and its glass were last laid out, for the edge.
    placement: Rc<Placement>,
    /// Waits for a stop to have taken too long.
    forcing: Option<Task<()>>,
    /// Whether a stop has taken long enough for the turn to be ended instead.
    can_force: bool,
    /// Whether ⌘'s down, which the send button shows.
    secondary_held: bool,
    /// How many of the footer's chips say what they are, as wide as it is.
    labels: usize,
    _subscriptions: Vec<Subscription>,
    #[cfg(test)]
    pub(super) renders: usize,
    /// What it answered.
    #[cfg(test)]
    pub(super) said: Vec<(RequestId, Response)>,
    /// What it asked the host to change of the chat.
    #[cfg(test)]
    pub(super) updated: Vec<UpdateParams>,
    /// How it asked messages to go.
    #[cfg(test)]
    pub(super) sent_as: Vec<koyo_agent::protocol::methods::Delivery>,
    /// What it asked the host of the turn and the queue.
    #[cfg(test)]
    pub(super) asked_host: Vec<(&'static str, serde_json::Value)>,
}

impl EventEmitter<ComposerEvent> for ChatComposer {}

impl ChatComposer {
    pub(super) fn new(
        chats: Entity<Chats>,
        catalog: Entity<Catalog>,
        cwd: PathBuf,
        place: SharedString,
        thread: Option<Entity<ChatThread>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(true)
                .placeholder(Asked::Nothing.placeholder())
        });
        let claude_missing = chats.read(cx).claude_missing.clone();
        let subscriptions = vec![
            cx.subscribe_in(&text, window, |this, _, event, window, cx| match event {
                InputEvent::PressEnter {
                    secondary: true,
                    shift: false,
                } => this.queue(window, cx),
                InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
                _ => {}
            }),
            cx.observe(&chats, |this, chats, cx| {
                let connection = chats.read(cx).connection().clone();
                if this.connection != connection {
                    this.connection = connection;
                    cx.notify();
                }
            }),
            cx.observe(&claude_missing, |_, _, cx| cx.notify()),
            // What the menus were opened on may not be there any more, nor the commands listed.
            cx.observe(&catalog, |this, _, cx| {
                this.model_menu.close(cx);
                this.effort_menu.close(cx);
                this.search(cx);
                cx.notify();
            }),
            cx.observe(&text, |this, _, cx| {
                this.style_text(cx);
                this.follow_text(cx);
                this.keep_draft(cx);
            }),
            // The text views' colours, which its markdown is drawn in, follow the theme.
            cx.observe_global::<TextViewDefaults>(Self::restyle),
            // What's attached changes the composer and not what's typed.
            cx.observe_self(|this, cx| this.keep_draft(cx)),
            cx.on_release(|this: &mut Self, cx| this.let_go(cx)),
        ];
        let connection = chats.read(cx).connection().clone();
        let placement = Rc::new(Placement::default());
        let activity = cx.new(|cx| Activity::new(placement.clone(), window, cx));
        let mut composer = ChatComposer {
            chats,
            catalog,
            cwd,
            place,
            thread: None,
            creating: false,
            text,
            styled: None,
            error: None,
            failed: None,
            connection,
            asked: Asked::Nothing,
            settings: Settings::default(),
            context: None,
            model_menu: Trigger::new(("chat-model-menu", cx.entity_id().as_u64())),
            effort_menu: Trigger::new(("chat-effort-menu", cx.entity_id().as_u64())),
            context_menu: Trigger::new(("chat-context-menu", cx.entity_id().as_u64())),
            suggest: None,
            dismissed: None,
            attached: Vec::new(),
            tiles_made: 0,
            unsent: None,
            attachments: drafts::attachments_folder(cx),
            draft: None,
            draft_written: None,
            turn: Turn::default(),
            activity,
            placement,
            forcing: None,
            can_force: false,
            secondary_held: false,
            labels: ROOM_FOR_LABELS.len(),
            _subscriptions: subscriptions,
            #[cfg(test)]
            renders: 0,
            #[cfg(test)]
            said: Vec::new(),
            #[cfg(test)]
            updated: Vec::new(),
            #[cfg(test)]
            sent_as: Vec::new(),
            #[cfg(test)]
            asked_host: Vec::new(),
        };
        if let Some(thread) = thread {
            composer.send_to_thread(thread, window, cx);
        }
        composer.bring_back_draft(window, cx);
        composer
    }

    /// From now on what's sent goes to `thread`.
    fn send_to_thread(
        &mut self,
        thread: Entity<ChatThread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions
            .push(cx.observe_in(&thread, window, |this, _, window, cx| {
                this.thread_changed(window, cx);
            }));
        self.thread = Some(thread);
        self.thread_changed(window, cx);
    }

    /// Takes what it shows of its thread, and is drawn only if that's new.
    fn thread_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let held = self.thread.as_ref().map(|thread| thread.read(cx));
        let failed = held.and_then(|thread| thread.failed().cloned());
        let replica = held.and_then(ChatThread::replica);
        let settings = replica.map(|replica| Settings::of(&replica.thread));
        let context = replica.and_then(Replica::context);
        let turn = replica.map_or_else(Turn::default, Turn::of);
        let working = replica.is_some_and(Replica::working);
        let asking = replica.and_then(Replica::asking);
        if self.asked.request() != asking.map(|asked| &asked.id) {
            self.asked = asking.map_or(Asked::Nothing, Asked::of);
            let placeholder = self.asked.placeholder();
            self.text
                .update(cx, |text, cx| text.set_placeholder(placeholder, window, cx));
            cx.notify();
        }
        if self.failed != failed {
            self.failed = failed;
            cx.notify();
        }
        self.follow_turn(turn, cx);
        // Told only when it changes, as every token comes through here.
        if self.activity.read(cx).working() != working {
            (self.activity).update(cx, |activity, cx| activity.set_working(working, cx));
        }
        if let Some(settings) = settings.filter(|settings| *settings != self.settings) {
            // The slider's stops are the efforts of the model it opened on.
            if settings.model != self.settings.model {
                self.effort_menu.close(cx);
            }
            self.settings = settings;
            cx.notify();
        }
        if self.context != context {
            self.context = context;
            cx.notify();
        }
    }

    /// Whether nothing's been said here yet, nor is on its way.
    pub(super) fn is_new(&self) -> bool {
        self.thread.is_none() && !self.creating
    }

    /// What's in the way: why the last send failed, why the chat can't be shown, or how things
    /// are with the host.
    fn note(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (text, restart): (SharedString, bool) =
            match (&self.error, &self.failed, &self.connection) {
                (Some(error), _, _) => (error.clone(), false),
                (None, Some(failed), _) => (failed.clone(), false),
                (None, None, Connection::Failed(why)) => (why.clone(), false),
                (None, None, Connection::OtherBuild) => (
                    "Chats are running on another build of Koyo. Restart them to use this one; \
                 running turns stop."
                        .into(),
                    true,
                ),
                _ => return None,
            };
        let row = div()
            .pb_2()
            .flex()
            .items_center()
            .gap_2()
            .text_sm()
            .text_color(p.danger)
            .child(text)
            .when(restart, |row| {
                row.child(
                    glass("chat-restart-host", p.text, cx)
                        .small()
                        .label("Restart chats")
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            this.chats.update(cx, Chats::replace_host);
                        })),
                )
            });
        Some(row.into_any_element())
    }
}

impl Focusable for ChatComposer {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.text.read(cx).focus_handle(cx)
    }
}

/// The composer, with the note over it and, in an empty chat, where the chat is.
impl Render for ChatComposer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let p = theme::palette(cx);
        let keyed = ElementId::NamedInteger("chat-composer-glass".into(), cx.entity_id().as_u64());
        let on_glass = liquid::backdrop(keyed, px(RADIUS), window, cx);
        let surface = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .pt_3()
            .pb_2()
            .px_3()
            .rounded(px(RADIUS))
            .occlude();
        let typed = self.text.read(cx).value();
        let surface = Self::takes_drops(liquid::floating(surface, on_glass, p), p, cx)
            .children(self.drawer(&typed, p, cx))
            .children(self.held_queue(p, cx))
            .children(self.tiles(p, cx))
            .child(at_chat_size(
                Textarea::new(&self.text)
                    .appearance(false)
                    .on_paste(self.takes_paste(cx)),
                cx,
            ))
            .child(self.footer(&typed, p, cx))
            .children(self.suggestions(p, window, cx))
            .on_mouse_down_out(cx.listener(Self::pressed_outside));
        // Measured round it, so what's measured is its glass, or its hairline where there's none.
        let placement = self.placement.clone();
        let surface = div()
            .w_full()
            .child(surface)
            .on_prepaint(move |bounds, _, _| placement.glass_at(bounds));
        let mut keys = KeyContext::default();
        keys.add(COMPOSER);
        // A popover has the keys while it's open, ahead of what's asked.
        if self.suggesting() {
            keys.add(SUGGESTING);
        } else if typed.is_empty() {
            keys.add(RECALLING);
            if let Some(answering) = self.asked.key_context() {
                keys.add(answering);
            }
            if self.turn.is_on() {
                keys.add(STOPPABLE);
            }
        }
        let composer = cx.weak_entity();
        let labels = self.labels;
        let placement = self.placement.clone();
        let hearing = Self::hears_turn(Self::hears_footer(Self::hears_answers(div(), cx), cx), cx);
        self.hears_suggestions(Self::hears_model_menu(hearing, cx), cx)
            .key_context(keys)
            .relative()
            // As wide as the column the tab holds it in, with the edge over it.
            .w_full()
            .px_6()
            .flex()
            .flex_col()
            .child(
                div()
                    .when(self.suggesting(), gpui_kit::Styled::invisible)
                    .when(self.is_new(), |above| {
                        above.child(
                            div()
                                .pb_4()
                                .flex()
                                .justify_center()
                                .text_lg()
                                .text_color(p.muted)
                                .child(format!("Chat in {}", self.place)),
                        )
                    })
                    .children(self.claude_missing_note(p, cx))
                    .children(self.note(p, cx)),
            )
            .child(surface)
            // Told a frame late, as it's measured once drawn.
            .on_prepaint(move |bounds, _, cx| {
                placement.composer_at(bounds);
                let fitting = labels_fitting(bounds.size.width);
                if fitting != labels {
                    cx.defer(move |cx| {
                        composer
                            .update(cx, |composer, cx| {
                                composer.labels = fitting;
                                cx.notify();
                            })
                            .ok();
                    });
                }
            })
    }
}

#[cfg(test)]
mod tests;
