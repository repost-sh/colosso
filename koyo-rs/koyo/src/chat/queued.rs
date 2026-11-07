//! A message waiting its turn, as the timeline shows it (`docs/chat.md` 5.3): a dashed bubble that
//! can go now, come off the queue, or be changed. It's changed where it shows, leaving what's being
//! written in the composer alone. What goes to the host is the composer's to say, as it hears why
//! it can't be done and shows that.

use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Action, AnyWindowHandle, App, AppContext as _, ClickEvent, Context, Div, Entity,
    Focusable as _, InteractiveElement as _, KeyBinding, ParentElement as _, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Window, actions, div, px,
    relative,
};
use koyo_agent::protocol::ids::RunId;
use koyo_agent::protocol::model::{Attachment, Run};
use koyo_agent::timeline::RowId;

use super::asks::says;
use super::rows::{Drawing, attached, bubble, word_button};
use super::timeline::Timeline;

/// The context of a queued message being changed, where Esc leaves it as it was.
const EDITING: &str = "ChatQueuedEdit";

actions!(chat, [CancelQueuedEdit]);

/// Asks for this queued message to go at once.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct SendQueuedNow(pub(super) RunId);

/// Asks for this queued message to come off the queue.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct RemoveQueued(pub(super) RunId);

/// This queued message now says this.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct SaveQueued {
    pub(super) run: RunId,
    pub(super) text: String,
    /// What it attached, which stays as it is.
    pub(super) attachments: Vec<Attachment>,
}

/// A message went while it was being changed, to this.
#[derive(Clone, Debug, PartialEq, Action)]
#[action(namespace = chat, no_json)]
pub(super) struct GiveBackEdit(pub(super) String);

pub(super) fn bind_keys(cx: &mut App) {
    let editing = format!("{EDITING} > Input");
    cx.bind_keys([KeyBinding::new(
        "escape",
        CancelQueuedEdit,
        Some(editing.as_str()),
    )]);
}

/// A queued message being changed, where it shows.
pub(super) struct QueuedEdit {
    pub(super) run: RunId,
    pub(super) text: Entity<TextareaState>,
    /// What it says until this is saved.
    said: String,
    /// Where it's said from, as what's edited may have to go back from outside of a draw.
    window: AnyWindowHandle,
    _hearing: Subscription,
}

