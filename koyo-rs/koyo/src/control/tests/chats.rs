//! Chats as their operations see them: listed by worktree, shown in a tab, and refused when
//! there's no such chat or nothing to say; and, against a stand-in host, started on what's asked
//! and told as they run. What a real host and agent do with it is QA's.

use std::path::Path;

use gpui_kit::TestAppContext;
use koyo_agent::protocol::ids::{ThreadId, Time};
use koyo_agent::protocol::model::{Mode, Status, ThreadSummary};
use koyo_agent::protocol::requests::RequestKind;
use koyo_app_control::{ErrorCode, Scope};
use serde_json::{Value, json};

use super::host::{MADE, call_through_host, stand_in};
use super::{Rig, activity, allow, call, chat, controlled, external, read_shell, terminal};
use crate::control::ActivityKind;
use crate::test_support::{repo, select};

pub(super) fn summary(id: &str, cwd: &Path, at: &str) -> ThreadSummary {
    ThreadSummary {
        id: ThreadId(id.into()),
        cwd: cwd.to_string_lossy().into_owned(),
        title: format!("Chat {id}"),
        provider: "claude".into(),
        model: None,
        effort: None,
        fast: false,
        permission: None,
        plan: false,
        status: Status::Working,
        needs: Some(RequestKind::Approval),
        background: false,
        queued: 1,
        unread: true,
        tab_open: false,
        updated_at: Time(at.into()),
    }
}

pub(super) fn hears(rig: &Rig, summaries: Vec<ThreadSummary>, cx: &mut TestAppContext) {
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
        ("chat_set", json!({ "chat": "one", "mode": "ask" })),
    ] {
        let refused = call(&reader, operation, input, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::ScopeDenied, "{operation}");
    }
}

/// What a chat is told it runs with, as the stand-in host's catalog has it.
pub(super) fn runs_with(model: &str, name: &str, effort: Option<&str>, mode: &str) -> Value {
    let mut runs =
        json!({ "model": model, "modelName": name, "mode": mode, "fast": false, "plan": false });
    if let Some(effort) = effort {
        runs["effort"] = json!(effort);
    }
    runs
}

