//! A chat's agent gives no chat a mode above its own chat's (decided 2026-11-11), whether it starts
//! one or sets one, its own included; an agent outside a chat isn't held to one. Against a
//! stand-in host whose new chats get Auto.

use gpui_kit::TestAppContext;
use koyo_agent::protocol::model::{Mode, ThreadSummary};
use koyo_app_control::ErrorCode;
use serde_json::{Value, json};

use super::chats::{hears, summary};
use super::host::{Heard, call_through_host, stand_in};
use super::{chat, controlled, terminal};
use crate::test_support::{repo, select};

/// A worktree with `dispatcher`, whose agent calls, in `mode` (or left to the settings, Auto),
/// and `other`, asking first.
fn dispatching(name: &str, mode: Option<Mode>, cx: &mut TestAppContext) -> Heard {
    let root = repo(name, &[("README.md", "hi\n")]);
    let rig = controlled(name, &[&root], cx);
    select(&rig.shell, &root, cx);
    let heard = stand_in(&rig, cx);
    let dispatcher = ThreadSummary {
        permission: mode,
        ..summary("dispatcher", &root, "2026-11-04T10:00:00Z")
    };
    let other = ThreadSummary {
        permission: Some(Mode::Ask),
        ..summary("other", &root, "2026-11-04T11:00:00Z")
    };
    heard.keep(&dispatcher);
    heard.keep(&other);
    hears(&rig, vec![dispatcher, other], cx);
    heard
}

fn create(mode: Option<&str>) -> Value {
    let mut input = json!({ "text": "Go." });
    if let Some(mode) = mode {
        input["mode"] = json!(mode);
    }
    input
}

#[gpui_kit::test]
fn a_chat_starts_no_chat_above_its_own_mode(cx: &mut TestAppContext) {
    let heard = dispatching("control-chat-own-start", Some(Mode::AcceptEdits), cx);
    let dispatcher = chat("dispatcher");

    for above in ["full_access", "auto"] {
        let refused = call_through_host(&dispatcher, "chat_create", create(Some(above)), cx);
        let refused = refused.unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{above}");
        assert!(
            refused.message.contains("runs in `accept_edits`")
                && refused
                    .message
                    .contains("Choose one of: `ask`, `accept_edits`;"),
            "{above}: {}",
            refused.message
        );
    }
    assert!(heard.of("threads.create").is_empty(), "nothing was started");

    call_through_host(&dispatcher, "chat_create", create(Some("ask")), cx).unwrap();
    let left_out = call_through_host(&dispatcher, "chat_create", create(None), cx).unwrap();
    let made = heard.of("threads.create");
    assert_eq!(made[0]["permission"], json!("ask"));
    assert_eq!(
        made[1]["permission"],
        json!("accept_edits"),
        "a new chat would get Auto, above the chat that starts it"
    );
    assert_eq!(left_out["runsWith"]["mode"], json!("accept_edits"));
}

#[gpui_kit::test]
fn a_chat_sets_no_chat_above_its_own_mode_its_own_included(cx: &mut TestAppContext) {
    let heard = dispatching("control-chat-own-set", Some(Mode::AcceptEdits), cx);
    let dispatcher = chat("dispatcher");

    for (chat_id, mode) in [("other", "full_access"), ("dispatcher", "auto")] {
        let asked = json!({ "chat": chat_id, "mode": mode });
        let refused = call_through_host(&dispatcher, "chat_set", asked, cx).unwrap_err();
        assert_eq!(refused.code, ErrorCode::InvalidInput, "{chat_id} {mode}");
        assert!(
            refused.message.contains("runs in `accept_edits`"),
            "{}",
            refused.message
        );
    }
    assert!(heard.of("threads.update").is_empty(), "nothing was changed");

    let to_its_own = json!({ "chat": "other", "mode": "accept_edits" });
    call_through_host(&dispatcher, "chat_set", to_its_own, cx).unwrap();
    let itself_lower = json!({ "chat": "dispatcher", "mode": "ask" });
    let lowered = call_through_host(&dispatcher, "chat_set", itself_lower, cx).unwrap();
    assert_eq!(lowered["runsWith"]["mode"], json!("ask"));
    let modes: Vec<Value> = (heard.of("threads.update").into_iter())
        .map(|update| update["permission"].clone())
        .collect();
    assert_eq!(modes, [json!("accept_edits"), json!("ask")]);
}

#[gpui_kit::test]
fn a_chat_left_to_the_settings_is_held_to_what_they_give_it(cx: &mut TestAppContext) {
    let heard = dispatching("control-chat-own-default", None, cx);
    let dispatcher = chat("dispatcher");

    let refused = call_through_host(&dispatcher, "chat_create", create(Some("full_access")), cx);
    let refused = refused.unwrap_err();
    assert!(
        refused.message.contains("runs in `auto`"),
        "{}",
        refused.message
    );
    call_through_host(&dispatcher, "chat_create", create(Some("auto")), cx).unwrap();
    assert_eq!(heard.of("threads.create")[0]["permission"], json!("auto"));
}

#[gpui_kit::test]
fn an_agent_outside_a_chat_is_held_to_no_chat(cx: &mut TestAppContext) {
    let heard = dispatching("control-chat-own-terminal", Some(Mode::Ask), cx);
    let agent = terminal("term_1");

    call_through_host(&agent, "chat_create", create(Some("full_access")), cx).unwrap();
    let asked = json!({ "chat": "other", "mode": "full_access" });
    call_through_host(&agent, "chat_set", asked, cx).unwrap();
    assert_eq!(
        heard.of("threads.create")[0]["permission"],
        json!("full_access")
    );
    let updates = heard.of("threads.update");
    let other = updates.iter().find(|update| update["threadId"] == "other");
    assert_eq!(other.unwrap()["permission"], json!("full_access"));
}