impl Timeline {
    /// Run `run` is changed where it shows, starting from what it says.
    pub(super) fn edit_queued(&mut self, run: RunId, window: &mut Window, cx: &mut Context<Self>) {
        let replica = self.thread.read(cx).replica();
        let said = replica.and_then(|replica| replica.queued_run(&run));
        let Some(said) = said.map(|queued| queued.prompt.text.clone()) else {
            return;
        };
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 12)
                .submit_on_enter(true)
        });
        text.update(cx, |text, cx| text.insert(said.clone(), window, cx));
        let hearing = cx.subscribe_in(&text, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { shift: false, .. } => this.save_queued_edit(window, cx),
            InputEvent::Change => this.remeasure_queued_edit(cx),
            _ => {}
        });
        window.focus(&text.read(cx).focus_handle(cx), cx);
        self.queued_edit = Some(QueuedEdit {
            run,
            text,
            said,
            window: window.window_handle(),
            _hearing: hearing,
        });
        self.remeasure_queued_edit(cx);
    }

    /// The row of what's being changed takes the height of it, now.
    fn remeasure_queued_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.queued_edit else {
            return;
        };
        let id = RowId::Queued(edit.run.clone());
        if let Some(index) = self.rows.iter().position(|row| row.id == id) {
            self.scroller.update(cx, |scroller, cx| {
                scroller.remeasure_items(index..index + 1, cx)
            });
        }
        cx.notify();
    }

    /// Stops changing the queued message, and the keys go back to what's typed in the composer.
    pub(super) fn end_queued_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.remeasure_queued_edit(cx);
        self.queued_edit = None;
        window.focus(&self.composer, cx);
    }

    /// What's been written goes to the host, unless it's nothing to be said: without words a
    /// message only goes if it attached something, and without a change there's nothing to do.
    fn save_queued_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(edit) = &self.queued_edit else {
            return;
        };
        let text = edit.text.read(cx).value().trim().to_owned();
        let run = edit.run.clone();
        let replica = self.thread.read(cx).replica();
        let held = replica.and_then(|replica| replica.queued_run(&run));
        let Some(attachments) = held.map(|queued| queued.prompt.attachments.clone()) else {
            return;
        };
        if text.is_empty() && attachments.is_empty() {
            return;
        }
        let changed = text != edit.said;
        self.end_queued_edit(window, cx);
        if changed {
            let saved = SaveQueued {
                run,
                text,
                attachments,
            };
            self.composer.dispatch_action(&saved, window, cx);
        }
    }

    /// A message that isn't waiting any more isn't there to be changed. What it was being changed
    /// to goes to the composer, if it's something it didn't say.
    pub(super) fn follow_queued_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &self.queued_edit else {
            return;
        };
        let replica = self.thread.read(cx).replica();
        if replica.is_some_and(|replica| replica.queued_run(&edit.run).is_some()) {
            return;
        }
        let text = edit.text.read(cx).value().trim().to_owned();
        let changed = text != edit.said && !text.is_empty();
        let (window, composer) = (edit.window, self.composer.clone());
        self.queued_edit = None;
        // It's told in the window, which a thread changing isn't in.
        cx.defer(move |cx| {
            window
                .update(cx, |_, window, cx| {
                    window.focus(&composer, cx);
                    if changed {
                        composer.dispatch_action(&GiveBackEdit(text), window, cx);
                    }
                })
                .ok();
        });
    }
}

impl Drawing {
    /// A message waiting its turn: dashed, as it hasn't been said yet, and what can be done with it.
    pub(super) fn queued(&self, run: &Run) -> Div {
        let p = self.p;
        let prompt = &run.prompt;
        let said = bubble()
            .border_1()
            .border_dashed()
            .border_color(p.wash_line)
            .when(!prompt.attachments.is_empty(), |said| {
                said.child(attached(&run.id.0, &prompt.attachments, p))
            });
        let row = div().flex().flex_col().items_end().gap_1();
        let editing = self
            .queued_edit
            .as_ref()
            .filter(|(editing, _)| *editing == run.id);
        if let Some((_, text)) = editing {
            let said = said.w(relative(0.8)).text_color(p.text);
            return row
                .key_context(EDITING)
                .on_action(self.on_timeline::<CancelQueuedEdit>(Timeline::end_queued_edit))
                .child(said.child(Textarea::new(text).appearance(false)))
                .child(self.queue_actions().children([
                    self.queue_action(run, "Cancel", self.on_timeline(Timeline::end_queued_edit)),
                    self.queue_action(run, "Save", self.on_timeline(Timeline::save_queued_edit)),
                ]));
        }
        let said = said
            .text_color(p.muted)
            .when(!prompt.text.is_empty(), |said| {
                said.child(prompt.text.clone())
            });
        let id = run.id.clone();
        let edits = self.on_timeline(move |this, window, cx| {
            this.edit_queued(id.clone(), window, cx);
        });
        row.child(said).child(self.queue_actions().children([
            self.queue_action(
                run,
                "Send now",
                says(self, Box::new(SendQueuedNow(run.id.clone()))),
            ),
            self.queue_action(run, "Edit", edits),
            self.queue_action(
                run,
                "Remove",
                says(self, Box::new(RemoveQueued(run.id.clone()))),
            ),
        ]))
    }

    /// The row under a queued message.
    fn queue_actions(&self) -> Div {
        div().flex().items_center().gap_3().h(px(20.))
    }

    /// A word under a queued message that does something.
    fn queue_action(
        &self,
        run: &Run,
        says: &'static str,
        does: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let id = SharedString::from(format!("queued-{says}-{}", run.id));
        word_button(id, says, self.p).on_click(does)
    }
}
