//! Going back to a message of a chat (`docs/chat.md` C11): asked first, as it can't be undone. The
//! host takes the message and what came after out of the chat and Claude's conversation, and its
//! words come back to the composer.

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{RunId, ThreadId};
use koyo_agent::protocol::methods::{RewindParams, Rewound, ThreadsRewind};

use super::Chats;
use super::composer::ChatComposer;
use crate::modal::{self, Button, SMALL_LINE, Tone, small_line};
use crate::theme;

const WIDTH: f32 = 400.;
const GAP: f32 = 8.;
const TITLE: &str = "Go back to this message?";
const WHAT_GOES: &str = "It and what came after leave the chat and what Claude knows.";
const WHAT_STAYS: &str = "Its words come back to the composer. Files stay as they are.";

impl Chats {
    /// Takes thread `id` back to before `run`. The answer has what the run was asked. On its own
    /// thread, as the host reads the conversation for it and every chat's messages shouldn't wait
    /// for that; nothing's on its way to the chat by now, as the button only shows when it's still.
    fn rewind(&self, id: ThreadId, run: RunId, cx: &App) -> Task<Result<Rewound, ClientError>> {
        let Some(client) = self.client.clone() else {
            return Task::ready(Err(ClientError::Closed));
        };
        let params = RewindParams { thread_id: id, run_id: run };
        cx.background_spawn(async move { client.request::<ThreadsRewind>(&params) })
    }
}

/// Asks over `window` before going back to the message of `run` of thread `thread`.
pub(super) fn confirm(
    chats: Entity<Chats>,
    composer: Entity<ChatComposer>,
    thread: ThreadId,
    run: RunId,
    window: &mut Window,
    cx: &mut App,
) {
    let parent = window.window_handle();
    let title = modal::heading_height(&TITLE.into(), WIDTH, window);
    let height = 2. * modal::PADDING + title + GAP + 2. * SMALL_LINE + 2. * GAP + modal::FOOTER;
    modal::open(WIDTH, height, window, cx, |_, _| GoingBack {
        chats,
        composer,
        parent,
        thread,
        run,
        going: false,
        error: None,
    });
}

struct GoingBack {
    chats: Entity<Chats>,
    composer: Entity<ChatComposer>,
    /// The window of the chat, where the composer is.
    parent: AnyWindowHandle,
    thread: ThreadId,
    run: RunId,
    going: bool,
    error: Option<SharedString>,
}

impl GoingBack {
    fn go(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.going {
            return;
        }
        self.going = true;
        self.error = None;
        cx.notify();
        let gone = self.chats.read(cx).rewind(self.thread.clone(), self.run.clone(), cx);
        cx.spawn_in(window, async move |this, cx| {
            let gone = gone.await;
            this.update_in(cx, |this, window, cx| match gone {
                Ok(Rewound { prompt }) => {
                    let composer = this.composer.clone();
                    let taken = this.parent.update(cx, |_, window, cx| {
                        composer.update(cx, |composer, cx| composer.take_back(prompt, window, cx));
                    });
                    if let Err(error) = taken {
                        log::warn!("the chat went before its words could come back: {error}");
                    }
                    modal::dismiss(window, cx);
                }
                Err(error) => {
                    this.going = false;
                    modal::fail(&mut this.error, "go back", error.to_string());
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

impl Render for GoingBack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = theme::palette(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(GAP))
            .child(modal::heading(TITLE))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .text_color(p.muted)
                    .child(small_line().child(WHAT_GOES))
                    .child(small_line().child(WHAT_STAYS)),
            )
            .child(
                modal::footer(self.error.clone(), p)
                    .child(modal::cancel("go-back-cancel"))
                    .child(
                        Button::new("go-back-confirm", "Go Back", Tone::Danger)
                            .busy(self.going)
                            .on_click(cx.listener(|this, _, window, cx| this.go(window, cx))),
                    ),
            )
    }
}