#[gpui_kit::test]
fn a_chat_starts_on_the_model_effort_and_mode_asked_for_and_says_so(cx: &mut TestAppContext) {
    let root = repo("control-chat-asked", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-asked", &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let agent = terminal("term_1");

    let asked = json!({
        "text": "Implement it end to end.",
        "model": "Opus 5.5",
        "effort": "max",
        "mode": "accept_edits",
        "fast": true,
        "plan": true,
    });
    let started = call_through_host(&agent, "chat_create", asked, cx).unwrap();

    let made = heard.of("threads.create");
    assert_eq!(made.len(), 1);
    assert_eq!(
        (
            &made[0]["model"],
            &made[0]["effort"],
            &made[0]["permission"]
        ),
        (&json!("opus"), &json!("max"), &json!("accept_edits")),
        "the chat is made on what was asked, by the catalog's id"
    );
    assert_eq!(
        (&made[0]["fast"], &made[0]["plan"]),
        (&json!(true), &json!(true))
    );
    assert_eq!(started["chat"], json!(MADE));
    let mut told = runs_with("opus", "Opus 5.5", Some("max"), "accept_edits");
    (told["fast"], told["plan"]) = (json!(true), json!(true));
    assert_eq!(started["runsWith"], told);
    assert_eq!(
        heard.of("turns.send")[0]["prompt"]["text"],
        json!("Implement it end to end.")
    );
}

#[gpui_kit::test]
fn a_chat_that_asks_for_nothing_says_the_model_its_settings_run_not_the_first(
    cx: &mut TestAppContext,
) {
    let root = repo("control-chat-defaults", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-defaults", &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let agent = terminal("term_1");

    let started = call_through_host(&agent, "chat_create", json!({ "text": "Go." }), cx).unwrap();

    let made = &heard.of("threads.create")[0];
    assert_eq!(
        made["model"],
        Value::Null,
        "left to Claude's settings, as today"
    );
    assert_eq!(
        made["permission"],
        Value::Null,
        "left to the person's settings, as today"
    );
    assert_eq!(
        made["effort"],
        json!("xhigh"),
        "Koyo's setting for new chats, as the composer takes it"
    );
    assert_eq!(
        started["runsWith"],
        runs_with("opus", "Opus 5.5", Some("xhigh"), "auto"),
        "what Claude's settings run, not the recommended row's Fable 5.1"
    );
    let choices = call_through_host(&agent, "chat_choices", json!({}), cx).unwrap();
    assert_eq!(
        choices["defaults"], started["runsWith"],
        "what chat_choices says a new chat gets"
    );
    let ids: Vec<&Value> = (choices["models"].as_array().unwrap().iter())
        .map(|m| &m["id"])
        .collect();
    assert_eq!(ids, [&json!("default"), &json!("opus"), &json!("haiku")]);
    assert_eq!(choices["models"][2]["efforts"], json!([]));
    assert_eq!(
        choices["modes"],
        json!(["ask", "accept_edits", "auto", "full_access"])
    );
}

#[gpui_kit::test]
fn what_a_model_cant_run_with_is_refused_naming_what_it_can(cx: &mut TestAppContext) {
    let root = repo("control-chat-cant", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-cant", &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let agent = terminal("term_1");

    for (asked, says) in [
        (json!({ "model": "gpt-9" }), "`opus` (Opus 5.5)"),
        (
            json!({ "model": "haiku", "effort": "high" }),
            "Haiku 5.5 has no efforts to choose",
        ),
        (
            json!({ "model": "haiku", "mode": "auto" }),
            "`ask`, `accept_edits`, `full_access`",
        ),
        (
            json!({ "model": "haiku", "fast": true }),
            "these have one: `opus` (Opus 5.5)",
        ),
    ] {
        let mut input = asked.clone();
        input["text"] = json!("Go.");
        let refused = call_through_host(&agent, "chat_create", input, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{asked}");
        assert!(
            refused.message.contains(says),
            "{asked}: {}",
            refused.message
        );
    }
    assert!(heard.of("threads.create").is_empty(), "no chat was made");
}

#[gpui_kit::test]
fn a_listed_chat_says_what_it_runs_with_and_a_message_takes_the_effort_shown(
    cx: &mut TestAppContext,
) {
    let root = repo("control-chat-listed", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-listed", &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let chosen = ThreadSummary {
        model: Some("haiku".into()),
        permission: Some(Mode::FullAccess),
        ..summary("chosen", &root, "2026-11-04T11:00:00Z")
    };
    hears(
        &rig,
        vec![summary("left", &root, "2026-11-04T10:00:00Z"), chosen],
        cx,
    );
    let agent = terminal("term_1");

    let listed = call_through_host(&agent, "chat_list", json!({}), cx).unwrap();
    assert_eq!(
        listed["chats"][0]["runsWith"],
        runs_with("haiku", "Haiku 5.5", None, "full_access")
    );
    assert_eq!(
        listed["chats"][1]["runsWith"],
        runs_with("opus", "Opus 5.5", Some("xhigh"), "auto")
    );

    // A chat that chose no effort ran on Claude's own while Koyo showed its setting: it takes
    // the one shown before the message goes.
    call_through_host(
        &agent,
        "chat_send",
        json!({ "chat": "left", "text": "Go on." }),
        cx,
    )
    .unwrap();
    let methods: Vec<String> = heard.methods();
    let (update, turn) = (
        methods.iter().position(|m| m == "threads.update").unwrap(),
        methods.iter().position(|m| m == "turns.send").unwrap(),
    );
    assert!(update < turn, "{methods:?}");
    assert_eq!(heard.of("threads.update")[0]["effort"], json!("xhigh"));
}

/// The owner's words to a chat: "dispatch a worktree from main with opus 5.5 xhigh full access".
/// Their new chats get Auto; a chat in Full access asking for it gets it without asking them, as
/// they chose (2026-11-11: "if i ask the agent to do it, it should be able to do it"), as it's no
/// more than its own (`chat_modes`).
#[gpui_kit::test]
fn a_chat_dispatched_on_opus_xhigh_in_full_access_gets_it_without_asking(cx: &mut TestAppContext) {
    let root = repo("control-chat-dispatched", &[("README.md", "hi\n")]);
    let rig = controlled("control-chat-dispatched", &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let in_full_access = ThreadSummary {
        permission: Some(Mode::FullAccess),
        ..summary("dispatcher", &root, "2026-11-04T10:00:00Z")
    };
    hears(&rig, vec![in_full_access], cx);
    let dispatcher = chat("dispatcher");

    let asked = json!({
        "text": "Implement this end to end.",
        "model": "opus 5.5",
        "effort": "xhigh",
        "mode": "full_access",
    });
    let started = call_through_host(&dispatcher, "chat_create", asked, cx).unwrap();

    let made = &heard.of("threads.create")[0];
    assert_eq!(
        (&made["model"], &made["effort"], &made["permission"]),
        (&json!("opus"), &json!("xhigh"), &json!("full_access"))
    );
    assert_eq!(
        started["runsWith"],
        runs_with("opus", "Opus 5.5", Some("xhigh"), "full_access")
    );
    let confirmed = (activity(cx).into_iter())
        .any(|entry| matches!(entry.kind, ActivityKind::Confirmation { .. }));
    assert!(!confirmed, "the person wasn't asked");
}
