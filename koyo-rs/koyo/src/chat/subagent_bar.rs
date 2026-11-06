//! What takes the composer's place in a subagent's chat (`docs/chat.md` 5.3): it runs on its own,
//! so nothing's said to it from here. This says how it's doing, with what, and for how long;
//! it can be stopped, and left for the chat that started it.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, ClickEvent, Context, Div, ElementId, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, actions, div, px,
};
use koyo_agent::protocol::methods::Catalog;
use koyo_agent::protocol::model::{ItemStatus, Run, RunStatus};
use koyo_agent::replica::Replica;
use koyo_agent::words::{effort_label, model_called, subagent_line, subagent_status};

use super::Chats;
use super::rows::word_button;
use super::subagents::avatar;
use super::thread::ChatThread;
use super::timeline::keep_ticking;
use crate::native::liquid;
use crate::shell::ellipsized;
use crate::theme::{self, Palette};

actions!(chat, [OpenParent]);

/// Where a subagent's chat has the keys.
pub(super) const KEYS: &str = "ChatSubagent";

pub(super) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", OpenParent, Some(KEYS))]);
}

/// A person went back to the chat that started the subagent.
pub(super) struct WentToParent;

pub(super) struct SubagentBar {
    thread: Entity<ChatThread>,
    catalog: Entity<Catalog>,
    chats: Entity<Chats>,
    focus: FocusHandle,
    /// Keeps the elapsed time going while it runs.
    ticking: Option<Task<()>>,
    _following: Subscription,
}

impl EventEmitter<WentToParent> for SubagentBar {}

impl SubagentBar {
    pub(super) fn new(
        thread: Entity<ChatThread>,
        catalog: Entity<Catalog>,
        chats: Entity<Chats>,
        cx: &mut Context<Self>,
    ) -> Self {
        let _following = cx.observe(&thread, |bar, _, cx| {
            bar.keep_time(cx);
            cx.notify();
        });
        let mut bar = SubagentBar {
            thread,
            catalog,
            chats,
            focus: cx.focus_handle(),
            ticking: None,
            _following,
        };
        bar.keep_time(cx);
        bar
    }

    fn keep_time(&mut self, cx: &mut Context<Self>) {
        let running = self
            .thread
            .read(cx)
            .replica()
            .and_then(Replica::live)
            .is_some();
        keep_ticking(&mut self.ticking, running, cx);
    }

    fn stop(&mut self, cx: &mut Context<Self>) {
        let id = self.thread.read(cx).id().clone();
        self.chats.update(cx, |chats, cx| chats.stop(&id, cx));
    }

    /// What it's called, how it's doing, and what it runs as the catalog calls it, how hard.
    fn words(&self, p: Palette, cx: &App) -> Div {
        let replica = self.thread.read(cx).replica();
        let thread = replica.map(|replica| &replica.thread);
        let title = thread.map_or_else(String::new, |thread| thread.title.clone());
        let status = replica.and_then(|replica| replica.runs.last()).map(status);
        let model = (thread.and_then(|thread| thread.model.as_deref()))
            .map(|id| model_called(id, &self.catalog.read(cx).models));
        let effort = thread
            .and_then(|thread| thread.effort)
            .map(|effort| effort_label(effort).to_owned());
        let said = [Some("Runs on its own".to_owned()), model, effort];
        let said: Vec<String> = said.into_iter().flatten().collect();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .text_sm()
                    .child(ellipsized(title).text_color(p.text))
                    .children(status.map(|status| {
                        div()
                            .flex_shrink_0()
                            .text_xs()
                            .text_color(p.muted)
                            .child(status)
                    })),
            )
            .child(ellipsized(said.join(" · ")).text_xs().text_color(p.muted))
    }
}

/// How it's doing and for how long.
fn status(run: &Run) -> String {
    let doing = match run.status {
        RunStatus::Stopping => "Stopping",
        _ => subagent_status(how_it_is(run)),
    };
    let (started, ended) = (run.started_at.as_ref(), run.ended_at.as_ref());
    subagent_line(doing, started, ended, chrono::Utc::now())
}

/// How a subagent's doing, by its run.
fn how_it_is(run: &Run) -> ItemStatus {
    match run.status {
        RunStatus::Completed => ItemStatus::Completed,
        RunStatus::Failed => ItemStatus::Failed,
        RunStatus::Interrupted => ItemStatus::Interrupted,
        _ => ItemStatus::Running,
    }
}

impl Focusable for SubagentBar {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SubagentBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        let keyed = ElementId::NamedInteger("chat-subagent-glass".into(), cx.entity_id().as_u64());
        let on_glass = liquid::backdrop(keyed, px(18.), window, cx);
        let run = self
            .thread
            .read(cx)
            .replica()
            .and_then(|replica| replica.runs.last());
        let stoppable =
            run.is_some_and(|run| run.status.is_live() && run.status != RunStatus::Stopping);
        let surface = div()
            .relative()
            .w_full()
            .flex()
            .items_center()
            .gap_4()
            .px_4()
            .py_3()
            .rounded(px(18.))
            .occlude();
        let surface = liquid::floating(surface, on_glass, p)
            .child(avatar(run.map_or(ItemStatus::Running, how_it_is), p))
            .child(self.words(p, cx))
            .when(stoppable, |bar| {
                bar.child(
                    word_button("chat-subagent-stop", "Stop", p)
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.stop(cx))),
                )
            })
            .child(
                word_button("chat-open-parent", "Open parent", p)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(WentToParent))),
            );
        div()
            .track_focus(&self.focus)
            .relative()
            .w_full()
            .map(|bar| crate::settings::get(cx).chat_width.column_of(bar))
            .px_6()
            .child(surface)
    }
}
