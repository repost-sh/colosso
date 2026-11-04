//! The turn a chat is on, as the composer stops it, and what waits behind it (`docs/chat.md` 4.5.3,
//! 4.5.4, 5.3): the button that sends or stops, the keys, and the drawer of a queue that's held.
//! Its own copy of it, since a token updates the thread and the composer is drawn for what it read.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::{
    App, ClickEvent, Context, Div, InteractiveElement as _, KeyBinding,
    ModifiersChangedEvent, ParentElement as _, Stateful, StatefulInteractiveElement as _,
    Styled as _, Window, actions, div, px,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{RunId, ThreadId};
use koyo_agent::protocol::methods::{
    EditParams, Method, QueueEdit, QueueRemove, QueueResume, QueueSendNow, RunParams, StopParams,
    ThreadParams, TurnsStop,
};
use koyo_agent::protocol::model::{Attachment, RunStatus};
use koyo_agent::replica::Replica;

use super::ChatComposer;
use super::asked::{attached_above, drawer_line};
use super::footer::COMPOSER;
use crate::chat::queued::{GiveBackEdit, RemoveQueued, SaveQueued, SendQueuedNow};
use crate::modal;
use crate::shell::{glass, icon, solid};
use crate::theme::Palette;

/// The composer's context while nothing's typed and a turn can be stopped, where Esc stops it.
pub(super) const STOPPABLE: &str = "ChatStoppable";
/// The composer's context while nothing's typed, where ↑ brings back what was said last.
pub(super) const RECALLING: &str = "ChatRecalling";
/// How long a turn is given to stop before ending the agent's process is offered.
const FORCE_AFTER: Duration = Duration::from_secs(10);

actions!(chat, [StopTurn, RecallPrompt]);

pub(super) fn bind_keys(cx: &mut App) {
    let stoppable = format!("{STOPPABLE} > Input");
    let recalling = format!("{RECALLING} > Input");
    cx.bind_keys([
        KeyBinding::new("escape", StopTurn, Some(stoppable.as_str())),
        // Keyboard shortcuts names the last one, which works wherever a person's typing.
        KeyBinding::new("cmd-.", StopTurn, Some(COMPOSER)),
        KeyBinding::new("up", RecallPrompt, Some(recalling.as_str())),
    ]);
}

/// Where a chat's turn is, and how many messages wait behind it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Turn {
    /// How far the run a session's on is, if there is one.
    status: Option<RunStatus>,
    queued: usize,
    /// Whether what's queued waits for a person, not for the turn.
    held: bool,
}

impl Turn {
    pub(super) fn of(replica: &Replica) -> Turn {
        Turn {
            status: replica.live().map(|run| run.status),
            queued: replica.queued().count(),
            held: replica.thread.queue_held,
        }
    }

    pub(super) fn is_on(self) -> bool {
        self.status.is_some()
    }

    fn is_stopping(self) -> bool {
        self.status == Some(RunStatus::Stopping)
    }
}

impl ChatComposer {
    /// How many messages wait behind the turn.
    pub(in crate::chat) fn queued(&self) -> usize {
        self.turn.queued
    }

