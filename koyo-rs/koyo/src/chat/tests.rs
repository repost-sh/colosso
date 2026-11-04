//! The chats as the host's answers and events bring them, with no host: what comes back as tabs,
//! when, and what an entity is told.

use std::path::Path;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use koyo_agent::agent_client::Notice;
use koyo_agent::client::ClientError;
use koyo_agent::protocol::events::{Appended, Changed, Event, Removed};
use koyo_agent::protocol::ids::{ItemId, RunId, ThreadId, Time};
use koyo_agent::protocol::methods::ThreadSnapshot;
use koyo_agent::protocol::model::{
    Item, ItemBody, ItemStatus, Status, Text, Thread, ThreadSummary, TitleSource,
};
use koyo_agent::protocol::requests::{Question, QuestionOption};

use super::Chats;
use super::thread::{ChatThread, ThreadEvent};
use crate::model::PaneKind;
use crate::shell::Shell;
use crate::test_support::{draw, repo, select, shell_with};

pub(super) mod asks;
mod drawers;
mod drawing;

const AT: &str = "2026-11-03T10:00:00.000Z";

/// A question that's its own id, with options of those labels.
pub(in crate::chat) fn question(text: &str, multi_select: bool, labels: &[&str]) -> Question {
    Question {
        id: text.into(),
        header: "About".into(),
        text: text.into(),
        multi_select,
        options: (labels.iter())
            .map(|label| QuestionOption {
                label: (*label).into(),
                description: None,
                preview: None,
            })
            .collect(),
    }
}

fn summary(id: &str, cwd: &Path, title: &str, tab_open: bool) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId(id.into()),
        cwd: cwd.to_string_lossy().into_owned(),
        title: title.into(),
        provider: "claude".into(),
        model: None,
        status: Status::Idle,
        needs: None,
        queued: 0,
        unread: false,
        tab_open,
        updated_at: Time(AT.into()),
    }
}

pub(super) fn snapshot(id: &str, seq: u64, items: Vec<Item>) -> ThreadSnapshot {
    ThreadSnapshot {
        thread: Thread {
            id: ThreadId(id.into()),
            cwd: "/work".into(),
            title: "Why".into(),
            title_source: TitleSource::Seed,
            provider: "claude".into(),
            model: None,
            effort: None,
            fast: false,
            permission: None,
            plan: false,
            session_id: None,
            queue_held: false,
            tab_open: true,
            created_at: Time(AT.into()),
            updated_at: Time(AT.into()),
            read_at: None,
            seq,
        },
        runs: Vec::new(),
        items,
        requests: Vec::new(),
    }
}

fn answer(id: &str, ordinal: u64, text: &str, seq: u64) -> Item {
    Item {
        id: ItemId(id.into()),
        thread_id: ThreadId("t".into()),
        run_id: RunId("r".into()),
        ordinal,
        parent_id: None,
        status: ItemStatus::Running,
        seq,
        body: ItemBody::AssistantText(Text { text: text.into() }),
    }
}

fn appended(item: &str, text: &str) -> ThreadEvent {
    ThreadEvent::Appended(Appended {
        thread_id: ThreadId("t".into()),
        item_id: ItemId(item.into()),
        text: text.into(),
    })
}

fn changed(seq: u64, items: Vec<Item>) -> ThreadEvent {
    ThreadEvent::Changed(Box::new(Changed {
        thread_id: ThreadId("t".into()),
        seq,
        thread: None,
        runs: Vec::new(),
        items,
        requests: Vec::new(),
    }))
}

fn text_of(thread: &Entity<ChatThread>, cx: &TestAppContext) -> String {
    thread.read_with(cx, |thread, _| {
        let replica = thread.replica().unwrap();
        let items = replica.items.iter().filter_map(|item| item.body.text());
        items.collect::<Vec<_>>().join("|")
    })
}

/// How many times `thread` notifies while `act` runs.
fn notifications(
    thread: &Entity<ChatThread>,
    cx: &mut TestAppContext,
    act: impl FnOnce(&mut ChatThread, &mut gpui_kit::Context<ChatThread>),
) -> usize {
    let seen = std::rc::Rc::new(std::cell::Cell::new(0));
    let counting = seen.clone();
    let _observing = cx.update(|cx| {
        cx.observe(thread, move |_, _| counting.set(counting.get() + 1))
    });
    thread.update(cx, act);
    cx.run_until_parked();
    seen.get()
}

/// The titles of the tabs on the shown checkout, in order.
fn tabs(shell: &Entity<Shell>, cx: &TestAppContext) -> Vec<String> {
    shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        unit.tabs.iter().map(|tab| tab.first().title.to_string()).collect()
    })
}

