//! What a person says in the drawers above what's typed: a question's and a plan's.

use std::time::Duration;

use gpui_kit::TestAppContext;
use koyo_agent::protocol::ids::RequestId;
use koyo_agent::protocol::requests::{
    PlanDecision, Question, Questions, Request, RequestPayload, RequestStatus, Response,
};

use super::asks::{Asking, holds, request};
use super::question as asked;
use crate::test_support::draw;

fn asking(payload: RequestPayload) -> Request {
    Request {
        payload,
        ..request("q1", RequestStatus::Pending, false, 10)
    }
}

fn questions(questions: Vec<Question>) -> Request {
    asking(RequestPayload::Question(Questions {
        questions,
        live: true,
    }))
}

fn plan() -> Request {
    asking(RequestPayload::Plan {
        markdown: "# Ship it\n\n1. Build".into(),
    })
}

/// The answer of `pairs` to what's asked.
fn answer(pairs: &[(&str, &[&str])]) -> (RequestId, Response) {
    let answers = (pairs.iter())
        .map(|(question, said)| {
            let said = said.iter().map(|each| (*each).to_owned()).collect();
            ((*question).to_owned(), said)
        })
        .collect();
    (RequestId("q1".into()), Response::Question { answers })
}

/// Long enough for a single choice to go on.
fn a_moment_passes(chat: &Asking, cx: &mut TestAppContext) {
    cx.executor().advance_clock(Duration::from_millis(200));
    cx.run_until_parked();
    draw(chat.window, cx);
}

#[gpui_kit::test]
fn a_single_choice_goes_on_by_itself_after_a_moment(cx: &mut TestAppContext) {
    let pending = questions(vec![
        asked("Tabs?", false, &["Tabs", "Spaces"]),
        asked("Which?", false, &["Zed", "Vim"]),
    ]);
    let chat = Asking::new("chat-question-single", vec![pending], cx);

    chat.keys("2", cx);
    assert_eq!(chat.typed(cx), "", "a digit chooses");
    chat.keys("1", cx);
    a_moment_passes(&chat, cx);
    assert_eq!(chat.said(cx), [], "the second question is still to answer");
    chat.keys("2", cx);
    assert_eq!(chat.said(cx), [], "it shows what was chosen first");
    a_moment_passes(&chat, cx);

    assert_eq!(chat.said(cx), [answer(&[("Tabs?", &["Tabs"]), ("Which?", &["Vim"])])]);
}

#[gpui_kit::test]
fn several_are_chosen_and_then_gone_on_from(cx: &mut TestAppContext) {
    let pending = questions(vec![asked("Which?", true, &["Zed", "Vim", "Helix"])]);
    let chat = Asking::new("chat-question-several", vec![pending], cx);

    chat.keys("enter", cx);
    assert_eq!(chat.said(cx), [], "there's nothing to go on from");
    chat.keys("1", cx);
    chat.keys("3", cx);
    chat.keys("2", cx);
    chat.keys("1", cx);
    a_moment_passes(&chat, cx);
    assert_eq!(chat.said(cx), [], "it waits for a person to say they're done");
    chat.keys("enter", cx);

    assert_eq!(chat.said(cx), [answer(&[("Which?", &["Helix", "Vim"])])]);
}

#[gpui_kit::test]
fn what_is_typed_is_the_other_answer(cx: &mut TestAppContext) {
    let pending = questions(vec![
        asked("Tabs?", false, &["Tabs", "Spaces"]),
        asked("Which?", true, &["Zed", "Vim"]),
    ]);
    let chat = Asking::new("chat-question-other", vec![pending], cx);

    chat.keys("b o t h", cx);
    chat.keys("1", cx);
    assert_eq!(chat.typed(cx), "both1", "with something typed a digit is a digit");
    chat.keys("enter", cx);
    assert_eq!(chat.typed(cx), "", "it was taken");
    assert_eq!(chat.said(cx), [], "the second question is still to answer");

    chat.keys("2", cx);
    chat.keys("m i n e", cx);
    chat.keys("enter", cx);

    assert_eq!(
        chat.said(cx),
        [answer(&[("Tabs?", &["both1"]), ("Which?", &["Vim", "mine"])])],
    );
}

