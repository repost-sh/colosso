//! A chat tab: the timeline and the composer (`docs/chat.md` 5.3). It draws what its thread holds
//! and sends what's typed; the thread comes with the first send of a new chat.

use std::path::PathBuf;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::message_scroller::{MessageScroller, MessageScrollerState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div, px, relative,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::model::{ItemBody, Prompt};

use super::thread::ChatThread;
use super::{Chats, Connection};
use crate::shell::glass;
use crate::theme::{self, Palette};

/// The widest the timeline and the composer get.
const COLUMN: f32 = 760.;

/// What the shell hears from a chat tab.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChatViewEvent {
    /// The first send made the chat's thread.
    Created(ThreadId),
}

pub struct ChatView {
    chats: Entity<Chats>,
    cwd: PathBuf,
    thread: Option<Entity<ChatThread>>,
    /// Waiting for a new chat's thread, which a second send would make again.
    creating: bool,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<TextareaState>,
    /// Why the last send didn't go.
    error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ChatViewEvent> for ChatView {}

impl ChatView {
    pub fn new(
        chats: Entity<Chats>,
        cwd: PathBuf,
        thread: Option<ThreadId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        chats.update(cx, Chats::connect);
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(true)
                .placeholder("Ask Claude…")
        });
        let scroller = cx.new(|cx| MessageScrollerState::new(0, cx));
        let subscriptions = vec![
            cx.subscribe_in(&composer, window, |this, _, event, window, cx| {
                if let InputEvent::PressEnter { shift: false, .. } = event {
                    this.send(window, cx);
                }
            }),
            cx.observe(&chats, |_, _, cx| cx.notify()),
        ];
        let mut view = ChatView {
            chats,
            cwd,
            thread: None,
            creating: false,
            scroller,
            composer,
            error: None,
            _subscriptions: subscriptions,
        };
        if let Some(id) = thread {
            let thread = view.chats.update(cx, |chats, cx| chats.thread(&id, cx));
            view.show(thread, cx);
        }
        view
    }

    /// From now on the tab shows `thread`.
    fn show(&mut self, thread: Entity<ChatThread>, cx: &mut Context<Self>) {
        let observed = cx.observe(&thread, |view, thread, cx| view.thread_changed(&thread, cx));
        self._subscriptions.push(observed);
        self.thread_changed(&thread, cx);
        self.thread = Some(thread);
    }

    /// The timeline has as many rows as the thread has items.
    fn thread_changed(&self, thread: &Entity<ChatThread>, cx: &mut Context<Self>) {
        let count = (thread.read(cx).replica()).map_or(0, |replica| replica.items.len());
        self.scroller.update(cx, |scroller, cx| {
            let held = scroller.item_count();
            if count > held {
                scroller.append(count - held, cx);
            } else if count < held {
                scroller.reset(count, cx);
            }
        });
        cx.notify();
    }

    /// Sends what's typed: the composer clears at once, and the message shows when the host
    /// says so. If it doesn't go the text comes back, with why.
    fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).value().trim().to_owned();
        if text.is_empty() || self.creating {
            return;
        }
        self.composer.update(cx, |composer, cx| {
            composer.set_value("", window, cx);
        });
        self.error = None;
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
                        this.show(opened, cx);
                        cx.emit(ChatViewEvent::Created(id.clone()));
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
        self.composer.update(cx, |composer, cx| {
            if composer.value().is_empty() {
                composer.set_value(text.to_owned(), window, cx);
            }
        });
        cx.notify();
    }

    fn timeline(&self, p: Palette) -> impl IntoElement {
        let thread = self.thread.clone();
        MessageScroller::new("chat-timeline", self.scroller.clone(), move |index, _, cx| {
            let row = thread.as_ref().and_then(|thread| row(thread, index, p, cx));
            div()
                .w_full()
                .max_w(px(COLUMN))
                .mx_auto()
                .px_6()
                .py_2()
                .children(row)
        })
        .size_full()
    }

    fn composer(&self, p: Palette, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .max_w(px(COLUMN))
            .mx_auto()
            .px_6()
            .pb_4()
            .pt_2()
            .children(self.note(p, cx))
            .child(
                div()
                    .p_3()
                    .rounded(px(14.))
                    .border_1()
                    .border_color(p.hairline)
                    .bg(p.raised)
                    .child(Textarea::new(&self.composer).appearance(false)),
            )
    }

    /// What's in the way: why the last send failed, why the chat can't be shown, or how things
    /// are with the host.
    fn note(&self, p: Palette, cx: &mut Context<Self>) -> Option<AnyElement> {
        let failed = (self.thread.as_ref()).and_then(|thread| thread.read(cx).failed().cloned());
        let connection = self.chats.read(cx).connection().clone();
        let (text, restart): (SharedString, bool) = match (&self.error, failed, connection) {
            (Some(error), _, _) => (error.clone(), false),
            (None, Some(failed), _) => (failed, false),
            (None, None, Connection::Failed(why)) => (why, false),
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

/// The row of the thread's item at `index`.
fn row(thread: &Entity<ChatThread>, index: usize, p: Palette, cx: &mut App) -> Option<AnyElement> {
    let item = thread.read(cx).replica()?.items.get(index)?.clone();
    let element = match &item.body {
        ItemBody::UserMessage(message) => div()
            .flex()
            .justify_end()
            .child(
                div()
                    .max_w(relative(0.8))
                    .px_3()
                    .py_2()
                    .rounded(px(12.))
                    .bg(p.raised)
                    .text_color(p.text)
                    .child(message.text.clone()),
            )
            .into_any_element(),
        ItemBody::AssistantText(text) => {
            let view = thread.update(cx, |thread, cx| thread.text(&item.id, &text.text, cx));
            TextView::new(&view).into_any_element()
        }
        ItemBody::Reasoning(_) => div()
            .text_sm()
            .text_color(p.muted)
            .child("Thinking…")
            .into_any_element(),
        ItemBody::Tool(tool) => div()
            .text_sm()
            .text_color(p.muted)
            .child(tool.label.clone())
            .into_any_element(),
        _ => div().into_any_element(),
    };
    Some(element)
}

impl Focusable for ChatView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.composer.read(cx).focus_handle(cx)
    }
}

impl Render for ChatView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .id("chat")
            .size_full()
            .flex()
            .flex_col()
            .text_color(p.text)
            .child(div().flex_1().min_h_0().child(self.timeline(p)))
            .child(self.composer(p, cx))
    }
}
