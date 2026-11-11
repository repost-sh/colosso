//! Another chat set to what's asked, as its composer's chips would set it, against a stand-in
//! host: what the host hears, what the caller is told, and what's refused.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::model::{Effort, Mode, ThreadSummary};
use koyo_app_control::ErrorCode;
use serde_json::{Value, json};

use super::chats::{hears, runs_with, summary};
use super::host::{Heard, call_through_host, stand_in};
use super::{Rig, chat, controlled};
use crate::test_support::{repo, select};

/// A worktree with two chats the host keeps: `dispatcher`, whose agent calls, in Full access, and
/// `other`, on Opus 5.5 at Max, fast, asking first.
fn two_chats(name: &str, cx: &mut TestAppContext) -> (Rig, Heard, ThreadSummary) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let rig = controlled(name, &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let other = ThreadSummary {
        model: Some("opus".into()),
        effort: Some(Effort::Max),
        fast: true,
        permission: Some(Mode::Ask),
        ..summary("other", &root, "2026-11-04T11:00:00Z")
    };
    let dispatcher = ThreadSummary {
        permission: Some(Mode::FullAccess),
        ..summary("dispatcher", &root, "2026-11-04T10:00:00Z")
    };
    heard.keep(&other);
    hears(&rig, vec![dispatcher, other.clone()], cx);
    (rig, heard, other)
}

#[gpui_kit::test]
fn another_chat_is_set_to_what_is_asked_and_the_rest_stays(cx: &mut TestAppContext) {
    let (_rig, heard, _) = two_chats("control-chat-set", cx);
    let dispatcher = chat("dispatcher");

    let asked = json!({ "chat": "other", "effort": "xhigh", "mode": "full_access" });
    let set = call_through_host(&dispatcher, "chat_set", asked, cx).unwrap();

    let update = &heard.of("threads.update")[0];
    assert_eq!(update["threadId"], json!("other"));
    assert_eq!(
        (&update["effort"], &update["permission"]),
        (&json!("xhigh"), &json!("full_access"))
    );
    for kept in ["model", "fast", "plan"] {
        assert!(
            update.get(kept).is_none(),
            "{kept} isn't asked of the host: {update}"
        );
    }
    let mut told = runs_with("opus", "Opus 5.5", Some("xhigh"), "full_access");
    told["fast"] = json!(true);
    assert_eq!(set["runsWith"], told);
    assert_eq!(set["chat"], json!("other"));
}

#[gpui_kit::test]
fn another_model_keeps_only_what_it_can_do(cx: &mut TestAppContext) {
    let (_rig, heard, _) = two_chats("control-chat-set-model", cx);
    let dispatcher = chat("dispatcher");

    let asked = json!({ "chat": "other", "model": "haiku" });
    let set = call_through_host(&dispatcher, "chat_set", asked, cx).unwrap();

    let update = &heard.of("threads.update")[0];
    assert_eq!(update["model"], json!("haiku"));
    assert_eq!(update["effort"], Value::Null, "Haiku 5.5 has no efforts");
    assert_eq!(update["fast"], json!(false), "nor a fast mode");
    assert!(update.get("permission").is_none(), "Ask stays: {update}");
    let haiku = runs_with("haiku", "Haiku 5.5", None, "ask");
    assert_eq!(set["runsWith"], haiku);
}

#[gpui_kit::test]
fn what_another_chats_model_cant_run_with_is_refused(cx: &mut TestAppContext) {
    let (rig, heard, other) = two_chats("control-chat-set-refused", cx);
    let haiku = ThreadSummary {
        model: Some("haiku".into()),
        effort: None,
        fast: false,
        ..other
    };
    heard.keep(&haiku);
    hears(&rig, vec![haiku], cx);
    let dispatcher = chat("dispatcher");

    for (asked, code, says) in [
        (
            json!({ "chat": "other" }),
            ErrorCode::InvalidInput,
            "nothing to change",
        ),
        (
            json!({ "chat": "other", "effort": "high" }),
            ErrorCode::InvalidInput,
            "Haiku 5.5 has no efforts to choose",
        ),
        (
            json!({ "chat": "other", "mode": "auto" }),
            ErrorCode::InvalidInput,
            "Haiku 5.5 can't run in `auto`",
        ),
        (
            json!({ "chat": "other", "model": "gpt-9" }),
            ErrorCode::InvalidInput,
            "`opus` (Opus 5.5)",
        ),
        (
            json!({ "chat": "nobody", "mode": "ask" }),
            ErrorCode::NotFound,
            "nobody",
        ),
    ] {
        let refused = call_through_host(&dispatcher, "chat_set", asked.clone(), cx).unwrap_err();
        assert_eq!(refused.code, code, "{asked}");
        assert!(
            refused.message.contains(says),
            "{asked}: {}",
            refused.message
        );
    }
    assert!(heard.of("threads.update").is_empty(), "nothing changed");
}