#[gpui_kit::test]
fn a_digit_past_the_options_starts_what_is_typed(cx: &mut TestAppContext) {
    let pending = questions(vec![asked("How many?", false, &["One", "Two"])]);
    let chat = Asking::new("chat-question-digit", vec![pending], cx);

    chat.keys("7", cx);
    chat.keys("0", cx);

    assert_eq!(chat.typed(cx), "70");
    assert_eq!(chat.said(cx), []);
}

#[gpui_kit::test]
fn skipping_answers_with_nothing_and_only_while_nothing_is_typed(cx: &mut TestAppContext) {
    let pending = questions(vec![asked("Tabs?", false, &["Tabs", "Spaces"])]);
    let chat = Asking::new("chat-question-skip", vec![pending], cx);

    chat.keys("a", cx);
    chat.keys("cmd-backspace", cx);
    assert_eq!(chat.said(cx), [], "it deleted the line");
    chat.keys("cmd-backspace", cx);

    assert_eq!(chat.said(cx), [answer(&[])]);
}

#[gpui_kit::test]
fn an_answer_that_did_not_go_can_be_given_again(cx: &mut TestAppContext) {
    let pending = questions(vec![asked("Tabs?", false, &["Tabs", "Spaces"])]);
    let chat = Asking::new("chat-question-again", vec![pending], cx);

    // No host hears it here, so it doesn't go, and the drawer shows it again.
    chat.keys("1", cx);
    a_moment_passes(&chat, cx);
    chat.keys("2", cx);
    a_moment_passes(&chat, cx);

    assert_eq!(
        chat.said(cx),
        [answer(&[("Tabs?", &["Tabs"])]), answer(&[("Tabs?", &["Spaces"])])],
    );
}

#[gpui_kit::test]
fn a_settled_question_takes_its_drawer_and_keys_with_it(cx: &mut TestAppContext) {
    let pending = questions(vec![asked("Tabs?", false, &["Tabs", "Spaces"])]);
    let chat = Asking::new("chat-question-settled", vec![pending.clone()], cx);

    let settled = Request {
        status: RequestStatus::Expired,
        ..pending
    };
    holds(&chat.thread, vec![settled], 12, cx);
    draw(chat.window, cx);
    chat.keys("1", cx);

    assert_eq!(chat.typed(cx), "1");
    assert_eq!(chat.said(cx), []);
}

#[gpui_kit::test]
fn a_question_the_agent_stopped_waiting_on_is_answered_by_the_next_message(cx: &mut TestAppContext) {
    let pending = asking(RequestPayload::Question(Questions {
        questions: vec![asked("Tabs?", false, &["Tabs", "Spaces"])],
        live: false,
    }));
    let chat = Asking::new("chat-question-message", vec![pending], cx);

    chat.keys("1", cx);
    chat.keys("cmd-backspace", cx);

    assert_eq!(chat.typed(cx), "", "the keys edit as they always do");
    assert_eq!(chat.said(cx), []);
}

#[gpui_kit::test]
fn a_plan_is_implemented_by_the_key_while_nothing_is_typed(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-plan-key", vec![plan()], cx);

    chat.keys("cmd-enter", cx);

    let implement = Response::Plan {
        decision: PlanDecision::Implement,
    };
    assert_eq!(chat.said(cx), [(RequestId("q1".into()), implement)]);
}

#[gpui_kit::test]
fn what_is_typed_at_a_plan_goes_as_a_message(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-plan-refine", vec![plan()], cx);

    chat.keys("s m a l l e r", cx);
    chat.keys("enter", cx);

    assert_eq!(chat.said(cx), [], "the next message is what answers it");
}

#[gpui_kit::test]
fn a_plan_shows_in_its_place_and_copies_whole(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-plan-row", vec![plan()], cx);
    let timeline = chat.view.read_with(cx, |view, _| view.shown.as_ref().unwrap().timeline.clone());
    let item = koyo_agent::protocol::ids::ItemId("asks-q1".into());

    let text = chat.thread.read_with(cx, |thread, _| thread.drawn_text(&item).is_some());
    assert!(text, "drawing the row made the view of its text");

    timeline.update(cx, |timeline, cx| timeline.copy(item, cx));
    let copied = cx.read_from_clipboard().and_then(|copied| copied.text());
    assert_eq!(copied.as_deref(), Some("# Ship it\n\n1. Build"));
}
