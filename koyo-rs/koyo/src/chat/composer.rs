//! The composer (`docs/chat.md` 5.3): one surface of glass where a chat's messages are written and
//! sent from. A view of its own, as its caret blinks and what's typed changes with every key, none
//! of which is the timeline's to draw. If a message doesn't go the text comes back, with why.
//!
//! What it shows of the thread and the host it holds a copy of. A view is drawn again each time
//! what it read is updated, told or not, and every token updates both.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Window, div, px,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::model::Prompt;

use super::thread::ChatThread;
use super::view::COLUMN;
use super::{Chats, Connection};
use crate::native::liquid;
use crate::shell::{glass, icon};
use crate::theme::{self, Palette};

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
    cwd: PathBuf,
    /// Where the chat is, as it says when it's empty: "koyo / main".
    place: SharedString,
    thread: Option<Entity<ChatThread>>,
    /// Waiting for a new chat's thread, which a second send would make again.
    creating: bool,
    pub(super) text: Entity<TextareaState>,
    /// Why the last send didn't go.
    error: Option<SharedString>,
    /// Why the chat can't be shown.
    failed: Option<SharedString>,
    /// How things are with the host.
    connection: Connection,
    _subscriptions: Vec<Subscription>,
    #[cfg(test)]
    pub(super) renders: usize,
}

impl EventEmitter<ComposerEvent> for ChatComposer {}

impl ChatComposer {
    pub(super) fn new(
        chats: Entity<Chats>,
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
                .placeholder("Ask Claude…")
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
        ];
        let connection = chats.read(cx).connection().clone();
        let mut composer = ChatComposer {
            chats,
            cwd,
            place,
            thread: None,
            creating: false,
            text,
            error: None,
            failed: None,
            connection,
            _subscriptions: subscriptions,
            #[cfg(test)]
            renders: 0,
        };
        if let Some(thread) = thread {
            composer.send_to_thread(thread, cx);
        }
        composer
    }

    /// From now on what's sent goes to `thread`.
    fn send_to_thread(&mut self, thread: Entity<ChatThread>, cx: &mut Context<Self>) {
        self.failed = thread.read(cx).failed().cloned();
        self._subscriptions.push(cx.observe(&thread, |this, thread, cx| {
            let failed = thread.read(cx).failed().cloned();
            if this.failed != failed {
                this.failed = failed;
                cx.notify();
            }
        }));
        self.thread = Some(thread);
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
        cx.notify();
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
        let made = self.chats.read(cx).create(&self.cwd);
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match made {
                    Ok(thread) => {
                        let id = thread.id;
                        let opened = this.chats.update(cx, |chats, cx| chats.thread(&id, cx));
                        this.send_to_thread(opened.clone(), cx);
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
        self.text.update(cx, |composer, cx| {
            if composer.value().is_empty() {
                composer.set_value(text.to_owned(), window, cx);
            }
        });
        cx.notify();
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

    fn footer(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        let text = self.text.read(cx).value();
        let can_send = !text.trim().is_empty() && !self.creating;
        let (fill, mark): (Hsla, Hsla) = if can_send {
            (p.accent, p.on_text)
        } else {
            (p.text.opacity(0.08), p.muted)
        };
        let send = div()
            .id("chat-send")
            .size(px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(fill)
            .text_color(mark)
            .child(icon(IconName::ArrowUp, 15.))
            .when(can_send, |button| {
                button
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| this.send(window, cx)))
            });
        div().flex().items_center().justify_end().child(send)
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
        let surface = liquid::floating(surface, on_glass, p)
            .child(Textarea::new(&self.text).appearance(false))
            .child(self.footer(p, cx));
        div()
            .w_full()
            .max_w(px(COLUMN))
            .px_6()
            .flex()
            .flex_col()
            .when(self.is_new(), |column| {
                column.child(
                    div()
                        .pb_4()
                        .flex()
                        .justify_center()
                        .text_lg()
                        .text_color(p.muted)
                        .child(format!("Chat in {}", self.place)),
                )
            })
            .children(self.note(p, cx))
            .child(surface)
    }
}
