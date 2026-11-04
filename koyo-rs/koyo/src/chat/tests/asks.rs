//! What a person says to what the agent asks, by the keys, the menu and what's typed. No host
//! hears it here, so the composer's own record of what it said is what's looked at.

use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Focusable as _, Modifiers, TestAppContext,
    VisualTestContext,
};
use koyo_agent::protocol::events::Changed;
use koyo_agent::protocol::ids::{ItemId, RequestId, RunId, ThreadId, Time};
use koyo_agent::protocol::model::{Item, ItemBody, ItemStatus, RequestRef, ToolFamily};
use koyo_agent::protocol::requests::{
    AllowSession, Approval, Decision, Request, RequestPayload, RequestStatus, Response,
};

use super::super::asks::{AllowRequest, AllowRequestForChat, StopRequest, answers_past_allow};
use super::super::composer::ChatComposer;
use super::super::thread::{ChatThread, ThreadEvent};
use super::super::view::ChatView;
use super::drawing::shown_chat;
use super::{AT, answer};
use crate::menu;
use crate::test_support::draw;

fn request(id: &str, status: RequestStatus, for_chat: bool, seq: u64) -> Request {
    Request {
        id: RequestId(id.into()),
        thread_id: ThreadId("0001".into()),
        run_id: RunId("r".into()),
        status,
        payload: RequestPayload::Approval(Approval {
            tool: "Bash".into(),
            family: ToolFamily::Command,
            title: "Run a command".into(),
            label: "cargo test".into(),
            detail: "cargo test".into(),
            change: None,
            input: serde_json::Value::Null,
            reason: None,
            allow_session: for_chat.then(|| AllowSession {
                label: "Bash(cargo test:*)".into(),
            }),
        }),
        answer: None,
        created_at: Time(AT.into()),
        resolved_at: None,
        seq,
    }
}

/// The thread now holds `requests`, each with its row at the end of the run.
fn holds(thread: &Entity<ChatThread>, requests: Vec<Request>, seq: u64, cx: &mut TestAppContext) {
    let items: Vec<Item> = (requests.iter().enumerate())
        .map(|(at, request)| Item {
            id: ItemId(format!("asks-{}", request.id)),
            status: ItemStatus::Completed,
            body: ItemBody::Request(RequestRef {
                request_id: request.id.clone(),
            }),
            ..answer("", 10 + at as u64, "", seq)
        })
        .collect();
    let event = ThreadEvent::Changed(Box::new(Changed {
        thread_id: ThreadId("0001".into()),
        seq,
        thread: None,
        runs: Vec::new(),
        items,
        requests,
    }));
    thread.update(cx, |thread, cx| thread.take_in(vec![event], cx));
}

struct Asking {
    window: AnyWindowHandle,
    composer: Entity<ChatComposer>,
    thread: Entity<ChatThread>,
    _view: Entity<ChatView>,
}

impl Asking {
    /// A chat's tab with the composer focused, and `requests` waiting on the person.
    fn new(name: &str, requests: Vec<Request>, cx: &mut TestAppContext) -> Asking {
        let (window, view, thread) = shown_chat(name, cx);
        holds(&thread, requests, 10, cx);
        let composer = view.read_with(cx, |view, _| view.composer.clone());
        cx.update_window(window, |_, window, cx| {
            window.focus(&composer.read(cx).focus_handle(cx), cx);
        })
        .unwrap();
        draw(window, cx);
        Asking {
            window,
            composer,
            thread,
            _view: view,
        }
    }

    fn said(&self, cx: &TestAppContext) -> Vec<(RequestId, Response)> {
        self.composer.read_with(cx, |composer, _| composer.said.clone())
    }

    fn typed(&self, cx: &TestAppContext) -> String {
        self.composer.read_with(cx, |composer, cx| {
            composer.text.read(cx).value().to_string()
        })
    }

    fn keys(&self, keys: &str, cx: &mut TestAppContext) {
        cx.simulate_keystrokes(self.window, keys);
        draw(self.window, cx);
    }

    fn says(&self, action: &dyn gpui_kit::Action, cx: &mut TestAppContext) {
        cx.update_window(self.window, |_, window, cx| {
            let target = self.composer.read(cx).focus_handle(cx);
            target.dispatch_action(action, window, cx);
        })
        .unwrap();
        draw(self.window, cx);
    }
}

fn approval_of(request: &str, decision: Decision, message: Option<&str>) -> (RequestId, Response) {
    let response = Response::Approval {
        decision,
        message: message.map(str::to_owned),
    };
    (RequestId(request.into()), response)
}

