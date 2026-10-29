//! A server that reports its status: waited on until it's ready, and asked again what it
//! gave up on while it reloaded.

use colosso_lsp::lsp_types::error_codes::CONTENT_MODIFIED;
use colosso_lsp::{LspError, ServerStatus};
use futures::future::join;
use futures::{FutureExt, StreamExt};
use serde_json::{Value, json};

use crate::common::{Echo, Received, fake, wait};

fn status(quiescent: bool) -> Value {
    json!({"send": {"method": "experimental/serverStatus", "params": {
        "health": "ok",
        "quiescent": quiescent,
    }}})
}

fn content_modified(request: usize) -> Value {
    json!({"reply": request, "error": {"code": CONTENT_MODIFIED, "message": "content modified"}})
}

#[test]
fn ready_waits_until_the_server_says_it_is_quiescent() {
    let client = fake(&json!([
        status(false),
        {"recv": "fake/echo"},
        {"reply": 0, "result": null},
        {"recv": "fake/echo"},
        status(true),
        {"reply": 1, "result": null},
    ]));
    let mut statuses = client.subscribe::<ServerStatus>();
    // Each answer comes after the status sent before it, so it's been taken in by then.
    wait(5, client.request::<Echo>(&json!(null))).unwrap();
    assert_eq!(client.ready().now_or_never(), None);
    let waiting = client.ready();
    wait(5, client.request::<Echo>(&json!(null))).unwrap();
    wait(5, waiting).unwrap();
    wait(5, client.ready()).unwrap();
    assert!(!wait(5, statuses.next()).unwrap().quiescent);
    assert!(wait(5, statuses.next()).unwrap().quiescent);
}

#[test]
fn a_request_overtaken_by_a_reload_is_asked_again_once_the_server_is_ready() {
    let client = fake(&json!([
        {"recv": "fake/echo"},
        {"recv": "fake/echo"},
        status(false),
        content_modified(0),
        {"reply": 1, "result": "meanwhile"},
        // Sent once the second answer is in. Asked again at once, the first request would
        // have come before it.
        {"recv": "fake/received"},
        status(true),
        {"recv": "fake/echo"},
        {"reply": 2, "result": "answered"},
    ]));
    let first = json!("first");
    let retrying = client.request_retrying::<Echo>(&first);
    let meanwhile = async {
        let answer = client.request::<Echo>(&json!("second")).await;
        client.notify::<Received>(&json!(null)).unwrap();
        answer
    };
    let (first, second) = wait(5, join(retrying, meanwhile));
    assert_eq!(second.unwrap(), json!("meanwhile"));
    assert_eq!(first.unwrap(), json!("answered"));
}

#[test]
fn a_request_is_asked_again_three_times_at_most() {
    let mut script = Vec::new();
    for attempt in 0..4 {
        script.extend([json!({"recv": "fake/echo"}), content_modified(attempt)]);
    }
    script.extend([
        json!({"recv": "fake/echo"}),
        json!({"reply": 4, "result": "next"}),
    ]);
    let client = fake(&Value::Array(script));
    let answer = wait(5, client.request_retrying::<Echo>(&json!(null)));
    assert!(
        matches!(
            answer,
            Err(LspError::Server {
                code: CONTENT_MODIFIED,
                ..
            })
        ),
        "{answer:?}"
    );
    // Had it asked a fifth time, this would have been answered as that.
    assert_eq!(
        wait(5, client.request::<Echo>(&json!(null))).unwrap(),
        json!("next")
    );
}

#[test]
fn ready_fails_once_the_server_has_ended() {
    let client = fake(&json!([status(false), {"exit": 1}]));
    assert_eq!(wait(5, client.ready()), Err(LspError::Exited));
    assert_eq!(wait(5, client.ready()), Err(LspError::Exited));
}
