//! The server's requests to the client: answered by default, or by the owner's handlers.

use std::thread;

use colosso_lsp::lsp_types::request::{ApplyWorkspaceEdit, ShowDocument, WorkspaceConfiguration};
use colosso_lsp::lsp_types::{ApplyWorkspaceEditResponse, GlobPattern};
use colosso_lsp::{Event, ResponseError};
use futures::StreamExt;
use serde_json::{Value, json};

use crate::common::{Echo, Received, fake, wait};

/// The fake sends `request`, and the client's answer comes back as an echo.
fn ask(id: &str, method: &str, params: &Value) -> [Value; 2] {
    [
        json!({"send": {"id": id, "method": method, "params": params}}),
        json!({"echo_reply": true}),
    ]
}

#[test]
fn server_requests_get_answers_that_keep_it_going() {
    let mut script = vec![json!({"recv": "fake/echo"})];
    script.extend(ask(
        "config",
        "workspace/configuration",
        &json!({"items": [{"section": "fake"}, {"section": "fake.answer"}, {"section": "other"}]}),
    ));
    script.extend(ask(
        "progress",
        "window/workDoneProgress/create",
        &json!({"token": "t"}),
    ));
    script.extend(ask("edit", "workspace/applyEdit", &json!({"edit": {}})));
    script.extend(ask(
        "show",
        "window/showDocument",
        &json!({"uri": "file:///a.rs"}),
    ));
    script.extend(ask("unknown", "fake/unknown", &json!({})));
    script.push(json!({"reply": 0, "result": null}));
    let client = fake(&Value::Array(script));
    let mut received = client.subscribe::<Received>();
    wait(5, client.request::<Echo>(&json!(null))).unwrap();

    let mut answers = Vec::new();
    for _ in 0..5 {
        answers.push(wait(5, received.next()).unwrap());
    }
    assert_eq!(answers[0]["id"], "config");
    assert_eq!(answers[0]["result"], json!([{"answer": 42}, 42, null]));
    assert_eq!(answers[1]["result"], Value::Null);
    assert_eq!(answers[2]["result"]["applied"], false);
    assert_eq!(answers[3]["result"], json!({"success": false}));
    assert_eq!(answers[4]["error"]["code"], ResponseError::METHOD_NOT_FOUND);
}

#[test]
fn registrations_are_kept_until_unregistered() {
    let mut script = vec![json!({"recv": "fake/echo"})];
    script.extend(ask(
        "register",
        "client/registerCapability",
        &json!({"registrations": [{
            "id": "watch",
            "method": "workspace/didChangeWatchedFiles",
            "registerOptions": {"watchers": [
                {"globPattern": "**/*.rs"},
                {"globPattern": "**/Cargo.toml", "kind": 7},
            ]},
        }]}),
    ));
    script.push(json!({"reply": 0, "result": null}));
    script.push(json!({"recv": "fake/echo"}));
    script.extend(ask(
        "unregister",
        "client/unregisterCapability",
        &json!({"unregisterations": [{"id": "watch", "method": "workspace/didChangeWatchedFiles"}]}),
    ));
    script.push(json!({"reply": 1, "result": null}));
    let client = fake(&Value::Array(script));
    let mut events = client.events();
    let mut received = client.subscribe::<Received>();

    wait(5, client.request::<Echo>(&json!(null))).unwrap();
    assert_eq!(wait(5, received.next()).unwrap()["result"], Value::Null);
    assert_eq!(wait(5, events.next()), Some(Event::RegistrationsChanged));
    let globs: Vec<_> = client
        .watched_files()
        .into_iter()
        .map(|watcher| match watcher.glob_pattern {
            GlobPattern::String(glob) => glob,
            GlobPattern::Relative(relative) => relative.pattern,
        })
        .collect();
    assert_eq!(globs, ["**/*.rs", "**/Cargo.toml"]);
    assert_eq!(client.registrations()[0].id, "watch");

    wait(5, client.request::<Echo>(&json!(null))).unwrap();
    assert_eq!(wait(5, received.next()).unwrap()["result"], Value::Null);
    assert_eq!(wait(5, events.next()), Some(Event::RegistrationsChanged));
    assert!(client.registrations().is_empty());
    assert!(client.watched_files().is_empty());
}

#[test]
fn owner_handlers_answer_now_later_or_by_default() {
    let mut script = vec![json!({"recv": "fake/echo"})];
    script.extend(ask("edit", "workspace/applyEdit", &json!({"edit": {}})));
    script.push(json!({"send": {"id": "show", "method": "window/showDocument", "params": {"uri": "file:///a.rs"}}}));
    // What the handler sent the server before it gave up on answering.
    script.push(json!({"echo": "fake/received"}));
    script.push(json!({"echo_reply": true}));
    script.extend(ask(
        "config",
        "workspace/configuration",
        &json!({"items": [{"section": "x"}]}),
    ));
    script.push(json!({"reply": 0, "result": null}));
    let client = fake(&Value::Array(script));
    // Answered later, from another thread, as an editor applies an edit on its own.
    client.handle::<ApplyWorkspaceEdit>(|_, reply, _| {
        thread::spawn(move || {
            reply.send(Ok(ApplyWorkspaceEditResponse {
                applied: true,
                failure_reason: None,
                failed_change: None,
            }));
        });
    });
    // Tells the server something through its peer, then drops the reply unanswered: the
    // default answer goes instead.
    client.handle::<ShowDocument>(|params, reply, peer| {
        peer.notify::<Received>(&json!(params.uri.as_str()))
            .unwrap();
        drop(reply);
    });
    client.handle::<WorkspaceConfiguration>(|params, reply, _| {
        reply.send(Ok(params.items.iter().map(|_| json!("mine")).collect()));
    });
    let mut received = client.subscribe::<Received>();
    wait(5, client.request::<Echo>(&json!(null))).unwrap();

    let edit = wait(5, received.next()).unwrap();
    assert_eq!(edit["result"], json!({"applied": true}));
    let told = wait(5, received.next()).unwrap();
    assert_eq!(told["params"], "file:///a.rs");
    let show = wait(5, received.next()).unwrap();
    assert_eq!(show["result"], json!({"success": false}));
    let config = wait(5, received.next()).unwrap();
    assert_eq!(config["result"], json!(["mine"]));
}
