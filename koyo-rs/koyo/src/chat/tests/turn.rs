//! Stopping a turn and the messages that wait behind it, by the keys, the drawer and the bubbles. No
//! host hears it here, so what the composer asked of it is what's looked at.

use std::time::Duration;

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use koyo_agent::protocol::events::Changed;
use koyo_agent::protocol::ids::{RunId, ThreadId};
use koyo_agent::protocol::methods::Delivery;
use koyo_agent::protocol::model::{Run, RunStatus};
use serde_json::{Value, json};

use super::super::queued::{RemoveQueued, SendQueuedNow};
use super::super::thread::ThreadEvent;
use super::super::composer::ChatComposer;
use super::super::timeline::Timeline;
use super::asks::Asking;
use super::{run, snapshot};
use crate::test_support::draw;

/// The host now says these runs, and whether the queue waits for a person.
fn runs_are(chat: &Asking, runs: Vec<Run>, held: bool, seq: u64, cx: &mut TestAppContext) {
    let mut thread = snapshot("0001", seq, Vec::new()).thread;
    thread.queue_held = held;
    let event = ThreadEvent::Changed(Box::new(Changed {
        thread_id: ThreadId("0001".into()),
        seq,
        thread: Some(thread),
        runs,
        items: Vec::new(),
        requests: Vec::new(),
    }));
    chat.thread.update(cx, |thread, cx| thread.take_in(vec![event], cx));
    draw(chat.window, cx);
}

fn chat_on(name: &str, status: RunStatus, cx: &mut TestAppContext) -> Asking {
    let chat = Asking::new(name, Vec::new(), cx);
    runs_are(&chat, vec![run("r2", status, "Go on.", 20)], false, 20, cx);
    chat
}

fn asked(chat: &Asking, cx: &TestAppContext) -> Vec<(&'static str, Value)> {
    chat.composer.read_with(cx, |composer, _| composer.asked_host.clone())
}

fn stop(force: bool) -> (&'static str, Value) {
    ("turns.stop", json!({ "threadId": "0001", "force": force }))
}

fn queued_is_asked(method: &'static str, run: &str) -> (&'static str, Value) {
    (method, json!({ "threadId": "0001", "runId": run }))
}

#[gpui_kit::test]
fn with_a_turn_on_enter_goes_into_it_and_cmd_enter_after_it(cx: &mut TestAppContext) {
    let chat = chat_on("chat-turn-delivery", RunStatus::Running, cx);

    chat.keys("n o enter", cx);
    assert_eq!(chat.typed(cx), "no", "with no host to hear it, it comes back");
    chat.keys("cmd-enter", cx);

    let sent = chat.composer.read_with(cx, |composer, _| composer.sent_as.clone());
    assert_eq!(sent, [Delivery::Auto, Delivery::Queue]);
}

#[gpui_kit::test]
fn the_keys_stop_a_turn_and_esc_only_with_nothing_typed(cx: &mut TestAppContext) {
    let chat = chat_on("chat-turn-stop", RunStatus::Running, cx);

    chat.keys("escape", cx);
    assert_eq!(asked(&chat, cx), [stop(false)]);

    chat.keys("n o", cx);
    chat.keys("escape", cx);
    assert_eq!(asked(&chat, cx), [stop(false)], "it's left to what's being typed");

    chat.keys("cmd-.", cx);
    assert_eq!(asked(&chat, cx), [stop(false), stop(false)], "this one stops whatever's typed");
}

#[gpui_kit::test]
fn there_is_nothing_to_stop_without_a_turn(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-turn-idle", Vec::new(), cx);

    chat.keys("escape", cx);
    chat.keys("cmd-.", cx);

    assert_eq!(asked(&chat, cx), []);
}

#[gpui_kit::test]
fn a_stop_that_takes_too_long_can_be_forced(cx: &mut TestAppContext) {
    let chat = chat_on("chat-turn-force", RunStatus::Stopping, cx);

    chat.keys("cmd-.", cx);
    assert_eq!(asked(&chat, cx), [], "it's on its way");

    cx.executor().advance_clock(Duration::from_secs(10));
    draw(chat.window, cx);
    chat.keys("cmd-.", cx);

    assert_eq!(asked(&chat, cx), [stop(true)]);
}

