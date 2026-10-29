//! A server's life: initialized, shut down, killed when it won't go, and reported when it
//! crashes or breaks the protocol.

use std::time::{Duration, Instant};

use colosso_lsp::{Encoding, Event, Exit, LspError, uri};
use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use serde_json::json;

use crate::common::{Echo, Received, fake, fake_logged, root, wait};

/// The end `events` reports next, which must be a crash.
fn expect_crash(events: &mut UnboundedReceiver<Event>) -> Exit {
    match wait(5, events.next()) {
        Some(Event::Ended(exit)) if exit.crashed => exit,
        other => panic!("no crash reported: {other:?}"),
    }
}

#[test]
fn initialize_offers_our_capabilities_and_takes_the_encoding_chosen() {
    let client = fake(&json!([
        {"echo": "initialize"},
        {"reply": 0, "result": {"capabilities": {"positionEncoding": "utf-8", "textDocumentSync": 2}}},
        {"recv": "initialized"},
        {"recv": "fake/echo"},
        {"reply": 1, "result": null},
    ]));
    let mut received = client.subscribe::<Received>();
    assert_eq!(client.encoding(), Encoding::Utf16);
    let result = wait(5, client.initialize(root())).unwrap();
    assert_eq!(client.encoding(), Encoding::Utf8);
    assert_eq!(client.capabilities().as_deref(), Some(&result.capabilities));
    // Answered only once `initialized` came.
    wait(5, client.request::<Echo>(&json!(null))).unwrap();

    let params = &wait(5, received.next()).unwrap()["params"];
    let capabilities = &params["capabilities"];
    let text = &capabilities["textDocument"];
    assert_eq!(
        capabilities["general"]["positionEncodings"],
        json!(["utf-8", "utf-16"])
    );
    assert_eq!(params["initializationOptions"], json!({"from": "launch"}));
    assert_eq!(
        params["workspaceFolders"][0]["uri"],
        uri::from_path(root()).unwrap().as_str()
    );
    assert_eq!(params["processId"], std::process::id());
    for goto in ["definition", "typeDefinition", "implementation"] {
        assert_eq!(text[goto]["linkSupport"], true, "{goto}");
    }
    assert!(text["references"].is_object());
    let item = &text["completion"]["completionItem"];
    assert_eq!(item["snippetSupport"], true);
    assert_eq!(item["insertReplaceSupport"], true);
    assert_eq!(item["labelDetailsSupport"], true);
    assert_eq!(
        item["resolveSupport"]["properties"],
        json!(["documentation", "detail", "additionalTextEdits"])
    );
    assert_eq!(
        text["hover"]["contentFormat"],
        json!(["markdown", "plaintext"])
    );
    assert_eq!(text["rename"]["prepareSupport"], true);
    assert_eq!(
        text["codeAction"]["resolveSupport"]["properties"],
        json!(["edit"])
    );
    assert_eq!(
        text["documentSymbol"]["hierarchicalDocumentSymbolSupport"],
        true
    );
    assert_eq!(text["publishDiagnostics"]["versionSupport"], true);
    assert_eq!(text["publishDiagnostics"]["relatedInformation"], true);
    assert_eq!(
        text["semanticTokens"]["requests"],
        json!({"range": true, "full": {"delta": true}})
    );
    for present in [
        "signatureHelp",
        "documentHighlight",
        "formatting",
        "inlayHint",
    ] {
        assert!(text[present].is_object(), "{present}");
    }
    let workspace = &capabilities["workspace"];
    assert_eq!(
        workspace["didChangeWatchedFiles"]["dynamicRegistration"],
        true
    );
    assert_eq!(workspace["configuration"], true);
    assert!(workspace["symbol"].is_object());
    assert_eq!(capabilities["window"]["workDoneProgress"], true);
    assert_eq!(capabilities["window"]["showDocument"]["support"], true);
    assert_eq!(
        capabilities["experimental"]["serverStatusNotification"],
        true
    );
}

