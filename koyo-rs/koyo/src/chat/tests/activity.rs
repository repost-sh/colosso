//! The composer's working edge (`docs/chat.md` 5.3, 5.4): its steps draw it alone, as long as it
//! moves or fades and no longer.

use gpui_kit::{AppContext as _, Entity, TestAppContext};
use koyo_agent::protocol::model::RunStatus;

use super::super::composer::activity::{Activity, FRAME};
use super::asks::Asking;
use super::run;
use super::turn::runs_are;
use crate::settings::{self, ChatActivity};

/// The host now says the chat's one run is `status`.
fn run_is(chat: &Asking, status: RunStatus, seq: u64, cx: &mut TestAppContext) {
    runs_are(chat, vec![run("r2", status, "Go on.", seq)], false, seq, cx);
}

fn edge(chat: &Asking, cx: &TestAppContext) -> Entity<Activity> {
    chat.composer
        .read_with(cx, |composer, _| composer.activity.clone())
}

/// Whether the edge has a step of its own waiting.
fn stepping(chat: &Asking, cx: &TestAppContext) -> bool {
    edge(chat, cx).read_with(cx, |activity, _| activity.stepping())
}

/// How many times the composer, then its edge, have been drawn.
fn drawn(chat: &Asking, cx: &TestAppContext) -> (usize, usize) {
    (
        chat.composer.read_with(cx, |composer, _| composer.renders),
        edge(chat, cx).read_with(cx, |activity, _| activity.renders),
    )
}

/// Lets `steps` of the edge's steps go by.
fn steps(steps: usize, cx: &mut TestAppContext) {
    for _ in 0..steps {
        cx.executor().advance_clock(FRAME);
        cx.run_until_parked();
    }
}

#[gpui_kit::test]
fn the_working_edge_steps_alone_while_it_moves_and_draws_nothing_once_it_has_faded(
    cx: &mut TestAppContext,
) {
    let chat = Asking::new("chat-working-edge", Vec::new(), cx);
    cx.update_window(chat.window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    let (_, idle) = drawn(&chat, cx);
    steps(10, cx);
    assert!(!stepping(&chat, cx), "nothing steps while nothing works");
    assert_eq!(drawn(&chat, cx).1, idle);

    run_is(&chat, RunStatus::Running, 20, cx);
    // Past its fade in, so what goes on is the line going round.
    steps(30, cx);
    let (typed, moving) = drawn(&chat, cx);
    steps(10, cx);
    let (typed_after, moved) = drawn(&chat, cx);
    // A running turn draws the tab again by itself now and then, as its time ticks on, and with
    // it the composer at most once here; drawn in it, the edge drew the composer each step.
    assert!(
        typed_after - typed <= 1,
        "its steps draw no composer: {typed} to {typed_after}"
    );
    assert!(
        (9..=11).contains(&(moved - moving)),
        "a step each frame's time, no more: {moving} to {moved}"
    );

    run_is(&chat, RunStatus::Completed, 21, cx);
    assert!(stepping(&chat, cx), "it fades out");
    // Past its fade out, it takes no step of its own: it's drawn again only as the tab around it
    // is, as for the run that ended.
    steps(40, cx);
    assert!(!stepping(&chat, cx), "nothing steps once it's gone");
    let (_, faded) = drawn(&chat, cx);
    steps(20, cx);
    let gone = drawn(&chat, cx).1;
    assert!(gone - faded <= 1, "nor is it drawn on: {faded} to {gone}");
    assert!(!stepping(&chat, cx));
}

/// It held still whenever Koyo wasn't the app in front, which read as a chat that had stopped.
#[gpui_kit::test]
fn the_working_edge_goes_round_with_another_app_in_front(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-working-edge-elsewhere", Vec::new(), cx);
    cx.update(|cx| assert!(cx.active_window().is_none(), "Koyo isn't in front"));
    run_is(&chat, RunStatus::Running, 20, cx);
    // Past its fade in, so what goes on is the line going round.
    steps(30, cx);
    let (_, moving) = drawn(&chat, cx);
    steps(10, cx);
    let (_, moved) = drawn(&chat, cx);
    assert!(stepping(&chat, cx), "it goes on round");
    assert!(
        (9..=11).contains(&(moved - moving)),
        "a step each frame's time: {moving} to {moved}"
    );
}

#[gpui_kit::test]
fn the_working_edge_holds_still_with_reduced_motion_or_its_setting_and_shows_nothing_off(
    cx: &mut TestAppContext,
) {
    let chat = Asking::new("chat-working-edge-still", Vec::new(), cx);
    cx.update_window(chat.window, |_, window, _| window.activate_window())
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| cx.set_reduce_motion(true));
    run_is(&chat, RunStatus::Running, 20, cx);
    // Past its fade in, it holds still: nothing more is drawn while the chat works.
    steps(30, cx);
    let (_, still) = drawn(&chat, cx);
    steps(10, cx);
    assert_eq!(drawn(&chat, cx).1, still, "reduced motion");

    cx.update(|cx| cx.set_reduce_motion(false));
    cx.update(|cx| settings::update(cx, |s| s.chat_activity = ChatActivity::Still));
    cx.run_until_parked();
    let (_, still) = drawn(&chat, cx);
    steps(10, cx);
    assert_eq!(drawn(&chat, cx).1, still, "a still edge");

    cx.update(|cx| settings::update(cx, |s| s.chat_activity = ChatActivity::Off));
    cx.run_until_parked();
    let (_, off) = drawn(&chat, cx);
    steps(10, cx);
    assert_eq!(drawn(&chat, cx).1, off, "off");
}