#[gpui_kit::test]
fn up_on_an_empty_composer_brings_back_what_was_said_last(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-turn-recall", Vec::new(), cx);

    chat.keys("up", cx);

    assert_eq!(chat.typed(cx), "Why?");
}

#[gpui_kit::test]
fn a_held_queue_goes_or_is_cleared_when_a_person_says(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-turn-held", Vec::new(), cx);
    let waiting = vec![
        run("q1", RunStatus::Queued, "First.", 20),
        run("q2", RunStatus::Queued, "Second.", 20),
    ];
    runs_are(&chat, waiting, true, 20, cx);
    assert_eq!(chat.composer.read_with(cx, |composer, _| composer.queued()), 2);

    chat.composer.update(cx, ChatComposer::send_held_queue);
    assert_eq!(asked(&chat, cx), [("queue.resume", json!({ "threadId": "0001" }))]);

    chat.composer.update(cx, ChatComposer::clear_queue);
    assert_eq!(
        asked(&chat, cx)[1..],
        [queued_is_asked("queue.remove", "q1"), queued_is_asked("queue.remove", "q2")],
    );
}

#[gpui_kit::test]
fn a_bubble_says_to_go_now_or_come_off_the_queue(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-turn-bubble", Vec::new(), cx);
    runs_are(&chat, vec![run("q1", RunStatus::Queued, "First.", 20)], false, 20, cx);

    chat.says(&SendQueuedNow(RunId("q1".into())), cx);
    chat.says(&RemoveQueued(RunId("q1".into())), cx);

    assert_eq!(
        asked(&chat, cx),
        [queued_is_asked("queue.sendNow", "q1"), queued_is_asked("queue.remove", "q1")],
    );
}

fn timeline(chat: &Asking, cx: &TestAppContext) -> Entity<Timeline> {
    chat.view.read_with(cx, |view, _| view.showing().unwrap().timeline.clone())
}

/// A chat with something being written and a message waiting, which is being changed.
fn editing(name: &str, cx: &mut TestAppContext) -> Asking {
    let chat = Asking::new(name, Vec::new(), cx);
    runs_are(&chat, vec![run("q1", RunStatus::Queued, "Later", 20)], false, 20, cx);
    chat.keys("o k", cx);
    let timeline = timeline(&chat, cx);
    cx.update_window(chat.window, |_, window, cx| {
        timeline.update(cx, |timeline, cx| {
            timeline.edit_queued(RunId("q1".into()), window, cx);
        });
    })
    .unwrap();
    draw(chat.window, cx);
    chat
}

#[gpui_kit::test]
fn a_queued_message_is_changed_where_it_shows(cx: &mut TestAppContext) {
    let chat = editing("chat-turn-edit", cx);

    chat.keys("!", cx);
    assert_eq!(chat.typed(cx), "ok", "what's being written is left alone");
    chat.keys("enter", cx);

    let edited = json!({ "threadId": "0001", "runId": "q1", "text": "Later!", "attachments": [] });
    assert_eq!(asked(&chat, cx), [("queue.edit", edited)]);
    // With no host to hear it, it couldn't be changed, so the words come here to go on from.
    chat.keys("!", cx);
    assert_eq!(chat.typed(cx), "ok\n\nLater!!", "and the keys are back in the composer");
}

#[gpui_kit::test]
fn esc_leaves_a_queued_message_as_it_was(cx: &mut TestAppContext) {
    let chat = editing("chat-turn-edit-esc", cx);

    chat.keys("! escape", cx);

    assert_eq!(asked(&chat, cx), []);
    timeline(&chat, cx).read_with(cx, |timeline, _| assert!(timeline.queued_edit.is_none()));
    chat.keys("!", cx);
    assert_eq!(chat.typed(cx), "ok!");
}

#[gpui_kit::test]
fn a_message_that_goes_while_it_is_changed_leaves_the_words_in_the_composer(
    cx: &mut TestAppContext,
) {
    let chat = editing("chat-turn-edit-went", cx);
    chat.keys("!", cx);

    runs_are(&chat, vec![run("q1", RunStatus::Running, "Later", 30)], false, 30, cx);

    assert_eq!(chat.typed(cx), "ok\n\nLater!");
    assert_eq!(asked(&chat, cx), []);
    let why = chat.composer.read_with(cx, |composer, _| composer.error.clone());
    assert!(why.is_some());
}