#[test]
fn a_server_that_names_no_encoding_speaks_utf16() {
    let client = fake(&json!([
        {"recv": "initialize"},
        {"reply": 0, "result": {"capabilities": {}}},
        {"recv": "initialized"},
    ]));
    wait(5, client.initialize(root())).unwrap();
    assert_eq!(client.encoding(), Encoding::Utf16);
}

#[test]
fn stderr_is_added_to_the_log_as_its_written() {
    // In the checkout's own target folder, wherever cargo builds.
    let log = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp/lsp-log/fake.log");
    std::fs::remove_file(&log).ok();
    // A log there already is added to, its folder made the first time.
    for said in ["first", "second"] {
        let client = fake_logged(
            &json!([{"stderr": format!("{said} line")}, {"exit": 0}]),
            Some(log.clone()),
        );
        let mut events = client.events();
        while let Some(event) = wait(5, events.next()) {
            if matches!(event, Event::Ended(_)) {
                break;
            }
        }
    }
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "first line\nsecond line\n"
    );
}

#[test]
fn a_crash_is_reported_with_its_last_stderr_and_fails_what_waits() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"stderr": "loading the workspace"},
        {"stderr": "thread 'main' panicked: boom"},
        {"exit": 3},
    ]));
    let mut events = client.events();
    let mut received = client.subscribe::<Received>();
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(null))),
        Err(LspError::Exited)
    );
    let exit = expect_crash(&mut events);
    assert_eq!(exit.code, Some(3));
    // Notification streams end with the server.
    assert_eq!(wait(5, received.next()), None);
    assert!(exit.crashed && !exit.killed);
    assert_eq!(
        exit.stderr,
        ["loading the workspace", "thread 'main' panicked: boom"]
    );
    // Afterwards, everything fails at once, and a late listener still hears of the crash.
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(null))),
        Err(LspError::Exited)
    );
    assert_eq!(
        client.notify::<Received>(&json!(null)),
        Err(LspError::Exited)
    );
    assert_eq!(
        wait(5, client.events().next()),
        Some(Event::Ended(exit.clone()))
    );
    assert_eq!(client.exit(), Some(exit));
}

#[test]
fn a_broken_frame_stops_the_server_as_a_crash() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"raw": "Content-Length: many\r\n\r\n{}"},
        {"hang": true},
    ]));
    let mut events = client.events();
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(null))),
        Err(LspError::Exited)
    );
    let exit = expect_crash(&mut events);
    assert!(exit.killed);
    assert!(
        exit.stderr.last().unwrap().contains("unreadable output"),
        "{exit:?}"
    );
}

#[test]
fn shutdown_asks_and_the_server_exits() {
    let client = fake(&json!([
        {"recv": "shutdown"},
        {"reply": 0, "result": null},
        {"recv": "exit"},
        {"exit": 0},
    ]));
    let mut events = client.events();
    let exit = wait(10, client.shutdown(Duration::from_secs(5)));
    assert_eq!(exit.code, Some(0));
    assert!(!exit.killed && !exit.crashed);
    assert_eq!(wait(5, events.next()), Some(Event::Ended(exit)));
}

#[test]
fn shutdown_kills_a_server_that_wont_exit() {
    let client = fake(&json!([
        {"recv": "shutdown"},
        // The unanswered shutdown is given up on, and so cancelled.
        {"recv": "$/cancelRequest"},
        {"recv": "exit"},
        {"hang": true},
    ]));
    let timeout = Duration::from_millis(300);
    let started = Instant::now();
    let exit = wait(10, client.shutdown(timeout));
    let took = started.elapsed();
    assert!(exit.killed && !exit.crashed, "{exit:?}");
    assert_eq!(exit.code, None);
    // One timeout for the unanswered shutdown, one for the exit that didn't happen.
    assert!(
        took >= timeout * 2 && took < Duration::from_secs(5),
        "{took:?}"
    );
}

#[test]
fn dropping_the_last_client_kills_the_server() {
    let client = fake(&json!([{"hang": true}]));
    let exited = client.exited();
    drop(client);
    let exit = wait(5, exited);
    assert!(exit.killed && !exit.crashed, "{exit:?}");
}
