//! Requests, answers and notifications, however the server frames and orders them.

use std::sync::Arc;

use colosso_lsp::lsp_types::error_codes::REQUEST_CANCELLED;
use colosso_lsp::lsp_types::notification::{LogMessage, Progress, PublishDiagnostics, ShowMessage};
use colosso_lsp::lsp_types::{MessageType, NumberOrString, ProgressParamsValue, WorkDoneProgress};
use colosso_lsp::{LspError, ResponseError};
use futures::StreamExt;
use futures::future::join;
use serde_json::{Value, json};

use crate::common::{Echo, Number, Received, fake, wait};

#[test]
fn answers_find_their_requests_in_any_order() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"recv": "fake/echo"},
        {"reply": 1, "result": "second"},
        {"reply": 0, "result": "first"},
    ]));
    let first = client.request::<Echo>(&json!(1));
    let second = client.request::<Echo>(&json!(2));
    let (first, second) = wait(5, join(first, second));
    assert_eq!(first.unwrap(), json!("first"));
    assert_eq!(second.unwrap(), json!("second"));
}

#[test]
fn frames_split_byte_by_byte_or_merged_are_read() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"bytewise": {"method": "fake/received", "params": "one"}},
        {"merged": [
            {"method": "fake/received", "params": "two"},
            {"id": 1, "result": "answer"},
            {"method": "fake/received", "params": "three"},
        ]},
    ]));
    let mut received = client.subscribe::<Received>();
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(null))).unwrap(),
        json!("answer")
    );
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(Value::clone(&wait(5, received.next()).unwrap()));
    }
    assert_eq!(seen, [json!("one"), json!("two"), json!("three")]);
}

#[test]
fn a_dropped_request_is_cancelled_and_its_late_answer_dropped() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"echo": "$/cancelRequest"},
        {"reply": 0, "result": "too late"},
        {"recv": "fake/echo"},
        {"reply": 1, "result": "in time"},
    ]));
    let mut received = client.subscribe::<Received>();
    let slow = client.request::<Echo>(&json!("slow"));
    let id = slow.id();
    drop(slow);
    let cancel = wait(5, received.next()).unwrap();
    assert_eq!(cancel["method"], "$/cancelRequest");
    assert_eq!(cancel["params"]["id"], id);
    // The late answer came first, and was nobody's.
    let next = client.request::<Echo>(&json!("next"));
    assert_eq!(wait(5, next).unwrap(), json!("in time"));
}

#[test]
fn errors_say_whether_the_server_failed_cancelled_or_broke_the_protocol() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"reply": 0, "error": {"code": ResponseError::INVALID_PARAMS, "message": "bad params", "data": [1]}},
        {"recv": "fake/echo"},
        {"reply": 1, "error": {"code": REQUEST_CANCELLED, "message": "cancelled"}},
        {"recv": "fake/number"},
        {"reply": 2, "result": "not a number"},
    ]));
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(1))),
        Err(LspError::Server {
            code: ResponseError::INVALID_PARAMS,
            message: "bad params".to_owned(),
            data: Some(json!([1])),
        })
    );
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(2))),
        Err(LspError::Cancelled)
    );
    let wrong = wait(5, client.request::<Number>(&json!(3)));
    assert!(matches!(wrong, Err(LspError::Protocol(_))), "{wrong:?}");
}

#[test]
fn notifications_reach_each_subscriber_parsed_once() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"send": {"method": "textDocument/publishDiagnostics", "params": {
            "uri": "file:///a.rs",
            "version": 3,
            "diagnostics": [{
                "range": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 5}},
                "message": "boom",
            }],
        }}},
        {"send": {"method": "window/logMessage", "params": {"type": 3, "message": "hello"}}},
        {"send": {"method": "$/progress", "params": {
            "token": "indexing",
            "value": {"kind": "begin", "title": "Indexing"},
        }}},
        {"send": {"method": "window/showMessage", "params": {"type": 1, "message": "oops"}}},
        {"reply": 0, "result": null},
    ]));
    let mut diagnostics = client.subscribe::<PublishDiagnostics>();
    let mut also_diagnostics = client.subscribe::<PublishDiagnostics>();
    let mut logs = client.subscribe::<LogMessage>();
    let mut progress = client.subscribe::<Progress>();
    let mut shown = client.subscribe::<ShowMessage>();
    wait(5, client.request::<Echo>(&json!(null))).unwrap();

    let published = wait(5, diagnostics.next()).unwrap();
    assert_eq!(published.version, Some(3));
    assert_eq!(published.diagnostics[0].message, "boom");
    assert_eq!(published.diagnostics[0].range.end.character, 5);
    // Both subscribers share the one parse.
    let also = wait(5, also_diagnostics.next()).unwrap();
    assert!(Arc::ptr_eq(&also, &published));
    let log = wait(5, logs.next()).unwrap();
    assert_eq!(
        (log.typ, log.message.as_str()),
        (MessageType::INFO, "hello")
    );
    let report = wait(5, progress.next()).unwrap();
    assert_eq!(report.token, NumberOrString::String("indexing".to_owned()));
    let ProgressParamsValue::WorkDone(WorkDoneProgress::Begin(begin)) = &report.value else {
        panic!("not a begin: {report:?}");
    };
    assert_eq!(begin.title, "Indexing");
    let message = wait(5, shown.next()).unwrap();
    assert_eq!(
        (message.typ, message.message.as_str()),
        (MessageType::ERROR, "oops")
    );
}