#[gpui_kit::test]
fn the_keys_answer_what_is_asked_while_nothing_is_typed(cx: &mut TestAppContext) {
    let pending = vec![request("q1", RequestStatus::Pending, true, 10)];
    let chat = Asking::new("chat-keys", pending, cx);

    chat.keys("cmd-enter", cx);
    chat.keys("cmd-alt-enter", cx);
    chat.keys("cmd-backspace", cx);

    assert_eq!(
        chat.said(cx),
        [
            approval_of("q1", Decision::Allow, None),
            approval_of("q1", Decision::AllowSession, None),
            approval_of("q1", Decision::Deny, None),
        ],
    );
}

#[gpui_kit::test]
fn the_keys_edit_as_they_always_do_once_something_is_typed(cx: &mut TestAppContext) {
    let pending = vec![request("q1", RequestStatus::Pending, true, 10)];
    let chat = Asking::new("chat-keys-typed", pending, cx);

    chat.keys("a b c", cx);
    assert_eq!(chat.typed(cx), "abc");
    chat.keys("cmd-backspace", cx);

    assert_eq!(chat.typed(cx), "", "it deleted the line");
    assert_eq!(chat.said(cx), [], "and answered nothing");
}

#[gpui_kit::test]
fn the_keys_answer_nothing_when_nothing_is_asked(cx: &mut TestAppContext) {
    let chat = Asking::new("chat-keys-idle", Vec::new(), cx);

    chat.keys("cmd-enter", cx);
    chat.keys("cmd-backspace", cx);

    assert_eq!(chat.said(cx), []);
}

#[gpui_kit::test]
fn what_is_typed_declines_with_its_words(cx: &mut TestAppContext) {
    let pending = vec![request("q1", RequestStatus::Pending, false, 10)];
    let chat = Asking::new("chat-decline", pending, cx);

    chat.keys("n o space w a y", cx);
    chat.keys("enter", cx);

    assert_eq!(chat.said(cx), [approval_of("q1", Decision::Deny, Some("no way"))]);
}

#[gpui_kit::test]
fn the_oldest_request_is_answered_first_and_then_the_next(cx: &mut TestAppContext) {
    let pending = vec![
        request("q2", RequestStatus::Pending, false, 11),
        request("q1", RequestStatus::Pending, false, 10),
    ];
    let chat = Asking::new("chat-oldest", pending, cx);

    chat.keys("cmd-enter", cx);
    assert_eq!(chat.said(cx), [approval_of("q1", Decision::Allow, None)]);

    let settled = vec![request("q1", RequestStatus::Answered, false, 12)];
    holds(&chat.thread, settled, 12, cx);
    draw(chat.window, cx);
    chat.keys("cmd-enter", cx);

    assert_eq!(chat.said(cx).last(), Some(&approval_of("q2", Decision::Allow, None)));
}

#[gpui_kit::test]
fn the_menu_answers_whatever_is_typed_and_for_the_chat_only_if_it_can_be(cx: &mut TestAppContext) {
    let pending = vec![request("q1", RequestStatus::Pending, false, 10)];
    let chat = Asking::new("chat-menu", pending, cx);
    chat.keys("a", cx);

    chat.says(&AllowRequestForChat, cx);
    assert_eq!(chat.said(cx), [], "there's nothing to add for this chat");

    chat.says(&StopRequest, cx);
    chat.says(&AllowRequest, cx);
    assert_eq!(
        chat.said(cx),
        [
            approval_of("q1", Decision::Cancel, None),
            approval_of("q1", Decision::Allow, None),
        ],
    );
    assert_eq!(chat.typed(cx), "a", "what's typed stays");
}

#[gpui_kit::test]
fn a_row_of_the_menu_says_its_answer_to_the_composer(cx: &mut TestAppContext) {
    let pending = vec![request("q1", RequestStatus::Pending, true, 10)];
    let chat = Asking::new("chat-menu-row", pending, cx);
    cx.update_window(chat.window, |_, window, cx| {
        let target = chat.composer.read(cx).focus_handle(cx);
        let items = answers_past_allow(Some("Allow Bash(cargo test:*) for this chat".into()));
        menu::open("test-chat-menu".into(), Bounds::default(), items, &target, window, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let popup = (cx.windows().into_iter())
        .find(|handle| *handle != chat.window)
        .expect("the menu opened a window of its own");

    VisualTestContext::from_window(popup, cx).simulate_click(menu::row_center(0), Modifiers::default());
    cx.run_until_parked();

    assert_eq!(chat.said(cx), [approval_of("q1", Decision::AllowSession, None)]);
}
