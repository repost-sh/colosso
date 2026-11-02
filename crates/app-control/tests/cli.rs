//! `colosso app`: reading its arguments, its help from the catalog, and calls against a fake
//! app, printing results and error envelopes as a script reads them.

#![cfg(unix)]

// Marked as test code, as everything here is, so the lints' leave for tests (to unwrap, to
// panic) reaches the helpers too.
#[cfg(test)]
mod common;

use colosso_app_control::cli::{self, CLIENT_NAME, Invocation};
use colosso_app_control::frame::{MAX_SERVER_FRAME, ServerFrame};
use colosso_app_control::{ControlError, Endpoint, ErrorCode, Scope, catalog};
use common::{next_call, spawn_app, temp_dir};
use serde_json::{Value, json};

#[cfg(test)]
fn parse(args: &[&str]) -> Result<Invocation, ControlError> {
    cli::parse(args.iter().map(|arg| (*arg).to_owned()))
}

#[cfg(test)]
fn call(operation: &str, input: Value, request_id: Option<&str>, client: &str) -> Invocation {
    Invocation::Call {
        operation: operation.into(),
        input,
        request_id: request_id.map(str::to_owned),
        client: client.into(),
    }
}

/// Runs `invocation` against `endpoint`, answering the exit code and what it printed.
#[cfg(test)]
fn run(invocation: &Invocation, endpoint: Endpoint) -> (u8, String) {
    let mut out = Vec::new();
    let code = cli::run(invocation, || Ok(endpoint), &mut out);
    (code, String::from_utf8(out).unwrap())
}

#[test]
fn help_is_asked_for_in_several_ways() {
    for args in [&[][..], &["help"], &["--help"], &["-h"]] {
        assert_eq!(parse(args).unwrap(), Invocation::Help(None), "{args:?}");
    }
    assert_eq!(
        parse(&["help", "tab_open"]).unwrap(),
        Invocation::Help(Some("tab_open".into()))
    );
    assert_eq!(
        parse(&["tab_open", "--help"]).unwrap(),
        Invocation::Help(Some("tab_open".into()))
    );
    assert!(parse(&["help", "tab_open", "more"]).is_err());
}

