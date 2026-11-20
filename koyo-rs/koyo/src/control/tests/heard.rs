//! What an agent hears of a page it reads: its console and its requests, kept from the first
//! call that asks, each call enabling again the domains they come from.

use gpui_kit::TestAppContext;
use koyo_browser_ipc::{AppRequest, HostEvent, PaneId};
use serde_json::{Value, json};

use super::{Rig, Slot, controlled, op, send, taken, terminal};
use crate::browser::FakeHost;
use crate::control::Caller;
use crate::test_support::{draw, repo, select};

/// An agent's page on the fake host: the rig, the host, the agent, the page's pane, and the
/// host's number for its page.
fn agents_page(name: &str, cx: &mut TestAppContext) -> (Rig, FakeHost, Caller, u64, PaneId) {
    let root = repo(name, &[("README.md", "hi\n")]);
    let rig = controlled(name, &[&root], cx);
    select(&rig.shell, &root, cx);
    let host = FakeHost::serve(&format!("{name}-host"), cx);
    let agent = terminal("term_1");
    let opening = json!({ "kind": "browser", "url": "https://a.example/", "select": true });
    let pane = super::call(&agent, "tab_open", opening, cx).unwrap()["pane"]
        .as_u64()
        .unwrap();
    draw(rig.window, cx);
    let opened = (host.asked(cx).into_iter())
        .find_map(|asked| match asked {
            AppRequest::Open { pane, .. } => Some(pane),
            _ => None,
        })
        .expect("the page opens in the host");
    (rig, host, agent, pane, opened)
}

/// Answers the commands the call in `slot` sends, every one with an empty result, until it's
/// answered: its answer, and the methods it sent.
fn answered(host: &FakeHost, slot: &Slot<Value>, cx: &mut TestAppContext) -> (Value, Vec<String>) {
    let mut methods = Vec::new();
    for _ in 0..10 {
        let asked = host.answers(cx, |_, _, _| Ok(json!({})));
        methods.extend(asked.into_iter().filter_map(|asked| match asked {
            AppRequest::Cdp { method, .. } => Some(method),
            _ => None,
        }));
        if let Some(answer) = taken(slot) {
            return (answer.expect("the call succeeds"), methods);
        }
    }
    panic!("the call wasn't answered; it sent {methods:?}");
}

#[gpui_kit::test]
fn an_agent_reads_a_pages_console_from_when_it_first_asks(cx: &mut TestAppContext) {
    let (_rig, host, agent, pane, opened) = agents_page("control-console", cx);
    let logged = |text: &str| HostEvent::CdpEvent {
        pane: opened,
        session: None,
        method: "Runtime.consoleAPICalled".into(),
        params: json!({ "type": "error", "args": [{ "type": "string", "value": text }], "timestamp": 1.0 }),
    };
    host.says(logged("before anyone asked"), cx);
    let reading = |clear: bool| op("browser_console", json!({ "pane": pane, "clear": clear }));

    let console = send(&agent, None, reading(false), cx);
    let (call, method) = (0..10)
        .find_map(|_| {
            host.asked(cx).into_iter().find_map(|asked| match asked {
                AppRequest::Cdp { call, method, .. } => Some((call, method)),
                _ => None,
            })
        })
        .expect("the call enables the page's runtime");
    assert_eq!(method, "Runtime.enable");
    host.says(logged("replayed as the runtime is enabled"), cx);
    let enabled = HostEvent::CdpReply {
        pane: opened,
        call,
        result: Some(json!({})),
        error: None,
    };
    host.says(enabled, cx);
    let (first, methods) = answered(&host, &console, cx);
    assert_eq!(methods, ["Log.enable"]);
    assert_eq!(
        first["messages"][0]["text"],
        "replayed as the runtime is enabled"
    );
    assert_eq!(
        (
            first["messages"].as_array().map(Vec::len),
            first["dropped"].clone()
        ),
        (Some(1), json!(0))
    );

    host.says(logged("later"), cx);
    let cleared = send(&agent, None, reading(true), cx);
    let (second, methods) = answered(&host, &cleared, cx);
    assert_eq!(
        methods,
        ["Runtime.enable", "Log.enable"],
        "enabled again, which the engine takes"
    );
    let texts: Vec<&Value> = (second["messages"].as_array().into_iter().flatten())
        .map(|message| &message["text"])
        .collect();
    assert_eq!(
        texts,
        [
            &json!("replayed as the runtime is enabled"),
            &json!("later")
        ]
    );
    let after = send(&agent, None, reading(false), cx);
    assert_eq!(
        answered(&host, &after, cx).0["messages"],
        json!([]),
        "cleared by the call that said them"
    );
}

#[gpui_kit::test]
fn an_agent_reads_a_pages_requests_from_when_it_first_asks(cx: &mut TestAppContext) {
    let (_rig, host, agent, pane, opened) = agents_page("control-network", cx);
    let event = |method: &str, params: Value| HostEvent::CdpEvent {
        pane: opened,
        session: None,
        method: method.into(),
        params,
    };
    let sent = json!({ "requestId": "6", "request": { "url": "https://a.example/", "method": "GET" }, "wallTime": 1.0 });
    host.says(event("Network.requestWillBeSent", sent), cx);

    let network = send(
        &agent,
        None,
        op("browser_network", json!({ "pane": pane })),
        cx,
    );
    let (none, methods) = answered(&host, &network, cx);
    assert_eq!(
        (methods, none["requests"].clone()),
        (vec!["Network.enable".to_owned()], json!([])),
        "none before"
    );
    let sent = json!({ "requestId": "7", "request": { "url": "https://a.example/api", "method": "POST" }, "type": "Fetch", "wallTime": 2.0 });
    host.says(event("Network.requestWillBeSent", sent), cx);
    let response =
        json!({ "requestId": "7", "response": { "status": 201, "mimeType": "application/json" } });
    host.says(event("Network.responseReceived", response), cx);
    let network = send(
        &agent,
        None,
        op("browser_network", json!({ "pane": pane })),
        cx,
    );
    let requests = answered(&host, &network, cx).0["requests"].clone();
    let request = &requests[0];
    assert_eq!(
        (
            &request["method"],
            &request["url"],
            &request["status"],
            &request["finished"]
        ),
        (
            &json!("POST"),
            &json!("https://a.example/api"),
            &json!(201),
            &json!(false)
        )
    );
    assert_eq!(requests.as_array().map(Vec::len), Some(1));
}
