//! Sending what's typed and what's attached: the composer clears at once, and the message shows when
//! the host says so. If it doesn't go it all comes back, with why.

use gpui_kit::{Context, Window};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{MessageId, ThreadId};
use koyo_agent::protocol::methods::{CreateParams, Delivery, ThreadsCreate};
use koyo_agent::protocol::model::{Prompt, Thread};
use koyo_agent::protocol::requests::{Decision, Response};

use super::asked::Asked;
use super::{ChatComposer, ComposerEvent};
use crate::chat::Chats;
use crate::history;

impl Chats {
    /// Makes a chat as `params` say, reaching the host first so it hears Koyo's settings before
    /// it opens the chat's session.
    pub(crate) fn create(
        &mut self,
        params: CreateParams,
        cx: &mut Context<Self>,
    ) -> impl Future<Output = Result<Thread, ClientError>> + use<> {
        self.connect(cx);
        self.ordered::<ThreadsCreate>(params)
    }
}

impl ChatComposer {
    /// Whether what's typed answers what's asked, which takes only words: what's attached stays for
    /// the message after.
    fn typed_answers(&self) -> bool {
        match &self.asked {
            Asked::Approval { .. } => true,
            // With all of it answered, and on its way, what's typed is just the next message.
            Asked::Question(drawer) => drawer.shown().is_some(),
            _ => false,
        }
    }

    /// Whether there's something to send, `typed` being what's typed.
    pub(super) fn can_send(&self, typed: &str) -> bool {
        let words = !typed.trim().is_empty();
        if self.creating {
            false
        } else if self.typed_answers() {
            words
        } else {
            !self.attaching() && (words || !self.attached.is_empty())
        }
    }

    /// Sends what's typed and what's attached: a turn of its own, or into the one that's on.
    pub(in crate::chat) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send_as(Delivery::Auto, window, cx);
    }

    /// Says `text`, as if it were typed and sent: the launcher's message to a new chat.
    pub(in crate::chat) fn say(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |composer, cx| {
            composer.set_value(text.to_owned(), window, cx);
        });
        self.send(window, cx);
    }

    /// Sends what's typed and what's attached, after the turn that's on.
    pub(super) fn queue(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.send_as(Delivery::Queue, window, cx);
    }

    fn send_as(&mut self, delivery: Delivery, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.text.read(cx).value().trim().to_owned();
        if !self.can_send(&text) {
            return;
        }
        self.text
            .update(cx, |composer, cx| composer.set_value("", window, cx));
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
            Asked::Question(drawer) if drawer.shown().is_some() => {
                self.next_question(Some(text), window, cx);
                return;
            }
            _ => {}
        }
        let attachments = self.take_attached();
        // The same message sent again keeps its id, so it can't go twice if it did go the first time.
        let message_id = (self.unsent.take())
            .filter(|failed| failed.text == text && failed.attachments == attachments)
            .map_or_else(MessageId::fresh, |failed| failed.message_id);
        if !text.is_empty() {
            let place = history::place_of(&self.cwd, cx);
            history::used(history::Kind::Prompt, &text, &place, cx);
        }
        let prompt = Prompt {
            message_id,
            text,
            attachments,
        };
        self.settle_effort(cx);
        match &self.thread {
            Some(thread) => {
                let id = thread.read(cx).id().clone();
                self.send_to(&id, prompt, delivery, window, cx);
            }
            None => self.create_and_send(prompt, window, cx),
        }
    }

    fn send_to(
        &mut self,
        id: &ThreadId,
        prompt: Prompt,
        delivery: Delivery,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        self.sent_as.push(delivery);
        let sent = self.chats.read(cx).send(id, prompt.clone(), delivery);
        cx.spawn_in(window, async move |this, cx| {
            if let Err(error) = sent.await {
                this.update_in(cx, |this, window, cx| {
                    this.prompt_not_sent(prompt, &error, window, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// A new chat's first send: its thread is made, then opened, then told the message.
    fn create_and_send(&mut self, prompt: Prompt, window: &mut Window, cx: &mut Context<Self>) {
        self.creating = true;
        let made = (self.chats).update(cx, |chats, cx| {
            chats.create(self.settings.to_create(&self.cwd), cx)
        });
        cx.spawn_in(window, async move |this, cx| {
            let made = made.await;
            this.update_in(cx, |this, window, cx| {
                this.creating = false;
                match made {
                    Ok(thread) => {
                        let id = thread.id;
                        let opened = this.chats.update(cx, |chats, cx| chats.thread(&id, cx));
                        this.send_to_thread(opened.clone(), window, cx);
                        this.draft_goes_to(&id, cx);
                        cx.emit(ComposerEvent::Created(opened, id.clone()));
                        this.send_to(&id, prompt, Delivery::Auto, window, cx);
                    }
                    Err(error) => this.prompt_not_sent(prompt, &error, window, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn prompt_not_sent(
        &mut self,
        prompt: Prompt,
        why: &ClientError,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.give_back_attached(prompt.attachments.clone());
        self.not_sent(&prompt.text, why, window, cx);
        self.unsent = Some(prompt);
    }

    pub(super) fn not_sent(
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
    pub(super) fn give_back(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.text.update(cx, |composer, cx| {
            if composer.value().is_empty() {
                composer.insert(text.to_owned(), window, cx);
            }
        });
    }
}