#[test]
fn a_call_takes_json_a_request_id_and_a_client_name() {
    assert_eq!(
        parse(&["app_status"]).unwrap(),
        call("app_status", json!({}), None, CLIENT_NAME)
    );
    assert_eq!(
        parse(&[
            "tab_open",
            "--json",
            r#"{"kind":"browser"}"#,
            "--request-id",
            "r-1",
            "--client",
            "ci"
        ])
        .unwrap(),
        call("tab_open", json!({ "kind": "browser" }), Some("r-1"), "ci")
    );
    assert_eq!(
        parse(&[
            "tab_open",
            r#"--json={"kind":"terminal"}"#,
            "--request-id=r-2"
        ])
        .unwrap(),
        call(
            "tab_open",
            json!({ "kind": "terminal" }),
            Some("r-2"),
            CLIENT_NAME
        )
    );
    // A requestId in the input moves to the call, where the socket takes it.
    assert_eq!(
        parse(&["tab_reopen", "--json", r#"{"requestId":"r-3"}"#]).unwrap(),
        call("tab_reopen", json!({}), Some("r-3"), CLIENT_NAME)
    );
}

#[test]
fn malformed_arguments_are_invalid_input() {
    let cases: &[&[&str]] = &[
        &["--json", "{}"],
        &["app_status", "--bogus"],
        &["app_status", "extra"],
        &["app_status", "--json"],
        &["app_status", "--json", "{}", "--json", "{}"],
        &["app_status", "--json", "[1]"],
        &["app_status", "--json", "{nope"],
        &["app_status", "--request-id", "has space"],
        &[
            "tab_reopen",
            "--request-id",
            "a",
            "--json",
            r#"{"requestId":"b"}"#,
        ],
        &["app_status", "--client", " "],
    ];
    for args in cases {
        let error = parse(args).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidInput, "{args:?}");
    }
}

#[test]
fn help_lists_every_operation_and_shows_one_with_its_schema() {
    let (code, text) = run(&Invocation::Help(None), Endpoint::at("/nowhere"));
    assert_eq!(code, 0);
    for descriptor in catalog() {
        let line = text
            .lines()
            .find(|line| line.split_whitespace().next() == Some(descriptor.name))
            .unwrap_or_else(|| panic!("{} isn't listed", descriptor.name));
        assert!(line.contains(descriptor.scope.id()), "{line}");
        assert!(line.contains(descriptor.title), "{line}");
    }
    let (code, text) = run(
        &Invocation::Help(Some("worktree_delete".into())),
        Endpoint::at("/nowhere"),
    );
    assert_eq!(code, 0);
    assert!(text.contains("Scope: destructive"), "{text}");
    assert!(text.contains("confirms each call"), "{text}");
    assert!(text.contains("\"additionalProperties\": false"), "{text}");
    let (code, text) = run(
        &Invocation::Help(Some("nope".into())),
        Endpoint::at("/nowhere"),
    );
    assert_eq!(code, 1);
    assert_eq!(envelope(&text)["code"], "invalid_input");
}

#[test]
fn without_an_app_a_call_prints_app_not_running() {
    let socket = temp_dir("cli-none").join("app.sock");
    let (code, text) = run(
        &call("app_status", json!({}), None, CLIENT_NAME),
        Endpoint::at(socket),
    );
    assert_eq!(code, 1);
    let error = envelope(&text);
    assert_eq!(error["code"], "app_not_running");
    assert_eq!(error["retryable"], true);
}

#[test]
fn an_unknown_operation_fails_before_connecting() {
    let (code, text) = run(
        &call("app_launch", json!({}), None, CLIENT_NAME),
        Endpoint::at("/nowhere"),
    );
    assert_eq!(code, 1);
    let error = envelope(&text);
    assert_eq!(error["code"], "invalid_input");
    assert!(
        error["message"].as_str().unwrap().contains("relay"),
        "{error}"
    );
}

#[test]
fn a_call_prints_the_apps_output_then_its_error_envelope() {
    let (endpoint, app) = spawn_app(
        "cli",
        Scope::DEFAULT_GRANT.to_vec(),
        |hello, mut reader, writer| {
            assert_eq!(hello.client.name, "ci");
            assert!(hello.cwd.is_some());
            let call = next_call(&mut reader);
            assert_eq!(call.operation, "project_list");
            assert_eq!(call.request_id, None);
            let output = ServerFrame::answer(call.id, Ok(json!({ "projects": [] })));
            writer.write(&output, MAX_SERVER_FRAME).unwrap();
        },
    );
    let (code, text) = run(&call("project_list", json!({}), None, "ci"), endpoint);
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({ "projects": [] })
    );
    app.join().unwrap();

    let (endpoint, app) = spawn_app("cli-err", vec![Scope::Read], |_, mut reader, writer| {
        let call = next_call(&mut reader);
        assert_eq!(call.request_id.as_deref(), Some("del-1"));
        let refused =
            ServerFrame::answer(call.id, Err(ControlError::scope_denied(Scope::Destructive)));
        writer.write(&refused, MAX_SERVER_FRAME).unwrap();
    });
    let delete = call(
        "worktree_delete",
        json!({ "worktree": "feat" }),
        Some("del-1"),
        CLIENT_NAME,
    );
    let (code, text) = run(&delete, endpoint);
    assert_eq!(code, 1);
    let error = envelope(&text);
    assert_eq!(error["code"], "scope_denied");
    assert_eq!(error["details"]["scope"], "destructive");
    app.join().unwrap();
}

/// The error of an envelope the CLI printed, which is all it printed.
#[cfg(test)]
fn envelope(text: &str) -> Value {
    let printed: Value = serde_json::from_str(text).unwrap();
    let object = printed.as_object().unwrap();
    assert_eq!(object.len(), 1, "{text}");
    printed["error"].clone()
}
