//! `activity_list` as agents read the log: newest first, with each call's summary or error and
//! each answer the person gave; every client's entries for an agent Koyo runs, its own for an
//! external client; and the listing, a read, left out of the log.

use gpui_kit::TestAppContext;
use koyo_app_control::Scope;
use serde_json::{Value, json};

use super::{allow, call, chat, controlled, external, terminal};
use crate::control::allow_denied;
use crate::test_support::{repo, select};

/// The entries of an `activity_list` answer, and whether they're only the caller's.
fn listed(answer: &Value) -> (&[Value], bool) {
    let entries = answer["entries"].as_array().expect("entries");
    (entries, answer["onlyYours"].as_bool().expect("onlyYours"))
}

#[gpui_kit::test]
fn agents_read_what_every_client_did_and_a_client_what_it_did(cx: &mut TestAppContext) {
    let root = repo("control-activity", &[("README.md", "hi\n")]);
    let rig = controlled("control-activity", &[&root], cx);
    select(&rig.shell, &root, cx);
    allow("Cursor", &[Scope::Read, Scope::Navigate], cx);
    let agent = terminal("term_1");
    let cursor = external("Cursor");
    call(&agent, "layout_set", json!({ "sidebar": false }), cx).unwrap();
    // Refused for want of a scope, and logged all the same.
    call(
        &cursor,
        "settings_set",
        json!({ "previewFiles": false }),
        cx,
    )
    .unwrap_err();
    // Reads aren't logged.
    call(&cursor, "app_status", json!({}), cx).unwrap();
    cx.update(|cx| allow_denied("Codex", cx));

    let everything = call(&agent, "activity_list", json!({}), cx).unwrap();
    let (entries, only_yours) = listed(&everything);
    assert!(!only_yours);
    assert_eq!(entries.len(), 3, "{entries:?}");
    assert_eq!(
        entries[0],
        json!({
            "at": entries[0]["at"], "client": "Codex", "kind": "approval",
            "decision": "allowed"
        })
    );
    assert_eq!(entries[1]["client"], "Cursor");
    assert_eq!(entries[1]["kind"], "call");
    assert_eq!(entries[1]["operation"], "settings_set");
    assert_eq!(entries[1]["error"]["code"], "scope_denied");
    assert!(entries[1].get("summary").is_none());
    assert_eq!(entries[2]["client"], "terminal:term_1");
    assert_eq!(entries[2]["operation"], "layout_set");
    assert!(entries[2]["summary"].is_string() && entries[2].get("error").is_none());
    assert!(
        entries[0]["at"].as_u64() >= entries[2]["at"].as_u64(),
        "newest first"
    );

    // A chat's agent reads it all too, as far as its `limit` goes.
    let latest = call(&chat("chat-1"), "activity_list", json!({ "limit": 1 }), cx).unwrap();
    let (entries, only_yours) = listed(&latest);
    assert!(!only_yours);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["client"], "Codex");

    // An external client reads only its own, not who else the person allowed.
    let own = call(&cursor, "activity_list", json!({}), cx).unwrap();
    let (entries, only_yours) = listed(&own);
    assert!(only_yours);
    assert_eq!(entries.len(), 1, "{entries:?}");
    assert_eq!(entries[0]["client"], "Cursor");
    assert_eq!(entries[0]["operation"], "settings_set");

    // None of the listings above is in the log.
    let again = call(&agent, "activity_list", json!({ "limit": 200 }), cx).unwrap();
    assert_eq!(listed(&again).0.len(), 3);
}