fn chats_of(shell: &Entity<Shell>, cx: &TestAppContext) -> Entity<Chats> {
    shell.read_with(cx, |shell, _| shell.chat_tabs.chats.clone())
}

#[gpui_kit::test]
fn what_comes_before_the_snapshot_is_applied_on_top_of_it(cx: &mut TestAppContext) {
    let thread = cx.new(|_| ChatThread::new(ThreadId("t".into())));
    thread.update(cx, |thread, cx| {
        assert_eq!(thread.opening(), None);
        thread.take_in(vec![appended("a", " world")], cx);
        assert!(thread.replica().is_none());
        let opened = snapshot("t", 3, vec![answer("a", 0, "Hello", 3)]);
        thread.opened(Ok(opened), cx);
    });
    assert_eq!(text_of(&thread, cx), "Hello world");
}

#[gpui_kit::test]
fn what_comes_before_a_reopens_snapshot_is_applied_on_top_of_it_too(cx: &mut TestAppContext) {
    let thread = cx.new(|_| ChatThread::new(ThreadId("t".into())));
    thread.update(cx, |thread, cx| {
        thread.opened(Ok(snapshot("t", 3, vec![answer("a", 0, "Hel", 3)])), cx);
        assert_eq!(thread.opening(), Some(3));
        thread.take_in(vec![appended("a", " world")], cx);
        // The snapshot has the item whole as it was, which the appends go on from.
        thread.opened(Ok(snapshot("t", 4, vec![answer("a", 0, "Hello", 3)])), cx);
    });
    assert_eq!(text_of(&thread, cx), "Hello world");
}

#[gpui_kit::test]
fn what_came_for_an_open_that_failed_is_let_go(cx: &mut TestAppContext) {
    let thread = cx.new(|_| ChatThread::new(ThreadId("t".into())));
    thread.update(cx, |thread, cx| {
        thread.opening();
        thread.take_in(vec![appended("a", " world")], cx);
        thread.opened(Err(ClientError::Closed), cx);
        // Nothing to go on: the next open's snapshot brings it.
        thread.take_in(vec![appended("a", " again")], cx);
        thread.opening();
        thread.opened(Ok(snapshot("t", 3, vec![answer("a", 0, "Hello", 3)])), cx);
    });
    assert_eq!(text_of(&thread, cx), "Hello");
}

#[gpui_kit::test]
fn a_token_notifies_only_its_text_and_a_change_the_thread_once(cx: &mut TestAppContext) {
    let thread = cx.new(|_| ChatThread::new(ThreadId("t".into())));
    thread.update(cx, |thread, cx| {
        thread.opened(Ok(snapshot("t", 1, vec![answer("a", 0, "", 1)])), cx);
    });
    cx.run_until_parked();
    let appends = notifications(&thread, cx, |thread, cx| {
        thread.take_in(vec![appended("a", "Hel"), appended("a", "lo")], cx);
    });
    assert_eq!(appends, 0);
    assert_eq!(text_of(&thread, cx), "Hello");
    let steps = notifications(&thread, cx, |thread, cx| {
        let first = changed(2, vec![answer("a", 0, "Hello.", 2)]);
        let second = changed(3, vec![answer("b", 1, "More", 3)]);
        thread.take_in(vec![first, second], cx);
    });
    assert_eq!(steps, 1);
    assert_eq!(text_of(&thread, cx), "Hello.|More");
}

#[gpui_kit::test]
fn a_text_already_drawn_grows_and_is_settled_with_the_rows(cx: &mut TestAppContext) {
    let thread = cx.new(|_| ChatThread::new(ThreadId("t".into())));
    let id = ItemId("a".into());
    let view = thread.update(cx, |thread, cx| {
        thread.opened(Ok(snapshot("t", 1, vec![answer("a", 0, "Hel", 1)])), cx);
        thread.text(&id, "Hel", cx)
    });
    let shows = |cx: &mut TestAppContext| {
        // The view parses in the background, and shows what it has parsed.
        cx.run_until_parked();
        view.read_with(cx, |view, _| view.rendered_text().as_str().trim_end().to_owned())
    };
    thread.update(cx, |thread, cx| {
        thread.take_in(vec![appended("a", "lo"), appended("a", "!")], cx);
    });
    assert_eq!(shows(cx), "Hello!");
    thread.update(cx, |thread, cx| {
        thread.take_in(vec![changed(2, vec![answer("a", 0, "Goodbye", 2)])], cx);
    });
    assert_eq!(shows(cx), "Goodbye");
}

