//! Going back to a message of a chat (`docs/chat.md` C11): asked first, as it can't be undone. The
//! host takes the message and what came after out of the chat and Claude's conversation, and its
//! words come back to the composer. The dialog opens at once and finds out what could become of
//! the files as it stands, and only offers them back when that's safe, on the side where the offer
//! moves nothing a person may be about to press.

use std::path::Path;

use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Task, Window, div, px,
};
use koyo_agent::client::ClientError;
use koyo_agent::protocol::ids::{RunId, ThreadId};
use koyo_agent::protocol::methods::{
    FilesBack, FilesBackAnswer, Method, RewindParams, Rewound, RunParams, ThreadsFilesBack,
    ThreadsRewind,
};

use super::Chats;
use super::composer::ChatComposer;
use crate::files::file_name;
use crate::modal::{self, Button, SMALL_LINE, Tone, small_line};
use crate::theme;

const WIDTH: f32 = 400.;
const GAP: f32 = 8.;
const TITLE: &str = "Go back to this message?";
const WHAT_GOES: &str = "It and what came after leave the chat and what Claude knows.";
const WORDS: &str = "Its words come back to the composer.";
const LOOKING: &str = "Looking at the files it changed…";
const FILES_STAY: &str = "Files stay as they are.";
const WHAT_RESTORING_LEAVES: &str = "What commands and subagents changed stays as it is.";

impl Chats {
    /// Asks for `M` on its own thread, ahead of the queue: the host reads the conversation for
    /// these and every chat's messages shouldn't wait for that. Nothing's on its way to the chat by
    /// now, as the button only shows when it's still.
    fn ahead_of_queue<M>(&self, params: M::Params, cx: &App) -> Task<Result<M::Result, ClientError>>
    where
        M: Method + 'static,
        M::Params: Send + 'static,
        M::Result: Send + 'static,
    {
        let Some(client) = self.client.clone() else {
            return Task::ready(Err(ClientError::Closed));
        };
        cx.background_spawn(async move { client.request::<M>(&params) })
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
    let height = 2. * modal::PADDING + title + GAP + 4. * SMALL_LINE + 2. * GAP + modal::FOOTER;
    modal::open(WIDTH, height, window, cx, |window, cx| {
        let params = RunParams { thread_id: thread.clone(), run_id: run.clone() };
        let asked = chats.read(cx).ahead_of_queue::<ThreadsFilesBack>(params, cx);
        cx.spawn_in(window, async move |this, cx| {
            let answer = asked.await;
            this.update(cx, |this: &mut GoingBack, cx| this.heard(answer, cx)).ok();
        })
        .detach();
        GoingBack {
            chats,
            composer,
            parent,
            thread,
            run,
            files: Files::Looking,
            going: None,
            error: None,
        }
    });
}

/// What the dialog knows of the files.
enum Files {
    Looking,
    Stay,
    Back(FilesBack),
}

impl Files {
    /// What it says of them, in one line, or two when there's more to it.
    fn lines(&self) -> (String, Option<String>) {
        match self {
            Files::Looking => (LOOKING.to_owned(), None),
            Files::Stay => (FILES_STAY.to_owned(), None),
            Files::Back(FilesBack { blocked: Some(why), .. }) => {
                (FILES_STAY.to_owned(), Some(why.clone()))
            }
            Files::Back(back) => (restoring(back), Some(WHAT_RESTORING_LEAVES.to_owned())),
        }
    }

    fn can_restore(&self) -> bool {
        matches!(self, Files::Back(FilesBack { blocked: None, .. }))
    }
}

/// What restoring would do, as a person reads it.
pub(super) fn restoring(back: &FilesBack) -> String {
    let name = |file: &String| file_name(Path::new(file));
    let names = match back.files.as_slice() {
        [] => String::new(),
        [one] => name(one),
        [one, two] => format!("{} and {}", name(one), name(two)),
        [first, rest @ ..] => format!("{} and {} other files", name(first), rest.len()),
    };
    format!("Restoring changes {names} (+{} −{}).", back.insertions, back.deletions)
}

struct GoingBack {
    chats: Entity<Chats>,
    composer: Entity<ChatComposer>,
    /// The window of the chat, where the composer is.
    parent: AnyWindowHandle,
    thread: ThreadId,
    run: RunId,
    files: Files,
    /// Whether it's going back, and if it is whether the files go too.
    going: Option<bool>,
    error: Option<SharedString>,
}

impl GoingBack {
    /// The host's answer on the files. Not knowing only leaves them where they are.
    fn heard(&mut self, answer: Result<FilesBackAnswer, ClientError>, cx: &mut Context<Self>) {
        self.files = match answer {
            Ok(FilesBackAnswer { files_back: Some(back) }) => Files::Back(back),
            Ok(FilesBackAnswer { files_back: None }) => Files::Stay,
            Err(error) => {
                log::warn!("what going back would do to the files: {error}");
                Files::Stay
            }
        };
        cx.notify();
    }

    fn go(&mut self, restore_files: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.going.is_some() {
            return;
        }
        self.going = Some(restore_files);
        self.error = None;
        cx.notify();
        let params =
            RewindParams { thread_id: self.thread.clone(), run_id: self.run.clone(), restore_files };
        let gone = self.chats.read(cx).ahead_of_queue::<ThreadsRewind>(params, cx);
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
                    this.going = None;
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
        let (files, more) = self.files.lines();
        let restore = self.files.can_restore().then(|| {
            Button::new("go-back-restore", "Restore and Go Back", Tone::Plain)
                .busy(self.going == Some(true))
                .on_click(cx.listener(|this, _, window, cx| this.go(true, window, cx)))
        });
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
                    .child(small_line().child(WORDS))
                    .child(small_line().child(files))
                    .children(more.map(|more| small_line().child(more))),
            )
            .child(
                modal::footer(self.error.clone(), p)
                    .children(restore)
                    .child(modal::cancel("go-back-cancel"))
                    .child(
                        Button::new("go-back-confirm", "Go Back", Tone::Danger)
                            .busy(self.going == Some(false))
                            .on_click(cx.listener(|this, _, window, cx| this.go(false, window, cx))),
                    ),
            )
    }
}