    /// Takes in where the chat's turn is now. A stop gets a while to happen, then can be forced.
    pub(super) fn follow_turn(&mut self, turn: Turn, cx: &mut Context<Self>) {
        if turn == self.turn {
            return;
        }
        if turn.is_stopping() != self.turn.is_stopping() {
            self.can_force = false;
            self.forcing = turn.is_stopping().then(|| {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(FORCE_AFTER).await;
                    this.update(cx, |this, cx| {
                        this.can_force = true;
                        cx.notify();
                    })
                    .ok();
                })
            });
        }
        self.turn = turn;
        cx.notify();
    }

    /// Asks the host for `M`, in order after what was asked before.
    fn asking<M>(
        &mut self,
        params: M::Params,
        cx: &App,
    ) -> impl Future<Output = Result<M::Result, ClientError>> + 'static + use<M>
    where
        M: Method + 'static,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
    {
        #[cfg(test)]
        self.asked_host.push((M::NAME, serde_json::to_value(&params).unwrap()));
        self.chats.read(cx).ordered::<M>(params)
    }

    /// Asks the host for `M`, which only says anything if it can't be done.
    pub(super) fn ask<M>(&mut self, params: M::Params, cx: &mut Context<Self>)
    where
        M: Method + 'static,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
    {
        let asked = self.asking::<M>(params, cx);
        self.when_said(asked, cx);
    }

    /// The chat's thread, once it has one.
    pub(super) fn thread_id(&self, cx: &App) -> Option<ThreadId> {
        Some(self.thread.as_ref()?.read(cx).id().clone())
    }

    /// Whether a stop was asked for, and it's too soon to force it.
    fn waits_to_stop(&self) -> bool {
        self.turn.is_stopping() && !self.can_force
    }

    /// Asks for the turn to stop, or once that's taken too long, for it to be ended.
    fn stop(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.thread_id(cx) else {
            return;
        };
        if !self.turn.is_on() || self.waits_to_stop() {
            return;
        }
        let params = StopParams {
            thread_id,
            force: self.can_force,
        };
        self.ask::<TurnsStop>(params, cx);
    }

    /// What was said last comes back to be said again, or changed first.
    fn recall(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let replica = self.thread.as_ref().and_then(|thread| thread.read(cx).replica());
        let Some(said) = replica.and_then(Replica::last_said).map(str::to_owned) else {
            return;
        };
        self.text.update(cx, |text, cx| text.insert(said, window, cx));
    }

    /// What names `run` of the chat, if there's a thread for it to be of.
    fn of_queue(&self, run: &RunId, cx: &App) -> Option<RunParams> {
        Some(RunParams {
            thread_id: self.thread_id(cx)?,
            run_id: run.clone(),
        })
    }

    fn send_queued_now(&mut self, run: &RunId, cx: &mut Context<Self>) {
        if let Some(params) = self.of_queue(run, cx) {
            self.ask::<QueueSendNow>(params, cx);
        }
    }

    fn remove_queued(&mut self, run: &RunId, cx: &mut Context<Self>) {
        if let Some(params) = self.of_queue(run, cx) {
            self.ask::<QueueRemove>(params, cx);
        }
    }

    /// A message waiting now says `text`, with what it `attachments` as it did. If it can't be
    /// changed any more, having started, what it was to say comes here to be sent.
    fn save_queued(
        &mut self,
        run: &RunId,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(thread_id) = self.thread_id(cx) else {
            return;
        };
        let params = EditParams {
            thread_id,
            run_id: run.clone(),
            text: text.clone(),
            attachments,
        };
        let asked = self.asking::<QueueEdit>(params, cx);
        cx.spawn_in(window, async move |this, cx| {
            if asked.await.is_err() {
                this.update_in(cx, |this, window, cx| this.give_back_edit(&text, window, cx)).ok();
            }
        })
        .detach();
    }

    /// What a queued message was being changed to, when it couldn't be: after what's typed, so
    /// neither is lost, with the caret at its end to go on from.
    fn give_back_edit(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.after_typed(text, window, cx);
        self.error = Some("That message couldn't be changed, so your words are here.".into());
        cx.notify();
    }

    /// Lets the queue held for a person go.
    pub(in crate::chat) fn send_held_queue(&mut self, cx: &mut Context<Self>) {
        if let Some(thread_id) = self.thread_id(cx) {
            self.ask::<QueueResume>(ThreadParams { thread_id }, cx);
        }
    }

    /// Takes everything off the queue.
    pub(in crate::chat) fn clear_queue(&mut self, cx: &mut Context<Self>) {
        let replica = self.thread.as_ref().and_then(|thread| thread.read(cx).replica());
        let runs: Vec<RunId> =
            replica.map_or_else(Vec::new, |held| held.queued().map(|run| run.id.clone()).collect());
        for run in &runs {
            self.remove_queued(run, cx);
        }
    }

    /// `column`, which hears the keys of the turn and what's said from the queue's bubbles.
    pub(super) fn hears_turn(column: Div, cx: &mut Context<Self>) -> Div {
        column
            .on_action(cx.listener(|this, _: &StopTurn, _, cx| this.stop(cx)))
            .on_action(cx.listener(|this, _: &RecallPrompt, window, cx| this.recall(window, cx)))
            .on_action(cx.listener(|this, queued: &SendQueuedNow, _, cx| {
                this.send_queued_now(&queued.0, cx);
            }))
            .on_action(cx.listener(|this, queued: &RemoveQueued, _, cx| {
                this.remove_queued(&queued.0, cx);
            }))
            .on_action(cx.listener(|this, queued: &SaveQueued, window, cx| {
                let (text, attachments) = (queued.text.clone(), queued.attachments.clone());
                this.save_queued(&queued.run, text, attachments, window, cx);
            }))
            .on_action(cx.listener(|this, edit: &GiveBackEdit, window, cx| {
                this.give_back_edit(&edit.0, window, cx);
            }))
            // What the send button shows follows ⌘, while there's a turn to queue behind.
            .on_modifiers_changed(cx.listener(|this, event: &ModifiersChangedEvent, _, cx| {
                let secondary = event.modifiers.secondary(cx);
                if this.secondary_held != secondary {
                    this.secondary_held = secondary;
                    if this.turn.is_on() {
                        cx.notify();
                    }
                }
            }))
    }

    /// The button at the end of the footer: it sends what there is to send, and with nothing to
    /// send, stops the turn if there's one on.
    pub(super) fn send_or_stop(&self, typed: &str, p: Palette, cx: &Context<Self>) -> Stateful<Div> {
        let button = div()
            .id("chat-send")
            .relative()
            .size(px(28.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded_full();
        let off = p.text.opacity(0.08);
        if self.can_send(typed) {
            let queues = self.turn.is_on() && self.secondary_held;
            let mark = if queues { IconName::ListEnd } else { IconName::ArrowUp };
            return button
                .bg(p.text)
                .text_color(p.on_text)
                .cursor_pointer()
                .child(icon(mark, 15.))
                .on_click(cx.listener(|this, click: &ClickEvent, window, cx| {
                    if click.modifiers().secondary(cx) {
                        this.queue(window, cx);
                    } else {
                        this.send(window, cx);
                    }
                }));
        }
        if !self.turn.is_on() {
            return button.bg(off).text_color(p.muted).child(icon(IconName::ArrowUp, 15.));
        }
        if self.waits_to_stop() {
            return button.bg(off).child(modal::spinner(p.muted));
        }
        let says = if self.can_force { "Force stop" } else { "Stop" };
        button
            .bg(off)
            .cursor_pointer()
            .hover(move |button| button.bg(p.text.opacity(0.16)))
            .child(div().size(px(10.)).rounded(px(2.)).bg(p.text))
            .tooltip(move |window, cx| {
                Tooltip::new(says).action(&StopTurn, Some(COMPOSER)).build(window, cx)
            })
            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx)))
    }

    /// Messages that wait for a person to let them go, as a stop or a failure leaves them.
    pub(super) fn held_queue(&self, p: Palette, cx: &Context<Self>) -> Option<Div> {
        let count = self.turn.queued;
        if !self.turn.held || count == 0 {
            return None;
        }
        let says = if count == 1 {
            "1 queued message is waiting".to_owned()
        } else {
            format!("{count} queued messages are waiting")
        };
        let clear = glass("chat-queue-clear", p.text, cx).small().label("Clear").on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.clear_queue(cx)),
        );
        let send = solid("chat-queue-send", true, cx).small().label("Send").on_click(
            cx.listener(|this, _: &ClickEvent, _, cx| this.send_held_queue(cx)),
        );
        Some(attached_above(drawer_line(IconName::ListEnd, says, clear, send), p))
    }
}
