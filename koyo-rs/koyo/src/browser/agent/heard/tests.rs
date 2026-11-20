//! What agents hear of a page, from the events the engine sends for the domains they enabled.

use koyo_app_control::ops::browser::{ConsoleLevel, KEPT};
use serde_json::json;

use super::{Frame, Heard};

/// A page's console says nothing until an agent asks for it, then keeps its calls, the errors
/// nothing caught and the engine's own entries, each with where it came from.
#[test]
fn the_console_is_kept_from_when_an_agent_asks() {
    let mut heard = Heard::default();
    let logged = json!({
        "type": "log",
        "args": [{ "type": "string", "value": "ready" }, { "type": "number", "value": 3 },
            { "type": "object", "description": "Object" }, { "type": "undefined" }],
        "timestamp": 1_700_000_000_000.5,
        "stackTrace": { "callFrames": [{ "url": "https://a.example/app.js", "lineNumber": 9 }] },
    });
    heard.heard(None, "Runtime.consoleAPICalled", &logged);
    assert_eq!(heard.console().read(false).0, [], "nobody asked yet");

    heard.heard(None, "Runtime.consoleAPICalled", &logged);
    let warned = json!({ "type": "warning", "args": [{ "type": "string", "value": "slow" }], "timestamp": 2.0 });
    heard.heard(None, "Runtime.consoleAPICalled", &warned);
    let thrown = json!({ "timestamp": 3.0, "exceptionDetails": {
        "text": "Uncaught", "url": "https://a.example/app.js", "lineNumber": 41,
        "exception": { "description": "TypeError: x is undefined" } } });
    heard.heard(None, "Runtime.exceptionThrown", &thrown);
    let entry = json!({ "entry": { "source": "network", "level": "error",
        "text": "Failed to load resource", "url": "https://a.example/x.png", "timestamp": 4.0 } });
    heard.heard(None, "Log.entryAdded", &entry);
    heard.heard(Some("AB12"), "Runtime.consoleAPICalled", &warned);
    heard.heard(None, "Runtime.executionContextCreated", &json!({}));

    let (messages, dropped) = heard.console().read(true);
    let said: Vec<_> = (messages.iter())
        .map(|message| {
            (
                message.level,
                message.source.as_str(),
                message.text.as_str(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            (ConsoleLevel::Log, "console", "ready 3 Object undefined"),
            (ConsoleLevel::Warning, "console", "slow"),
            (
                ConsoleLevel::Error,
                "exception",
                "TypeError: x is undefined"
            ),
            (ConsoleLevel::Error, "network", "Failed to load resource"),
        ],
        "a frame's own session's console isn't the page's"
    );
    assert_eq!(
        (messages[0].url.as_deref(), messages[0].line),
        (Some("https://a.example/app.js"), Some(10))
    );
    assert_eq!((messages[2].line, messages[3].timestamp), (Some(42), 4.0));
    assert_eq!(dropped, 0);
    assert_eq!(
        heard.console().read(false).0,
        [],
        "cleared by the read that said it"
    );
}

/// Past the messages kept, the oldest go, and the count of those let go says so.
#[test]
fn past_what_is_kept_the_oldest_messages_go() {
    let mut heard = Heard::default();
    heard.console();
    for at in 0..KEPT + 3 {
        let logged =
            json!({ "type": "log", "args": [{ "type": "string", "value": format!("m{at}") }] });
        heard.heard(None, "Runtime.consoleAPICalled", &logged);
    }
    let (messages, dropped) = heard.console().read(false);
    assert_eq!((messages.len(), dropped), (KEPT, 3));
    assert_eq!(messages[0].text, "m3");
}

/// A request is kept from when it's sent, given its response, and told over once it loads,
/// fails or is sent on elsewhere, each hop of a redirect listed.
#[test]
fn a_request_is_followed_from_sent_to_loaded_or_failed() {
    let mut heard = Heard::default();
    let sent = |id: &str, url: &str| json!({ "requestId": id, "request": { "url": url, "method": "GET" }, "type": "Document", "wallTime": 1.5 });
    heard.heard(
        None,
        "Network.requestWillBeSent",
        &sent("1", "https://a.example/"),
    );
    assert_eq!(heard.network().read(false).0, [], "nobody asked yet");

    heard.heard(
        None,
        "Network.requestWillBeSent",
        &sent("2", "http://a.example/"),
    );
    let mut moved = sent("2", "https://a.example/");
    moved["redirectResponse"] = json!({ "status": 301 });
    heard.heard(None, "Network.requestWillBeSent", &moved);
    let response =
        json!({ "requestId": "2", "response": { "status": 200, "mimeType": "text/html" } });
    heard.heard(None, "Network.responseReceived", &response);
    heard.heard(
        None,
        "Network.loadingFinished",
        &json!({ "requestId": "2" }),
    );
    let api = json!({ "requestId": "3", "request": { "url": "https://api.example/x", "method": "POST" }, "type": "Fetch" });
    heard.heard(None, "Network.requestWillBeSent", &api);
    let failed =
        json!({ "requestId": "3", "errorText": "net::ERR_CONNECTION_REFUSED", "canceled": false });
    heard.heard(None, "Network.loadingFailed", &failed);
    heard.heard(
        None,
        "Network.requestWillBeSent",
        &sent("4", "https://a.example/blocked.js"),
    );
    let blocked = json!({ "requestId": "4", "errorText": "net::ERR_BLOCKED_BY_CLIENT", "blockedReason": "inspector" });
    heard.heard(None, "Network.loadingFailed", &blocked);

    let (requests, _) = heard.network().read(false);
    let told: Vec<_> = (requests.iter())
        .map(|request| {
            (
                request.id.as_str(),
                request.method.as_str(),
                request.url.as_str(),
                request.status,
                request.failed.as_deref(),
                request.finished,
            )
        })
        .collect();
    assert_eq!(
        told,
        [
            ("2", "GET", "http://a.example/", Some(301), None, true),
            ("2", "GET", "https://a.example/", Some(200), None, true),
            (
                "3",
                "POST",
                "https://api.example/x",
                None,
                Some("net::ERR_CONNECTION_REFUSED"),
                true
            ),
            (
                "4",
                "GET",
                "https://a.example/blocked.js",
                None,
                Some("inspector"),
                true
            ),
        ]
    );
    assert_eq!(requests[1].mime_type.as_deref(), Some("text/html"));
    assert_eq!(
        (requests[0].resource_type.as_deref(), requests[0].timestamp),
        (Some("Document"), 1500.)
    );
}

/// A frame of another site is known by its target from when it's attached under a session the
/// app knows until it's detached, which takes the frames under it too; the frames attached
/// under a session of the page's `DevTools` aren't the app's.
#[test]
fn frames_attached_are_known_until_detached() {
    let mut heard = Heard::default();
    let attached = |session: &str, target: &str, kind: &str| json!({ "sessionId": session, "targetInfo": { "targetId": target, "type": kind, "url": "https://b.example/" } });
    heard.heard(
        None,
        "Target.attachedToTarget",
        &attached("S1", "F1", "iframe"),
    );
    heard.heard(
        None,
        "Target.attachedToTarget",
        &attached("W1", "T1", "worker"),
    );
    heard.heard(
        Some("S1"),
        "Target.attachedToTarget",
        &attached("S2", "F2", "iframe"),
    );
    heard.heard(
        Some("DEVTOOLS"),
        "Target.attachedToTarget",
        &attached("S3", "F3", "iframe"),
    );
    let frame = |session: &str, parent: Option<&str>| Frame {
        session: session.into(),
        parent: parent.map(Into::into),
    };
    assert_eq!(heard.frame("F1"), Some(&frame("S1", None)));
    assert_eq!(heard.frame("F2"), Some(&frame("S2", Some("S1"))));
    assert_eq!((heard.frame("T1"), heard.frame("F3")), (None, None));
    assert_eq!(heard.frames_under(None), ["F1"]);
    assert_eq!(heard.frames_under(Some("S1")), ["F2"]);

    heard.heard(
        None,
        "Target.detachedFromTarget",
        &json!({ "sessionId": "S1", "targetId": "F1" }),
    );
    assert_eq!(
        (heard.frame("F1"), heard.frame("F2")),
        (None, None),
        "and the frame under it"
    );
}