#[gpui_kit::test]
fn the_first_list_brings_back_the_tabs_it_names_in_the_order_they_were_made(
    cx: &mut TestAppContext,
) {
    let root = repo("chat-restore", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        let listed = vec![
            summary("0002", &root, "Second", true),
            summary("0003", &root, "Closed", false),
            summary("0001", &root, "First", true),
            summary("0004", Path::new("/not/listed"), "Elsewhere", true),
        ];
        chats.listed(Ok(listed), cx);
    });
    draw(window, cx);
    assert_eq!(tabs(&shell, cx), ["First", "Second"]);
    // What's on a checkout not listed yet waits for it; the others aren't brought back twice.
    let waiting = chats.read_with(cx, |chats, _| chats.to_restore().to_vec());
    assert_eq!(waiting, [ThreadId("0004".into())]);
    shell.update(cx, Shell::follow_chats);
    assert_eq!(tabs(&shell, cx), ["First", "Second"]);
}

#[gpui_kit::test]
fn a_chat_made_while_the_first_list_was_on_its_way_is_not_brought_back_by_it(
    cx: &mut TestAppContext,
) {
    let root = repo("chat-early", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| {
        shell.new_tab(PaneKind::Chat, cx);
        let pane = shell.workspace.branch().unwrap().tabs[0].first().id;
        shell.workspace.set_chat(pane, ThreadId("0001".into()));
    });
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "Made first", true)]), cx);
    });
    draw(window, cx);
    assert_eq!(tabs(&shell, cx), ["Made first"]);
    let waiting = chats.read_with(cx, |chats, _| chats.to_restore().to_vec());
    assert!(waiting.is_empty());
}

#[gpui_kit::test]
fn a_chat_made_here_is_not_brought_back_by_its_own_summary_or_a_later_list(
    cx: &mut TestAppContext,
) {
    let root = repo("chat-no-double", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| chats.listed(Ok(Vec::new()), cx));
    let made = summary("0001", &root, "Made here", true);
    chats.update(cx, |chats, cx| {
        let event = Event::ThreadSummary(made.clone());
        chats.take_in(vec![Notice::Event(Box::new(event))], cx);
    });
    draw(window, cx);
    assert!(tabs(&shell, cx).is_empty());
    chats.update(cx, |chats, cx| chats.listed(Ok(vec![made.clone()]), cx));
    draw(window, cx);
    assert!(tabs(&shell, cx).is_empty());
}

#[gpui_kit::test]
fn a_tab_is_titled_as_its_chat_and_goes_when_the_chat_does(cx: &mut TestAppContext) {
    let root = repo("chat-title", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "Before", true)]), cx);
    });
    draw(window, cx);
    assert_eq!(tabs(&shell, cx), ["Before"]);
    chats.update(cx, |chats, cx| {
        let event = Event::ThreadSummary(summary("0001", &root, "After", true));
        chats.take_in(vec![Notice::Event(Box::new(event))], cx);
    });
    draw(window, cx);
    assert_eq!(tabs(&shell, cx), ["After"]);
    chats.update(cx, |chats, cx| {
        let removed = Event::ThreadRemoved(Removed {
            thread_id: ThreadId("0001".into()),
        });
        chats.take_in(vec![Notice::Event(Box::new(removed))], cx);
    });
    draw(window, cx);
    assert!(tabs(&shell, cx).is_empty());
}

#[gpui_kit::test]
fn a_chat_pane_gets_its_view_only_once_it_is_shown_and_loses_it_with_the_pane(
    cx: &mut TestAppContext,
) {
    let root = repo("chat-view", &[("README.md", "hi\n")]);
    let (window, shell) = shell_with(&[&root], cx);
    select(&shell, &root, cx);
    shell.update(cx, |shell, cx| shell.new_tab(PaneKind::Chat, cx));
    draw(window, cx);
    let chats = chats_of(&shell, cx);
    chats.update(cx, |chats, cx| {
        chats.listed(Ok(vec![summary("0001", &root, "Restored", true)]), cx);
    });
    draw(window, cx);
    let (new, restored) = shell.read_with(cx, |shell, _| {
        let unit = shell.workspace.branch().unwrap();
        (unit.tabs[0].first().id, unit.tabs[1].first().id)
    });
    let has_view = |pane, cx: &TestAppContext| {
        shell.read_with(cx, |shell, _| shell.chat_tabs.view(pane).is_some())
    };
    assert!(has_view(new, cx), "the new chat is the tab shown");
    assert!(!has_view(restored, cx), "a tab not looked at costs nothing");
    shell.update(cx, |shell, cx| {
        shell.workspace.close_chat_panes(&ThreadId("0001".into()));
        let unit = shell.workspace.branch_mut().unwrap();
        unit.close_pane(new);
        cx.notify();
    });
    draw(window, cx);
    assert!(!has_view(new, cx));
}
