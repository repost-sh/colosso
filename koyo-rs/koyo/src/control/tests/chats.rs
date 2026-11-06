//! Chats as their operations see them: listed by worktree, shown in a tab, and refused when
//! there's no such chat or nothing to say. What goes through to a host is QA's, on a real one.

use std::path::Path;

use gpui_kit::TestAppContext;
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::model::{Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::{Value, json};

use super::{Rig, allow, call, controlled, external, read_shell, terminal};
use crate::test_support::{repo, select};

fn summary(id: &str, cwd: &Path, at: &str) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId(id.into()),
        cwd: cwd.to_string_lossy().into_owned(),
        title: format!("Chat {id}"),
        provider: "claude".into(),
        model: None,
        status: Status::Working,
        needs: Some(RequestKind::Approval),
        queued: 1,
        unread: true,
        tab_open: false,
        updated_at: Time(at.into()),
    }
}

fn hears(rig: &Rig, summaries: Vec<ThreadSummary>, cx: &mut TestAppContext) {
    let chats = read_shell(rig, cx, |shell, _| shell.chats().clone());
    chats.update(cx, |chats, cx| chats.hear_summaries(summaries, cx));
    cx.run_until_parked();
}

fn ids(listed: &Value) -> Vec<&str> {
    let chats = listed["chats"].as_array().unwrap();
    chats
        .iter()
        .map(|chat| chat["id"].as_str().unwrap())
        .collect()
}

#[gpui_kit::test]
fn a_worktrees_chats_are_listed_latest_first_and_every_worktrees_with_all(cx: &mut TestAppContext) {
    let root = repo("control-chats", &[("README.md", "hi\n")]);
    let rig = controlled("control-chats", &[&root], cx);
    select(&rig.shell, &root, cx);
    let said = vec![
        summary("older", &root, "2026-11-04T10:00:00Z"),
        summary("newer", &root, "2026-11-04T11:00:00Z"),
        summary("elsewhere", Path::new("/not/here"), "2026-11-04T12:00:00Z"),
    ];
    hears(&rig, said, cx);
    let agent = terminal("term_1");

    let listed = call(&agent, "chat_list", json!({}), cx).unwrap();
    assert_eq!(ids(&listed), ["newer", "older"]);
    let chat = &listed["chats"][0];
    assert_eq!(chat["title"], json!("Chat newer"));
    assert_eq!(chat["worktree"], json!(root));
    assert_eq!(chat["status"], json!("working"));
    assert_eq!(chat["needs"], json!("approval"));
    assert_eq!(chat["queued"], json!(1));
    assert_eq!(chat["unread"], json!(true));
    assert_eq!(chat["open"], json!(false));

    let every = call(&agent, "chat_list", json!({ "all": true }), cx).unwrap();
    assert_eq!(ids(&every), ["elsewhere", "newer", "older"]);
}

#[gpui_kit::test]
fn a_chat_opens_in_a_tab_and_opening_it_again_finds_that_tab(cx: &mut TestAppContext) {
    let root = repo("control-chat-open", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-open", &[&root], cx);
    select(&rig.shell, &root, cx);
    hears(
        &rig,
        vec![summary("one", &root, "2026-11-04T10:00:00Z")],
        cx,
    );
    let agent = terminal("term_1");

    let opened = call(&agent, "chat_open", json!({ "chat": "one" }), cx).unwrap();
    assert_eq!(opened["worktree"], json!(root));
    let again = call(&agent, "chat_open", json!({ "chat": "one" }), cx).unwrap();
    assert_eq!(again["pane"], opened["pane"], "the tab that shows it");

    let tabs = call(&agent, "tab_list", json!({}), cx).unwrap();
    let panes = tabs["worktrees"][0]["tabs"].as_array().unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(panes[0]["panes"][0]["kind"], json!("chat"));
}

#[gpui_kit::test]
fn what_names_no_chat_or_says_nothing_is_refused(cx: &mut TestAppContext) {
    let root = repo("control-chat-refused", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-refused", &[&root], cx);
    select(&rig.shell, &root, cx);
    hears(
        &rig,
        vec![summary("one", &root, "2026-11-04T10:00:00Z")],
        cx,
    );
    let agent = terminal("term_1");

    for (operation, input) in [
        ("chat_open", json!({ "chat": "none" })),
        ("chat_read", json!({ "chat": "none" })),
        ("chat_stop", json!({ "chat": "none" })),
        ("chat_send", json!({ "chat": "none", "text": "hi" })),
    ] {
        let refused = call(&agent, operation, input, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::NotFound, "{operation}");
    }
    let blank = json!({ "chat": "one", "text": "  " });
    let refused = call(&agent, "chat_send", blank, cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
    let refused = call(&agent, "chat_create", json!({ "text": "" }), cx).unwrap_err();
    assert_eq!(refused.code, ErrorCode::InvalidInput);
}

#[gpui_kit::test]
fn telling_a_chat_takes_more_than_reading_one(cx: &mut TestAppContext) {
    let root = repo("control-chat-scopes", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-scopes", &[&root], cx);
    select(&rig.shell, &root, cx);
    allow("reader", &[Scope::Read], cx);
    let reader = external("reader");

    call(&reader, "chat_list", json!({}), cx).unwrap();
    for (operation, input) in [
        ("chat_create", json!({ "text": "hi" })),
        ("chat_send", json!({ "chat": "one", "text": "hi" })),
        ("chat_stop", json!({ "chat": "one" })),
    ] {
        let refused = call(&reader, operation, input, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::ScopeDenied, "{operation}");
    }
}
