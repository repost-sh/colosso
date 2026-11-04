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

mod asked;
mod footer;
mod model_picker;
mod question;

use std::path::PathBuf;

use gpui_kit::base::ElementExt as _;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, InteractiveElement as _, IntoElement, KeyContext, ParentElement as _,
    Render, SharedString, Styled as _, Subscription, Window, div, px,
};
use koyo_agent::client::ClientError;
#[cfg(test)]
use koyo_agent::protocol::ids::RequestId;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
#[cfg(test)]
use koyo_agent::protocol::methods::UpdateParams;
use koyo_agent::protocol::methods::{Catalog, ThreadsCreate};
use koyo_agent::protocol::model::Prompt;
use koyo_agent::protocol::requests::{Decision, Response};
use koyo_agent::replica::Replica;

use self::asked::Asked;
use self::footer::{COMPOSER, ROOM_FOR_LABELS, Settings, labels_fitting};
use self::model_picker::{ModelPicker, PICKING};
use super::thread::ChatThread;
use super::view::COLUMN;
use super::{Chats, Connection};
use crate::native::liquid;
use crate::shell::glass;
use crate::theme::{self, Palette};

pub(super) fn bind_keys(cx: &mut App) {
    asked::bind_keys(cx);
    footer::bind_keys(cx);
    model_picker::bind_keys(cx);
    question::bind_keys(cx);
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
    /// Why the last thing a person asked of it didn't happen.
    error: Option<SharedString>,
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
    picker: Option<ModelPicker>,
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
        let subscriptions = vec![
            cx.subscribe_in(&text, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.send(window, cx);
                }
            }),
            cx.observe(&chats, |this, chats, cx| {
                let connection = chats.read(cx).connection().clone();
                if this.connection != connection {
                    this.connection = connection;
                    cx.notify();
                }
            }),
            // What the picker points at may not be there any more.
            cx.observe(&catalog, |this, _, _| this.picker = None),
        ];
        let connection = chats.read(cx).connection().clone();
        let mut composer = ChatComposer {
            chats,
            catalog,
            cwd,
            place,
            thread: None,
            creating: false,
            text,
            error: None,
            failed: None,
            connection,
            asked: Asked::Nothing,
            settings: Settings::default(),
            context: None,
            picker: None,
            labels: ROOM_FOR_LABELS.len(),
            _subscriptions: subscriptions,
            #[cfg(test)]
            renders: 0,
            #[cfg(test)]
            said: Vec::new(),
            #[cfg(test)]
            updated: Vec::new(),
        };
        if let Some(thread) = thread {
            composer.send_to_thread(thread, window, cx);
        }
        composer
    }

    /// From now on what's sent goes to `thread`.
    fn send_to_thread(
        &mut self,
        thread: Entity<ChatThread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._subscriptions.push(cx.observe_in(&thread, window, |this, _, window, cx| {
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
        let asking = replica.and_then(Replica::asking);
        if self.asked.request() != asking.map(|asked| &asked.id) {
            self.asked = asking.map_or(Asked::Nothing, Asked::of);
            let placeholder = self.asked.placeholder();
            self.text.update(cx, |text, cx| text.set_placeholder(placeholder, window, cx));
            cx.notify();
        }
        if self.failed != failed {
            self.failed = failed;
            cx.notify();
        }
        if let Some(settings) = settings.filter(|settings| *settings != self.settings) {
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

    /// Sends what's typed: the composer clears at once, and the message shows when the host says
    /// so.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text.read(cx).value().trim().to_owned();
        if text.is_empty() || self.creating {
            return;
        }
        self.text.update(cx, |composer, cx| composer.set_value("", window, cx));
        self.error = None;
        cx.emit(ComposerEvent::Sent);
        cx.notify();
        match &self.asked {
            Asked::Approval { .. } => {
                let response = Response::Approval {
                    decision: Decision::Deny,
                    message: Some(text.clone()),
                };
                self.respond(response, Some(text), window, cx);
                return;
            }
            // With all of it answered, and on its way, this is just the next message.
            Asked::Question(drawer) if drawer.shown().is_some() => {
                self.next_question(Some(text), window, cx);
                return;
            }
            _ => {}
        }
        let prompt = Prompt {
            message_id: MessageId::fresh(),
            text,
            attachments: Vec::new(),
        };
        match &self.thread {
            Some(thread) => {
                let id = thread.read(cx).id().clone();
                self.send_to(&id, prompt, window, cx);
            }
            None => self.create_and_send(prompt, window, cx),
        }
    }

    fn send_to(
        &mut self,
        id: &ThreadId,
        prompt: Prompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text = prompt.text.clone();
        let sent = self.chats.read(cx).send(id, prompt);
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = sent.await {
                this.update_in(cx, |this, window, cx| {
                    this.not_sent(&text, &error, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// A new chat's first send: its thread is made, then opened, then told the message.
    fn create_and_send(&mut self, prompt: Prompt, window: &mut Window, cx: &mut Context<Self>) {
        self.creating = true;
        let params = self.settings.to_create(&self.cwd);
        let made = self.chats.read(cx).ordered::<ThreadsCreate>(params);
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match made {
                    Ok(thread) => {
                        let id = thread.id;
                        let opened = this.chats.update(cx, |chats, cx| chats.thread(&id, cx));
                        this.send_to_thread(opened.clone(), window, cx);
                        cx.emit(ComposerEvent::Created(opened, id.clone()));
                        this.send_to(&id, prompt, window, cx);
                    }
                    Err(error) => this.not_sent(&prompt.text, &error, window, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn not_sent(
        &mut self,
        text: &str,
        why: &ClientError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        log::warn!("a message didn't go: {why}");
        self.error = Some(why.to_string().into());
        self.give_back(text, window, cx);
        cx.notify();
    }

    /// What was typed comes back, unless something else has been typed since.
    fn give_back(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |composer, cx| {
            if composer.value().is_empty() {
                composer.insert(text.to_owned(), window, cx);
            }
        });
    }

    /// What's in the way: why the last send failed, why the chat can't be shown, or how things
    /// are with the host.
    fn note(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (text, restart): (SharedString, bool) = match (&self.error, &self.failed, &self.connection) {
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
        let on_glass = liquid::backdrop(keyed, px(18.), window, cx);
        let surface = div()
            .relative()
            .w_full()
            .flex()
            .flex_col()
            .gap_2()
            .pt_3()
            .pb_2()
            .px_3()
            .rounded(px(18.))
            .occlude();
        let typed = self.text.read(cx).value();
        let surface = liquid::floating(surface, on_glass, p)
            .children(self.drawer(&typed, p, cx))
            .child(Textarea::new(&self.text).appearance(false))
            .child(self.footer(&typed, p, cx))
            .children(self.model_picker(p, window, cx));
        let mut keys = KeyContext::default();
        keys.add(COMPOSER);
        // The picker has the keys while it's open, ahead of what's asked.
        if self.picker.is_some() {
            keys.add(PICKING);
        } else if let Some(answering) = self.asked.key_context().filter(|_| typed.is_empty()) {
            keys.add(answering);
        }
        let composer = cx.weak_entity();
        let labels = self.labels;
        Self::hears_picker(Self::hears_footer(Self::hears_answers(div(), cx), cx), cx)
            .key_context(keys)
            .relative()
            .w_full()
            .max_w(px(COLUMN))
            .px_6()
            .flex()
            .flex_col()
            .child(
                div()
                    // Under the picker, whose glass only hides what's drawn under the glass.
                    .when(self.is_picking(), gpui_kit::Styled::invisible)
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
                    .children(self.note(p, cx)),
            )
            .child(surface)
            // Told a frame late, as it's measured once drawn.
            .on_prepaint(move |bounds, _, cx| {
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
