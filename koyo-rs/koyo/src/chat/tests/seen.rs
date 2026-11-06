//! What a chat has new is new until a person has seen it: on show, with the window in front.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use koyo_agent::agent_client::Notice;
use koyo_agent::protocol::events::Event;
use koyo_agent::protocol::ids::ThreadId;
use koyo_agent::protocol::model::{Status, ThreadSummary};

use super::super::{Chats, ChatsEvent};
use super::{chats_of, summary};
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

struct Chat {
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    chats: Entity<Chats>,
    root: PathBuf,
}

/// A window showing chat `0001`, in front if it's said to be.
fn shown(name: &str, in_front: bool, cx: &mut TestAppContext) -> Chat {
    let root = repo(name, &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "A chat", true)]), cx);
    });
    let chat = Chat {
        window,
        shell,
        chats,
        root,
    };
    if in_front {
        chat.front(cx);
    } else {
        draw(window, cx);
    }
    chat
}

impl Chat {
    fn front(&self, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, _| window.activate_window())
            .unwrap();
        draw(self.window, cx);
    }

    fn is_unread(&self, cx: &TestAppContext) -> bool {
        let id = ThreadId("0001".into());
        self.chats
            .read_with(cx, |chats, _| chats.summary(&id).unwrap().unread)
    }
}

fn news(root: &Path) -> ThreadSummary {
    ThreadSummary {
        unread: true,
        ..summary("0001", root, "A chat", true)
    }
}

#[gpui_kit::test]
fn what_a_chat_has_new_waits_for_its_window_to_come_to_the_front(cx: &mut TestAppContext) {
    let chat = shown("chat-seen-window", false, cx);

    chat.hears(news(&chat.root), cx);
    assert!(chat.is_unread(cx), "nobody's looking");

    chat.front(cx);
    assert!(!chat.is_unread(cx));
}

#[gpui_kit::test]
fn what_a_chat_has_new_is_seen_at_once_with_its_tab_on_show_in_front(cx: &mut TestAppContext) {
    let chat = shown("chat-seen-now", true, cx);

    chat.hears(news(&chat.root), cx);

    assert!(!chat.is_unread(cx));
}

#[gpui_kit::test]
fn what_a_chat_has_new_waits_for_its_tab(cx: &mut TestAppContext) {
    let chat = shown("chat-seen-tab", true, cx);
    chat.shell.update(cx, |shell, cx| {
        shell
            .workspace
            .add_chat_tab(&chat.root, ThreadId("0002".into()), false);
        shell.workspace.select_tab(0, 0, 1);
        cx.notify();
    });
    draw(chat.window, cx);

    chat.hears(news(&chat.root), cx);
    assert!(chat.is_unread(cx), "its tab isn't on show");

    chat.shell.update(cx, |shell, cx| {
        shell.workspace.select_tab(0, 0, 0);
        cx.notify();
    });
    draw(chat.window, cx);
    assert!(!chat.is_unread(cx));
}

fn working(root: &Path) -> ThreadSummary {
    ThreadSummary {
        status: Status::Working,
        ..summary("0001", root, "A chat", true)
    }
}

impl Chat {
    fn hears(&self, summary: ThreadSummary, cx: &mut TestAppContext) {
        self.chats.update(cx, |chats, cx| {
            chats.take_in(
                vec![Notice::Event(Box::new(Event::ThreadSummary(summary)))],
                cx,
            );
        });
        draw(self.window, cx);
    }

    /// How many times a person's told something, from now on.
    fn tells(&self, cx: &mut TestAppContext) -> Rc<Cell<usize>> {
        let told = Rc::new(Cell::new(0));
        let counts = told.clone();
        cx.update(|cx| {
            cx.subscribe(&self.chats, move |_, event, _| {
                if matches!(event, ChatsEvent::News { .. }) {
                    counts.set(counts.get() + 1);
                }
            })
            .detach();
        });
        told
    }
}

#[gpui_kit::test]
fn a_person_who_is_not_looking_hears_a_chat_finished(cx: &mut TestAppContext) {
    let chat = shown("chat-told", false, cx);
    let told = chat.tells(cx);
    chat.hears(working(&chat.root), cx);
    assert_eq!(told.get(), 0);

    chat.hears(summary("0001", &chat.root, "A chat", true), cx);

    assert_eq!(told.get(), 1);
}

#[gpui_kit::test]
fn a_person_at_the_app_is_told_nothing(cx: &mut TestAppContext) {
    let chat = shown("chat-not-told", true, cx);
    let told = chat.tells(cx);
    chat.hears(working(&chat.root), cx);

    chat.hears(summary("0001", &chat.root, "A chat", true), cx);

    assert_eq!(told.get(), 0);
}
