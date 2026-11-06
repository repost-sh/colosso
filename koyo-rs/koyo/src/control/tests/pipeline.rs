//! What every call goes through: the scopes it needs, the person's confirmation of a
//! destructive call, receipts, the activity log, and the topics followers hear of.

use std::sync::{Arc, Mutex};

use gpui_kit::TestAppContext;
use koyo_app_control::{ErrorCode, Scope, Topic};
use serde_json::json;

use super::{
    CONFIRM, activity, allow, call, controlled, external, op, read_shell, send, slot, taken,
    terminal,
};
use crate::control::topics::BATCH;
use crate::control::{ActivityKind, Decision, Request, handle};
use crate::test_support::{repo, select};

#[gpui_kit::test]
fn a_call_needs_every_scope_it_uses(cx: &mut TestAppContext) {
    let root = repo("control-scopes", &[("README.md", "hi\n")]);
    let rig = controlled("control-scopes", &[&root], cx);
    select(&rig.shell, &root, cx);
    allow("Claude Code", &Scope::DEFAULT_GRANT, cx);
    let agent = terminal("term_1");
    let chat = call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap()["pane"].clone();
    let refused = call(
        &external("Claude Code"),
        "settings_set",
        json!({ "previewFiles": false }),
        cx,
    );
    assert_eq!(
        refused.unwrap_err().details,
        Some(json!({ "scope": "settings" }))
    );
    // Discarding unsaved edits needs `destructive` too, which no agent in a terminal has.
    let refused = call(
        &agent,
        "tab_close",
        json!({ "pane": chat, "discard": true }),
        cx,
    );
    assert_eq!(
        refused.unwrap_err().details,
        Some(json!({ "scope": "destructive" }))
    );
    let refused = call(&agent, "worktree_delete", json!({ "worktree": "main" }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::ScopeDenied);
    assert!(!cx.has_pending_prompt(), "a refused call asks nobody");
    // Refused changes are logged as well.
    let logged: Vec<_> = (activity(cx).into_iter())
        .filter_map(|entry| match entry.kind {
            ActivityKind::Call { operation, outcome } => Some((operation, outcome.is_ok())),
            _ => None,
        })
        .collect();
    assert_eq!(
        logged,
        [
            ("worktree_delete", false),
            ("tab_close", false),
            ("settings_set", false),
            ("tab_open", true)
        ]
        .map(|(operation, done)| (operation.to_owned(), done))
    );
}

#[gpui_kit::test]
fn a_destructive_call_goes_ahead_once_the_person_confirms(cx: &mut TestAppContext) {
    let root = repo("control-confirm", &[("README.md", "hi\n")]);
    let rig = controlled("control-confirm", &[&root], cx);
    select(&rig.shell, &root, cx);
    allow("Claude Code", &Scope::ALL, cx);
    let client = external("Claude Code");
    let chat = call(&client, "tab_open", json!({ "kind": "chat" }), cx).unwrap()["pane"].clone();
    let closing = send(
        &client,
        None,
        op("tab_close", json!({ "pane": chat, "discard": true })),
        cx,
    );
    cx.run_until_parked();
    let (question, _) = cx.pending_prompt().expect("the person is asked");
    assert_eq!(
        question,
        "“Claude Code” wants to close the tab “New chat”, discarding unsaved edits"
    );
    assert!(taken(&closing).is_none());
    // One prompt per client: a second destructive call waits its turn.
    let second = call(
        &client,
        "tab_close",
        json!({ "pane": chat, "discard": true }),
        cx,
    )
    .unwrap_err();
    assert_eq!(
        (second.code, second.retryable),
        (ErrorCode::RequestConflict, true)
    );
    cx.simulate_prompt_answer("Close");
    cx.run_until_parked();
    assert_eq!(
        taken(&closing).unwrap().unwrap(),
        json!({ "closed": true, "discarded": [] })
    );
    assert_eq!(
        read_shell(&rig, cx, |shell, _| shell
            .workspace
            .branch()
            .unwrap()
            .tabs
            .len()),
        0
    );
    let confirmed = activity(cx).into_iter().any(|entry| {
        matches!(
            entry.kind,
            ActivityKind::Confirmation {
                decision: Decision::Allowed,
                ..
            }
        )
    });
    assert!(confirmed, "the decision is logged");
}

#[gpui_kit::test]
fn a_destructive_call_the_person_cancels_is_declined_and_keeps_no_receipt(cx: &mut TestAppContext) {
    let root = repo("control-decline", &[("README.md", "hi\n")]);
    let rig = controlled("control-decline", &[&root], cx);
    select(&rig.shell, &root, cx);
    allow("Claude Code", &Scope::ALL, cx);
    let client = external("Claude Code");
    let chat = call(&client, "tab_open", json!({ "kind": "chat" }), cx).unwrap()["pane"].clone();
    let close = op("tab_close", json!({ "pane": chat, "discard": true }));
    let closing = send(&client, Some("close-1"), close.clone(), cx);
    cx.run_until_parked();
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(
        taken(&closing).unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
    assert_eq!(
        read_shell(&rig, cx, |shell, _| shell
            .workspace
            .branch()
            .unwrap()
            .tabs
            .len()),
        1
    );
    // Asked again with the same requestId, the person is asked again; silence declines.
    let closing = send(&client, Some("close-1"), close, cx);
    cx.run_until_parked();
    assert!(cx.has_pending_prompt(), "a declined call kept no receipt");
    cx.executor().advance_clock(CONFIRM);
    cx.run_until_parked();
    assert_eq!(
        taken(&closing).unwrap().unwrap_err().code,
        ErrorCode::UserDeclined
    );
    let decisions: Vec<_> = (activity(cx).into_iter())
        .filter_map(|entry| match entry.kind {
            ActivityKind::Confirmation { decision, .. } => Some(decision),
            _ => None,
        })
        .collect();
    assert_eq!(decisions, [Decision::TimedOut, Decision::Denied]);
}

#[gpui_kit::test]
fn a_repeat_with_the_same_request_id_answers_the_first_result(cx: &mut TestAppContext) {
    let root = repo("control-receipts", &[("README.md", "hi\n")]);
    let rig = controlled("control-receipts", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let open = op("tab_open", json!({ "kind": "chat" }));
    let first = send(&agent, Some("open-1"), open.clone(), cx);
    cx.run_until_parked();
    let first = taken(&first).unwrap().unwrap();
    let repeat = send(&agent, Some("open-1"), open, cx);
    cx.run_until_parked();
    assert_eq!(
        taken(&repeat).unwrap().unwrap(),
        first,
        "the repeat answers the first result"
    );
    assert_eq!(
        read_shell(&rig, cx, |shell, _| shell
            .workspace
            .branch()
            .unwrap()
            .tabs
            .len()),
        1
    );
    let other = send(
        &agent,
        Some("open-1"),
        op("tab_open", json!({ "kind": "editor" })),
        cx,
    );
    cx.run_until_parked();
    assert_eq!(
        taken(&other).unwrap().unwrap_err().code,
        ErrorCode::RequestConflict
    );
    // Reads keep no receipts and aren't logged; the repeat acted no more, so it isn't either.
    let read = send(&agent, Some("open-1"), op("tab_list", json!({})), cx);
    cx.run_until_parked();
    assert!(taken(&read).unwrap().is_ok());
    let operations: Vec<_> = (activity(cx).into_iter())
        .filter_map(|entry| match entry.kind {
            ActivityKind::Call { operation, .. } => Some(operation),
            _ => None,
        })
        .collect();
    assert_eq!(
        operations,
        ["tab_open", "tab_open"],
        "the first call and the conflict"
    );
}

#[gpui_kit::test]
fn followers_hear_of_a_change_once_it_settles(cx: &mut TestAppContext) {
    let root = repo("control-topics", &[("README.md", "hi\n")]);
    let rig = controlled("control-topics", &[&root], cx);
    select(&rig.shell, &root, cx);
    let agent = terminal("term_1");
    let heard = Arc::new(Mutex::new(Vec::new()));
    let (subscribed, reply) = slot();
    let sink = Box::new({
        let heard = Arc::clone(&heard);
        move |topic| {
            heard.lock().unwrap().push(topic);
            true
        }
    });
    let request = Request::Subscribe {
        connection: 7,
        caller: agent.clone(),
        topics: vec![Topic::Workspace, Topic::Settings],
        sink,
        reply,
    };
    cx.update(|cx| handle(request, cx));
    assert!(taken(&subscribed).unwrap().is_ok());
    call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    assert!(heard.lock().unwrap().is_empty(), "changes gather first");
    cx.executor().advance_clock(BATCH);
    cx.run_until_parked();
    assert_eq!(
        *heard.lock().unwrap(),
        [Topic::Workspace],
        "two changes, one event"
    );
    // A setting set to what it is changes nothing to hear of.
    call(&agent, "settings_set", json!({ "previewFiles": true }), cx).unwrap();
    cx.executor().advance_clock(BATCH);
    cx.run_until_parked();
    assert_eq!(heard.lock().unwrap().len(), 1);
    call(&agent, "settings_set", json!({ "previewFiles": false }), cx).unwrap();
    cx.executor().advance_clock(BATCH);
    cx.run_until_parked();
    assert_eq!(*heard.lock().unwrap(), [Topic::Workspace, Topic::Settings]);
    // A connection gone hears nothing more.
    cx.update(|cx| handle(Request::Closed { connection: 7 }, cx));
    call(&agent, "tab_open", json!({ "kind": "chat" }), cx).unwrap();
    cx.executor().advance_clock(BATCH);
    cx.run_until_parked();
    assert_eq!(heard.lock().unwrap().len(), 2);
}
