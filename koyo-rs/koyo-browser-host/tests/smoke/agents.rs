//! An agent's protocol commands on a page, beside the page's own `DevTools`.

use koyo_browser_ipc::{AppRequest, HostEvent, Viewport};
use std::time::Duration;

use crate::support::{Session, launch};

#[test]
#[ignore = "Needs a packaged browser engine; see the module docs"]
fn agents_run_protocol_commands_on_the_page_beside_its_dev_tools() {
    let launch = launch("agent");
    let mut session = Session::start(&launch);
    let viewport = Viewport {
        width: 400,
        height: 300,
        scale: 1.,
    };
    let page = "data:text/html,<title>Agent</title><script>console.log('before-the-agent')</script>\
        <body style='margin:0;background:rgb(255,0,0)'><button>Go</button></body>";
    session
        .host
        .send(&AppRequest::Open {
            pane: 1,
            url: page.into(),
            viewport,
            profile: "smoke-agent".into(),
        })
        .expect("open");
    // A command sent before the page exists waits for it.
    cdp(
        &mut session,
        1,
        "Runtime.evaluate",
        serde_json::json!({"expression": "document.title", "returnByValue": true}),
    );
    let (result, error) = reply(&mut session, 1);
    assert_eq!(error, None);
    assert_eq!(result.expect("a result")["result"]["value"], "Agent");

    // DevTools open beside the agent; each keeps its own answers.
    session
        .host
        .send(&AppRequest::OpenDevTools {
            pane: 1,
            devtools: 2,
            viewport,
            inspect: None,
        })
        .expect("open devtools");
    session.wait("the inspector to attach to the page", |event, _| {
        matches!(event, HostEvent::Page { pane: 2, state } if state.title.starts_with("DevTools - "))
    });
    cdp(&mut session, 2, "No.such", serde_json::json!({}));
    cdp(&mut session, 3, "Runtime.enable", serde_json::Value::Null);
    let (mut failed, mut enabled, mut logged) = (None, false, false);
    session.wait(
        "the bad command, Runtime.enable and the earlier log",
        |event, _| {
            match event {
                HostEvent::CdpReply { call: 2, error, .. } => failed = error.clone(),
                HostEvent::CdpReply {
                    call: 3,
                    error: None,
                    ..
                } => enabled = true,
                HostEvent::CdpEvent { method, params, .. }
                    if method == "Runtime.consoleAPICalled" =>
                {
                    logged |= params["args"][0]["value"] == "before-the-agent";
                }
                _ => {}
            }
            failed.is_some() && enabled && logged
        },
    );
    assert!(failed.expect("an error").contains("wasn't found"));

    // The page closes with a command under way: it is answered all the same.
    cdp(
        &mut session,
        4,
        "Runtime.evaluate",
        serde_json::json!({"expression": "new Promise(() => {})", "awaitPromise": true}),
    );
    session
        .host
        .send(&AppRequest::Close { pane: 1 })
        .expect("close");
    let (_, error) = reply(&mut session, 4);
    assert!(error.is_some());
    session.host.shutdown(Duration::from_secs(10));
    let _ = std::fs::remove_dir_all(launch.profiles.parent().expect("scratch"));
}

/// Sends the agent command `call` on pane 1.
pub fn cdp(session: &mut Session, call: u64, method: &str, params: serde_json::Value) {
    session
        .host
        .send(&AppRequest::Cdp {
            pane: 1,
            call,
            method: method.into(),
            params,
        })
        .expect("cdp");
}

/// The host's reply to the agent command `call`: its result, or its error.
pub fn reply(session: &mut Session, call: u64) -> (Option<serde_json::Value>, Option<String>) {
    let mut answer = None;
    session.wait("the agent's answer", |event, _| {
        if let HostEvent::CdpReply {
            call: answered,
            result,
            error,
            ..
        } = event
            && *answered == call
        {
            answer = Some((result.clone(), error.clone()));
            return true;
        }
        false
    });
    answer.expect("answered")
}
